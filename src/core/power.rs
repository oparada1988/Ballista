use gio::prelude::*;
use std::cell::RefCell;
use std::os::unix::io::OwnedFd;
use std::rc::Rc;

const LOGIND_BUS_NAME: &str = "org.freedesktop.login1";
const LOGIND_OBJECT_PATH: &str = "/org/freedesktop/login1";
const LOGIND_MANAGER_IFACE: &str = "org.freedesktop.login1.Manager";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerEvent {
    Suspending,
    Resumed,
}

/// Monitors systemd-logind `PrepareForSleep` signals on the system D-Bus and holds a `delay`
/// sleep inhibitor lock so Ballista can cleanly dim screens and close libusb/HID handles
/// before `systemd-sleep` freezes `user.slice`, then cleanly recover devices on wake.
pub struct PowerMonitor {
    system_bus: Option<gio::DBusConnection>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    inhibitor_fd: Rc<RefCell<Option<OwnedFd>>>,
}

impl PowerMonitor {
    pub fn start<F>(callback: F) -> Self
    where
        F: Fn(PowerEvent) + 'static,
    {
        let system_bus = match gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!(
                    "System D-Bus unavailable; automatic suspend/resume recovery disabled: {e}"
                );
                return Self {
                    system_bus: None,
                    subscription: RefCell::new(None),
                    inhibitor_fd: Rc::new(RefCell::new(None)),
                };
            }
        };

        let subscription = system_bus.subscribe_to_signal(
            Some(LOGIND_BUS_NAME),
            Some(LOGIND_MANAGER_IFACE),
            Some("PrepareForSleep"),
            Some(LOGIND_OBJECT_PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| match signal.parameters.try_get::<(bool,)>() {
                Ok((true,)) => callback(PowerEvent::Suspending),
                Ok((false,)) => callback(PowerEvent::Resumed),
                Err(e) => {
                    tracing::warn!("Unexpected PrepareForSleep signal payload: {e}");
                }
            },
        );

        tracing::info!("Subscribed to systemd-logind PrepareForSleep power monitor");

        let monitor = Self {
            system_bus: Some(system_bus),
            subscription: RefCell::new(Some(subscription)),
            inhibitor_fd: Rc::new(RefCell::new(None)),
        };
        monitor.acquire_inhibitor();
        monitor
    }

    /// Acquires a `delay` sleep inhibitor lock from systemd-logind so pre-suspend cleanup
    /// completes before `user.slice` is frozen.
    pub fn acquire_inhibitor(&self) {
        let Some(ref conn) = self.system_bus else {
            return;
        };
        if self.inhibitor_fd.borrow().is_some() {
            return;
        }

        let params = (
            "sleep",
            "Ballista",
            "Dim Stream Deck displays and release USB handles cleanly before suspend",
            "delay",
        )
            .to_variant();

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
                    *self.inhibitor_fd.borrow_mut() = Some(fd);
                    tracing::debug!("Acquired systemd-logind sleep delay inhibitor lock");
                }
                Err(e) => {
                    tracing::warn!("Failed to extract sleep inhibitor FD from logind reply: {e}");
                }
            },
            Ok(_) => {
                tracing::warn!("systemd-logind Inhibit returned no file descriptor");
            }
            Err(e) => {
                tracing::warn!("Could not acquire systemd-logind sleep delay inhibitor: {e}");
            }
        }
    }

    /// Releases the `delay` sleep inhibitor FD so systemd can proceed with suspend immediately.
    pub fn release_inhibitor(&self) {
        if self.inhibitor_fd.borrow_mut().take().is_some() {
            tracing::debug!("Released systemd-logind sleep delay inhibitor lock");
        }
    }

    pub fn shutdown(&self) {
        self.release_inhibitor();
        *self.subscription.borrow_mut() = None;
    }
}
