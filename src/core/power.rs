use gio::prelude::*;
use std::cell::{Cell, RefCell};
use std::os::unix::io::OwnedFd;
use std::rc::Rc;

const LOGIND_BUS_NAME: &str = "org.freedesktop.login1";
const LOGIND_OBJECT_PATH: &str = "/org/freedesktop/login1";
const LOGIND_MANAGER_IFACE: &str = "org.freedesktop.login1.Manager";
const LOGIND_USER_SELF_PATH: &str = "/org/freedesktop/login1/user/self";
const LOGIND_USER_IFACE: &str = "org.freedesktop.login1.User";
const LOGIND_SESSION_IFACE: &str = "org.freedesktop.login1.Session";
const DBUS_PROPS_IFACE: &str = "org.freedesktop.DBus.Properties";

const GNOME_SCREENSAVER_BUS: &str = "org.gnome.ScreenSaver";
const GNOME_SCREENSAVER_PATH: &str = "/org/gnome/ScreenSaver";
const FDO_SCREENSAVER_BUS: &str = "org.freedesktop.ScreenSaver";
const FDO_SCREENSAVER_PATH: &str = "/org/freedesktop/ScreenSaver";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    Suspending,
    Resumed,
    ShuttingDown,
    ScreenLocked,
    ScreenUnlocked,
}

/// Monitors system power & session lifecycle across D-Bus:
/// - `systemd-logind` `PrepareForSleep` + `"sleep"` delay inhibitor (suspend / resume)
/// - `systemd-logind` `PrepareForShutdown` + `"shutdown"` delay inhibitor (poweroff / reboot)
/// - `org.gnome.ScreenSaver`, `org.freedesktop.ScreenSaver`, and `org.freedesktop.login1.Session`
///   lock/unlock signals (deduplicated so every desktop environment works reliably)
pub struct PowerMonitor {
    system_bus: Option<gio::DBusConnection>,
    session_bus: Option<gio::DBusConnection>,
    session_obj_path: Option<String>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
    sleep_inhibitor_fd: Rc<RefCell<Option<OwnedFd>>>,
    shutdown_inhibitor_fd: Rc<RefCell<Option<OwnedFd>>>,
    last_locked: Rc<Cell<bool>>,
}

impl PowerMonitor {
    /// Synchronously queries whether the desktop session is currently locked
    /// (tries GNOME ScreenSaver -> FreeDesktop ScreenSaver -> logind Session.LockedHint).
    pub fn query_screen_locked_now() -> bool {
        if let Ok(session_bus) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
            if let Some(active) = Self::call_screensaver_get_active(
                &session_bus,
                GNOME_SCREENSAVER_BUS,
                GNOME_SCREENSAVER_PATH,
                GNOME_SCREENSAVER_BUS,
            ) {
                return active;
            }
            if let Some(active) = Self::call_screensaver_get_active(
                &session_bus,
                FDO_SCREENSAVER_BUS,
                FDO_SCREENSAVER_PATH,
                FDO_SCREENSAVER_BUS,
            ) {
                return active;
            }
        }

        if let Ok(system_bus) = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
            if let Some(sess_path) = Self::resolve_display_session_path(&system_bus) {
                if let Some(locked) = Self::query_logind_locked_hint(&system_bus, &sess_path) {
                    return locked;
                }
            }
        }

        false
    }

    fn call_screensaver_get_active(
        bus: &gio::DBusConnection,
        bus_name: &str,
        obj_path: &str,
        iface: &str,
    ) -> Option<bool> {
        let reply = bus
            .call_sync(
                Some(bus_name),
                obj_path,
                iface,
                "GetActive",
                None,
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                400,
                gio::Cancellable::NONE,
            )
            .ok()?;
        reply.try_get::<(bool,)>().ok().map(|(v,)| v)
    }

    /// Resolves the current user's graphical display session object path from logind
    /// via `/org/freedesktop/login1/user/self` (`Display` property), which works reliably
    /// even when Ballista runs under `systemd --user` (`user@UID.service`).
    fn resolve_display_session_path(system_bus: &gio::DBusConnection) -> Option<String> {
        let params = (LOGIND_USER_IFACE, "Display").to_variant();
        let reply = system_bus
            .call_sync(
                Some(LOGIND_BUS_NAME),
                LOGIND_USER_SELF_PATH,
                DBUS_PROPS_IFACE,
                "Get",
                Some(&params),
                None,
                gio::DBusCallFlags::NONE,
                500,
                gio::Cancellable::NONE,
            )
            .ok()?;

        // Reply is `(v)` where `v` holds `(s, o)` = `(session_id, object_path)`
        let (variant,) = reply.try_get::<(glib::Variant,)>().ok()?;
        let (sess_id, obj_path) = variant.try_get::<(String, glib::variant::ObjectPath)>().ok()?;
        if sess_id.is_empty() || obj_path.as_str() == "/" {
            None
        } else {
            Some(obj_path.as_str().to_string())
        }
    }

    fn query_logind_locked_hint(system_bus: &gio::DBusConnection, sess_path: &str) -> Option<bool> {
        let params = (LOGIND_SESSION_IFACE, "LockedHint").to_variant();
        let reply = system_bus
            .call_sync(
                Some(LOGIND_BUS_NAME),
                sess_path,
                DBUS_PROPS_IFACE,
                "Get",
                Some(&params),
                None,
                gio::DBusCallFlags::NONE,
                400,
                gio::Cancellable::NONE,
            )
            .ok()?;
        let (variant,) = reply.try_get::<(glib::Variant,)>().ok()?;
        variant.get::<bool>()
    }

    pub fn start<F>(callback: F) -> Self
    where
        F: Fn(PowerEvent) + 'static,
    {
        let cb: Rc<dyn Fn(PowerEvent)> = Rc::new(callback);
        let initially_locked = Self::query_screen_locked_now();
        let last_locked = Rc::new(Cell::new(initially_locked));
        let mut subs: Vec<gio::SignalSubscription> = Vec::new();

        let emit_lock_state = {
            let cb_l = cb.clone();
            let state_l = last_locked.clone();
            Rc::new(move |locked: bool, source: &'static str| {
                let prev = state_l.replace(locked);
                if prev != locked {
                    tracing::info!("Session lock state changed ({source}): locked={locked}");
                    if locked {
                        cb_l(PowerEvent::ScreenLocked);
                    } else {
                        cb_l(PowerEvent::ScreenUnlocked);
                    }
                }
            })
        };

        // 1. System D-Bus: systemd-logind PrepareForSleep, PrepareForShutdown, and Session Lock/Unlock
        let system_bus = match gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
            Ok(conn) => Some(conn),
            Err(e) => {
                tracing::warn!(
                    "System D-Bus unavailable; automatic suspend/shutdown recovery disabled: {e}"
                );
                None
            }
        };

        let mut session_obj_path: Option<String> = None;

        if let Some(ref sys_bus) = system_bus {
            let cb_sleep = cb.clone();
            subs.push(sys_bus.subscribe_to_signal(
                Some(LOGIND_BUS_NAME),
                Some(LOGIND_MANAGER_IFACE),
                Some("PrepareForSleep"),
                Some(LOGIND_OBJECT_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| match signal.parameters.try_get::<(bool,)>() {
                    Ok((true,)) => cb_sleep(PowerEvent::Suspending),
                    Ok((false,)) => cb_sleep(PowerEvent::Resumed),
                    Err(e) => {
                        tracing::warn!("Unexpected PrepareForSleep signal payload: {e}");
                    }
                },
            ));

            let cb_shut = cb.clone();
            subs.push(sys_bus.subscribe_to_signal(
                Some(LOGIND_BUS_NAME),
                Some(LOGIND_MANAGER_IFACE),
                Some("PrepareForShutdown"),
                Some(LOGIND_OBJECT_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| match signal.parameters.try_get::<(bool,)>() {
                    Ok((true,)) => cb_shut(PowerEvent::ShuttingDown),
                    Ok((false,)) => {}
                    Err(e) => {
                        tracing::warn!("Unexpected PrepareForShutdown signal payload: {e}");
                    }
                },
            ));

            if let Some(sess_path) = Self::resolve_display_session_path(sys_bus) {
                tracing::info!("Monitoring systemd-logind session lock signals on {sess_path}");

                let emit_lock = emit_lock_state.clone();
                subs.push(sys_bus.subscribe_to_signal(
                    Some(LOGIND_BUS_NAME),
                    Some(LOGIND_SESSION_IFACE),
                    Some("Lock"),
                    Some(&sess_path),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| emit_lock(true, "logind.Lock"),
                ));

                let emit_unlock = emit_lock_state.clone();
                subs.push(sys_bus.subscribe_to_signal(
                    Some(LOGIND_BUS_NAME),
                    Some(LOGIND_SESSION_IFACE),
                    Some("Unlock"),
                    Some(&sess_path),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| emit_unlock(false, "logind.Unlock"),
                ));

                let emit_prop = emit_lock_state.clone();
                subs.push(sys_bus.subscribe_to_signal(
                    Some(LOGIND_BUS_NAME),
                    Some(DBUS_PROPS_IFACE),
                    Some("PropertiesChanged"),
                    Some(&sess_path),
                    Some(LOGIND_SESSION_IFACE),
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Ok((_iface, changed, _inv)) = signal
                            .parameters
                            .try_get::<(String, std::collections::HashMap<String, glib::Variant>, Vec<String>)>()
                        {
                            if let Some(v) = changed.get("LockedHint") {
                                if let Some(locked) = v.get::<bool>() {
                                    emit_prop(locked, "logind.LockedHint");
                                }
                            }
                        }
                    },
                ));

                session_obj_path = Some(sess_path);
            }

            tracing::info!(
                "Subscribed to systemd-logind PrepareForSleep & PrepareForShutdown monitors"
            );
        }

        // 2. Session D-Bus: GNOME ScreenSaver & FreeDesktop ScreenSaver ActiveChanged
        let session_bus = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
            Ok(conn) => Some(conn),
            Err(e) => {
                tracing::warn!("Session D-Bus unavailable; desktop ScreenSaver monitor disabled: {e}");
                None
            }
        };

        if let Some(ref sess_bus) = session_bus {
            let emit_gnome = emit_lock_state.clone();
            subs.push(sess_bus.subscribe_to_signal(
                Some(GNOME_SCREENSAVER_BUS),
                Some(GNOME_SCREENSAVER_BUS),
                Some("ActiveChanged"),
                Some(GNOME_SCREENSAVER_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let Ok((active,)) = signal.parameters.try_get::<(bool,)>() {
                        emit_gnome(active, "gnome.ScreenSaver");
                    }
                },
            ));

            let emit_fdo = emit_lock_state.clone();
            subs.push(sess_bus.subscribe_to_signal(
                Some(FDO_SCREENSAVER_BUS),
                Some(FDO_SCREENSAVER_BUS),
                Some("ActiveChanged"),
                Some(FDO_SCREENSAVER_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let Ok((active,)) = signal.parameters.try_get::<(bool,)>() {
                        emit_fdo(active, "freedesktop.ScreenSaver");
                    }
                },
            ));

            tracing::info!(
                "Subscribed to GNOME & FreeDesktop ScreenSaver lock monitors (initial locked={initially_locked})"
            );
        }

        let monitor = Self {
            system_bus,
            session_bus,
            session_obj_path,
            subscriptions: RefCell::new(subs),
            sleep_inhibitor_fd: Rc::new(RefCell::new(None)),
            shutdown_inhibitor_fd: Rc::new(RefCell::new(None)),
            last_locked,
        };
        monitor.acquire_sleep_inhibitor();
        monitor.acquire_shutdown_inhibitor();
        monitor
    }

    /// Queries the live lock state and updates internal tracking (useful right after system wake).
    pub fn is_screen_locked(&self) -> bool {
        let locked = if let Some(ref sess_bus) = self.session_bus {
            Self::call_screensaver_get_active(
                sess_bus,
                GNOME_SCREENSAVER_BUS,
                GNOME_SCREENSAVER_PATH,
                GNOME_SCREENSAVER_BUS,
            )
            .or_else(|| {
                Self::call_screensaver_get_active(
                    sess_bus,
                    FDO_SCREENSAVER_BUS,
                    FDO_SCREENSAVER_PATH,
                    FDO_SCREENSAVER_BUS,
                )
            })
        } else {
            None
        }
        .or_else(|| {
            let sys_bus = self.system_bus.as_ref()?;
            let sess_path = self.session_obj_path.as_deref()?;
            Self::query_logind_locked_hint(sys_bus, sess_path)
        })
        .unwrap_or_else(|| self.last_locked.get());

        self.last_locked.set(locked);
        locked
    }

    fn acquire_inhibitor_kind(
        &self,
        what: &str,
        why: &str,
        slot: &Rc<RefCell<Option<OwnedFd>>>,
    ) {
        let Some(ref conn) = self.system_bus else {
            return;
        };
        if slot.borrow().is_some() {
            return;
        }

        let params = (what, "Ballista", why, "delay").to_variant();

        match conn.call_with_unix_fd_list_sync(
            Some(LOGIND_BUS_NAME),
            LOGIND_OBJECT_PATH,
            LOGIND_MANAGER_IFACE,
            "Inhibit",
            Some(&params),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::UnixFDList::NONE,
            gio::Cancellable::NONE,
        ) {
            Ok((_reply, Some(fd_list))) if fd_list.length() > 0 => match fd_list.get(0) {
                Ok(fd) => {
                    *slot.borrow_mut() = Some(fd);
                    tracing::debug!("Acquired systemd-logind '{what}' delay inhibitor lock");
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to extract '{what}' inhibitor FD from logind reply: {e}"
                    );
                }
            },
            Ok(_) => {
                tracing::warn!("systemd-logind Inhibit('{what}') returned no file descriptor");
            }
            Err(e) => {
                tracing::warn!("Could not acquire systemd-logind '{what}' delay inhibitor: {e}");
            }
        }
    }

    pub fn acquire_sleep_inhibitor(&self) {
        self.acquire_inhibitor_kind(
            "sleep",
            "Fade Stream Deck displays to Ballista branding and release USB handles before suspend",
            &self.sleep_inhibitor_fd,
        );
    }

    pub fn release_sleep_inhibitor(&self) {
        if self.sleep_inhibitor_fd.borrow_mut().take().is_some() {
            tracing::debug!("Released systemd-logind sleep delay inhibitor lock");
        }
    }

    pub fn acquire_shutdown_inhibitor(&self) {
        self.acquire_inhibitor_kind(
            "shutdown",
            "Fade Stream Deck displays to Ballista branding before shutdown",
            &self.shutdown_inhibitor_fd,
        );
    }

    pub fn release_shutdown_inhibitor(&self) {
        if self.shutdown_inhibitor_fd.borrow_mut().take().is_some() {
            tracing::debug!("Released systemd-logind shutdown delay inhibitor lock");
        }
    }

    pub fn shutdown(&self) {
        self.release_sleep_inhibitor();
        self.release_shutdown_inhibitor();
        self.subscriptions.borrow_mut().clear();
    }
}
