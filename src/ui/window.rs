use super::canvas::Canvas;
use super::header::Header;
use super::inspector::{rgba_to_hex, InspectorTarget, PropertyInspector};
use super::settings::create_settings_window;
use super::sidebar::Sidebar;
use super::store::create_store_window;
use crate::core::{
    ConnectedDevice, HardwareEvent, HardwareManager, PluginManager, PowerEvent, PowerMonitor,
    TrayCommand, TrayController,
};
use crate::models::{
    CloseBehavior, DeviceKind, KeyAction, KeyAppearance, Profile, ProfileStore, TextVerticalAlign,
    WindowState,
};
use crate::native_sdk::{
    BrightnessOp, ConfigChange, HostCommand, LabelPos, MultiActionStep, NativePluginHost, PageNav,
};
use adw::prelude::*;
use adw::{ApplicationWindow, ToolbarView};
use gtk::gdk::RGBA;
use gtk::{Orientation, Paned};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

type NativeCommandDispatcher =
    Rc<dyn Fn(String, String, Vec<HostCommand>, Option<HashMap<String, serde_json::Value>>)>;

pub struct MainWindow {
    pub window: ApplicationWindow,
    pub header: Header,
    pub canvas: Rc<Canvas>,
    pub inspector: Rc<PropertyInspector>,
    pub sidebar: Rc<Sidebar>,
    pub profile: Rc<RefCell<Profile>>,
    pub profiles: Rc<RefCell<Vec<Profile>>>,
    pub active_profile_id: Rc<RefCell<String>>,
    pub plugin_manager: Rc<RefCell<PluginManager>>,
    pub native_host: NativePluginHost,
    pub hardware_manager: Rc<RefCell<HardwareManager>>,
    pub connected_devices: Rc<RefCell<Vec<ConnectedDevice>>>,
    pub active_device_idx: Rc<RefCell<usize>>,
    pub tray: Rc<RefCell<TrayController>>,
    pub power_monitor: Rc<RefCell<Option<Rc<PowerMonitor>>>>,
}

impl MainWindow {
    pub fn new(
        app: &adw::Application,
        hold_guard: Rc<RefCell<Option<gio::ApplicationHoldGuard>>>,
    ) -> Self {
        let boot_t0 = std::time::Instant::now();

        // Phase 1 — Instant Hardware Boot:
        // Initialize Native Rust Plugin Host FIRST (<1ms) so saved built-in Ballista actions
        // (Clock, System Metrics, Media, Folder, Launcher, etc.) render immediately on hardware boot!
        let state = WindowState::load();
        let profile_store = ProfileStore::load_or_default();
        let native_host = NativePluginHost::new();
        native_host.register_plugin(Box::new(
            crate::plugins::ballista_core::BallistaCorePlugin::new(),
        ));

        let hardware_manager = Rc::new(RefCell::new(HardwareManager::new()));
        hardware_manager.borrow().set_native_host(native_host.clone());
        let devices = hardware_manager.borrow().scan_devices();

        let initially_locked = PowerMonitor::query_screen_locked_now();
        let boot_targets: Vec<_> = devices
            .iter()
            .map(|d| {
                let skey = d.stable_key();
                let prof = profile_store.active_profile_for(&skey, d.kind);
                let bright = state.brightness_for(&skey);
                let p_names: Vec<String> = profile_store
                    .profiles
                    .iter()
                    .filter(|p| p.device_kind == d.kind)
                    .map(|p| p.name.clone())
                    .collect();
                native_host.set_deck_state(
                    &skey,
                    p_names,
                    prof.name.clone(),
                    prof.current_page_idx,
                    prof.pages.len(),
                    bright,
                );
                if let Some(pg) = prof.current_page() {
                    let _ = native_host.sync_page(&skey, pg);
                }
                (d.dev_path.clone(), skey, d.kind, prof, bright)
            })
            .collect();
        if !boot_targets.is_empty() {
            if initially_locked {
                let lock_targets: Vec<_> = boot_targets
                    .iter()
                    .map(|(p, _, kind, _, bright)| (p.clone(), *kind, *bright))
                    .collect();
                hardware_manager
                    .borrow()
                    .fade_to_branding_all_async(lock_targets, false);
            } else {
                hardware_manager.borrow().sync_all_devices_async(boot_targets);
            }
        }
        tracing::info!(
            "Phase-1 boot hardware dispatch queued for {} device(s) (locked={initially_locked}) in {:?}",
            devices.len(),
            boot_t0.elapsed()
        );

        let profiles = Rc::new(RefCell::new(profile_store.profiles));
        let active_by_device = Rc::new(RefCell::new(profile_store.active_by_device));
        let connected_devices = Rc::new(RefCell::new(devices));
        let active_device_idx = Rc::new(RefCell::new(0));

        // Phase 2 — Non-blocking Python StreamController bridge & plugin discovery
        let plugin_manager = Rc::new(RefCell::new(PluginManager::new()));

        let initial_dev = connected_devices.borrow().first().cloned();
        let initial_kind = initial_dev
            .as_ref()
            .map(|d| d.kind)
            .unwrap_or(DeviceKind::StreamDeckMK2);
        let initial_dev_key = initial_dev
            .as_ref()
            .map(|d| d.stable_key())
            .unwrap_or_else(|| format!("{initial_kind:?}"));

        let initial_p = {
            let store_view = ProfileStore {
                profiles: profiles.borrow().clone(),
                active_by_device: active_by_device.borrow().clone(),
            };
            store_view.active_profile_for(&initial_dev_key, initial_kind)
        };
        let active_profile_id = Rc::new(RefCell::new(initial_p.id.clone()));
        let profile = Rc::new(RefCell::new(initial_p));

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Ballista - Elgato Controller")
            .default_width(state.window_width)
            .default_height(state.window_height)
            .build();

        window.add_css_class("ballista-main");

        let toolbar_view = ToolbarView::new();

        // 2. Header
        let header = Header::new();
        header.populate_devices(&connected_devices.borrow());
        toolbar_view.add_top_bar(&header.header_bar);

        // 3. Resizable Paned Split
        let paned = Paned::new(Orientation::Horizontal);
        paned.set_vexpand(true);
        paned.set_hexpand(true);

        let work_paned = Paned::new(Orientation::Vertical);
        work_paned.set_vexpand(true);
        work_paned.set_hexpand(true);

        // 4. Canvas (Guaranteed minimum height to prevent hiding button grid)
        let canvas = Rc::new(Canvas::new(initial_kind));
        canvas.set_native_host(native_host.clone());
        canvas.set_active_device_key(&initial_dev_key);
        canvas.container.set_size_request(-1, 280);
        work_paned.set_start_child(Some(&canvas.container));
        work_paned.set_resize_start_child(true);
        work_paned.set_shrink_start_child(false);

        // 5. Property Inspector in Scrollable Container
        let inspector = Rc::new(PropertyInspector::new());
        inspector.set_active_device_key(&initial_dev_key);
        let active_plugin_settings: super::plugin_settings::ActivePluginSettingsRegistry =
            Rc::new(RefCell::new(None));
        let win_plugin_settings = window.clone();
        let pm_plugin_settings = plugin_manager.clone();
        let active_ps_cb = active_plugin_settings.clone();
        let open_plugin_settings: Rc<dyn Fn(String, String)> = Rc::new(move |pid, pname| {
            super::plugin_settings::open_plugin_settings_dialog(
                &win_plugin_settings,
                &pid,
                &pname,
                pm_plugin_settings.clone(),
                active_ps_cb.clone(),
            );
        });
        let open_ps_insp = open_plugin_settings.clone();
        *inspector.on_open_plugin_settings.borrow_mut() = Some(std::boxed::Box::new(move |pid, pname| {
            open_ps_insp(pid, pname);
        }));
        inspector.set_native_host(native_host.clone());
        let insp_scroll = gtk::ScrolledWindow::new();
        insp_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        insp_scroll.set_child(Some(&inspector.container));
        insp_scroll.set_min_content_height(160);
        insp_scroll.set_size_request(-1, 160);

        work_paned.set_end_child(Some(&insp_scroll));
        work_paned.set_resize_end_child(true);
        work_paned.set_shrink_end_child(false);
        work_paned.set_position(state.work_paned_position.max(280));

        // 6. Sidebar (Full Height) — shares active drag capabilities with Canvas for real-time drop exclusivity
        let mut sidebar_inner = Sidebar::new();
        sidebar_inner.set_drag_caps_cell(canvas.active_drag_caps.clone());
        let sidebar = Rc::new(sidebar_inner);
        paned.set_start_child(Some(&work_paned));
        paned.set_resize_start_child(true);
        paned.set_shrink_start_child(false);

        paned.set_end_child(Some(&sidebar.container));
        paned.set_resize_end_child(true);
        paned.set_shrink_end_child(false);
        paned.set_position(state.sidebar_position);

        // Window State Persistence & Dynamic Canvas Scaling:
        let win_pos1 = window.clone();
        let paned_pos1 = paned.clone();
        let canvas_scale_paned = canvas.clone();
        let prof_scale_paned = profile.clone();
        work_paned.connect_position_notify(move |p| {
            let pos = p.position().max(280);
            if p.position() < 280 {
                p.set_position(280);
            }
            canvas_scale_paned.update_scale(pos);
            canvas_scale_paned.render_profile_page(&prof_scale_paned.borrow());
            let w = win_pos1.width().max(600);
            let h = win_pos1.height().max(400);
            let s = WindowState {
                window_width: w,
                window_height: h,
                sidebar_position: paned_pos1.position(),
                work_paned_position: pos,
                ..WindowState::load()
            };
            s.save();
        });

        let win_pos2 = window.clone();
        let work_paned_pos2 = work_paned.clone();
        paned.connect_position_notify(move |p| {
            let pos = p.position();
            let w = win_pos2.width().max(600);
            let h = win_pos2.height().max(400);
            let s = WindowState {
                window_width: w,
                window_height: h,
                sidebar_position: pos,
                work_paned_position: work_paned_pos2.position(),
                ..WindowState::load()
            };
            s.save();
        });

        // Unified Atomic Profile Persistence Helper (syncs active profile back into `profiles` & writes profiles.json)
        let persist_profiles: Rc<dyn Fn()> = {
            let p_cur = profile.clone();
            let profs = profiles.clone();
            let act_map = active_by_device.clone();
            let devs = connected_devices.clone();
            let act_idx = active_device_idx.clone();
            Rc::new(move || {
                let current_p = p_cur.borrow().clone();
                {
                    let mut list = profs.borrow_mut();
                    if let Some(slot) = list.iter_mut().find(|p| p.id == current_p.id) {
                        *slot = current_p.clone();
                    } else {
                        list.push(current_p.clone());
                    }
                }
                {
                    let d_lock = devs.borrow();
                    if let Some(dev) = d_lock.get(*act_idx.borrow()) {
                        act_map
                            .borrow_mut()
                            .insert(dev.stable_key(), current_p.id.clone());
                    }
                }
                ProfileStore::save_profiles(&profs.borrow(), &act_map.borrow());
            })
        };

        // Native Linux StatusNotifierItem System Tray (GNOME/KDE/Waybar/Hyprland/XFCE)
        let (tray_tx, tray_rx) = async_channel::unbounded::<TrayCommand>();
        let initial_tray_names: Vec<String> = connected_devices
            .borrow()
            .iter()
            .map(|d| d.name.clone())
            .collect();
        let tray = Rc::new(RefCell::new(TrayController::start(
            tray_tx,
            initial_tray_names,
        )));

        // Full Graceful Quit Helper (triggered by Tray "Quit Ballista", Ctrl+Q, Close Dialog "Quit",
        // or SIGTERM/SIGINT on logout/service stop)
        let power_monitor_cell: Rc<RefCell<Option<Rc<PowerMonitor>>>> = Rc::new(RefCell::new(None));
        let is_quitting = Rc::new(Cell::new(false));
        let shutdown_transition_done = Rc::new(Cell::new(false));
        let perform_graceful_exit: Rc<dyn Fn(bool)> = {
            let quitting = is_quitting.clone();
            let shut_done = shutdown_transition_done.clone();
            let win_q = window.clone();
            let paned_q = paned.clone();
            let work_paned_q = work_paned.clone();
            let pm_q = plugin_manager.clone();
            let hw_q = hardware_manager.clone();
            let tray_q = tray.clone();
            let power_q = power_monitor_cell.clone();
            let persist_q = persist_profiles.clone();
            let hold_q = hold_guard.clone();
            let app_q = app.clone();
            Rc::new(move |fade_to_zero_after_hold: bool| {
                if quitting.replace(true) {
                    return;
                }
                tracing::info!(
                    "Shutting down Ballista cleanly (fade_to_zero={fade_to_zero_after_hold})..."
                );
                let w = win_q.width().max(600);
                let h = win_q.height().max(400);
                let s = WindowState {
                    window_width: w,
                    window_height: h,
                    sidebar_position: paned_q.position(),
                    work_paned_position: work_paned_q.position(),
                    ..WindowState::load()
                };
                s.save();
                persist_q();
                if !shut_done.replace(true) {
                    hw_q.borrow().prepare_for_shutdown(fade_to_zero_after_hold);
                } else {
                    hw_q.borrow().shutdown();
                }
                if let Some(ref pm) = *power_q.borrow() {
                    pm.shutdown();
                }
                tray_q.borrow().shutdown();
                pm_q.borrow().bridge.shutdown();
                *hold_q.borrow_mut() = None;
                app_q.quit();
            })
        };
        let perform_full_quit: Rc<dyn Fn()> = {
            let exit_cb = perform_graceful_exit.clone();
            Rc::new(move || exit_cb(true))
        };

        // Handle SIGTERM / SIGINT cleanly (async-signal-safe atomic flag polled on GLib main loop):
        // - If PrepareForShutdown already ran Option A (fade -> logo -> 0%), exit immediately.
        // - Otherwise (e.g. desktop logout to GDM or service restart), fade into Ballista branding
        //   and keep it lit for the GDM login screen before closing USB handles.
        {
            use std::sync::atomic::{AtomicI32, Ordering};
            static PENDING_EXIT_SIGNAL: AtomicI32 = AtomicI32::new(0);

            extern "C" fn handle_exit_signal(sig: libc::c_int) {
                PENDING_EXIT_SIGNAL.store(sig, Ordering::SeqCst);
            }

            unsafe {
                let mut sa: libc::sigaction = std::mem::zeroed();
                sa.sa_sigaction = handle_exit_signal as *const () as usize;
                sa.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut sa.sa_mask);
                libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
                libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
            }

            let exit_sig = perform_graceful_exit.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let sig = PENDING_EXIT_SIGNAL.swap(0, Ordering::SeqCst);
                if sig == libc::SIGTERM {
                    tracing::info!("Received SIGTERM — running graceful Ballista branding exit");
                    exit_sig(false);
                    return glib::ControlFlow::Break;
                } else if sig == libc::SIGINT {
                    tracing::info!("Received SIGINT — running graceful Ballista branding exit");
                    exit_sig(true);
                    return glib::ControlFlow::Break;
                }
                glib::ControlFlow::Continue
            });
        }

        // Register Ctrl+Q application quit shortcut
        {
            let quit_action = gio::SimpleAction::new("quit", None);
            let quit_cb = perform_full_quit.clone();
            quit_action.connect_activate(move |_, _| {
                quit_cb();
            });
            app.add_action(&quit_action);
            app.set_accels_for_action("app.quit", &["<Primary>q"]);
        }

        // Window Close Request: Ask / Minimize to Tray / Quit cleanly
        let win_close = window.clone();
        let paned_close = paned.clone();
        let work_paned_close = work_paned.clone();
        let persist_close = persist_profiles.clone();
        let quit_close = perform_full_quit.clone();
        let is_quitting_close = is_quitting.clone();
        window.connect_close_request(move |_| {
            if is_quitting_close.get() {
                return glib::Propagation::Proceed;
            }
            let w = win_close.width().max(600);
            let h = win_close.height().max(400);
            let mut st = WindowState::load();
            st.window_width = w;
            st.window_height = h;
            st.sidebar_position = paned_close.position();
            st.work_paned_position = work_paned_close.position();
            st.save();
            persist_close();

            match st.close_behavior {
                CloseBehavior::MinimizeToTray => {
                    win_close.set_visible(false);
                    glib::Propagation::Stop
                }
                CloseBehavior::Quit => {
                    quit_close();
                    glib::Propagation::Proceed
                }
                CloseBehavior::Ask => {
                    let remember_cb = gtk::CheckButton::with_label("Remember my choice");
                    remember_cb.set_active(true);
                    remember_cb.set_halign(gtk::Align::Center);
                    remember_cb.set_margin_top(6);

                    let dialog = adw::AlertDialog::builder()
                        .heading("Keep Ballista Running in Background?")
                        .body("Closing the window can hide Ballista to the system tray so your Stream Deck keys, dials, and plugins remain active.")
                        .extra_child(&remember_cb)
                        .build();
                    dialog.add_responses(&[
                        ("cancel", "Cancel"),
                        ("quit", "Quit Ballista"),
                        ("tray", "Keep in Tray"),
                    ]);
                    dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);
                    dialog.set_response_appearance("tray", adw::ResponseAppearance::Suggested);
                    dialog.set_default_response(Some("tray"));
                    dialog.set_close_response("cancel");

                    let win_dlg = win_close.clone();
                    let quit_dlg = quit_close.clone();
                    let rem_dlg = remember_cb.clone();
                    dialog.choose(Some(&win_close), gio::Cancellable::NONE, move |resp| {
                        let remember = rem_dlg.is_active();
                        match resp.as_str() {
                            "tray" => {
                                if remember {
                                    let mut s = WindowState::load();
                                    s.close_behavior = CloseBehavior::MinimizeToTray;
                                    s.save();
                                }
                                win_dlg.set_visible(false);
                            }
                            "quit" => {
                                if remember {
                                    let mut s = WindowState::load();
                                    s.close_behavior = CloseBehavior::Quit;
                                    s.save();
                                }
                                quit_dlg();
                            }
                            _ => {}
                        }
                    });
                    glib::Propagation::Stop
                }
            }
        });

        toolbar_view.set_content(Some(&paned));
        window.set_content(Some(&toolbar_view));

        // Populate Plugin Manager into Inspector & scan freedesktop desktop apps asynchronously (zero startup delay)
        *inspector.plugin_manager.borrow_mut() = Some(plugin_manager.borrow().clone());
        {
            let (apps_tx, apps_rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let apps = PluginManager::collect_installed_apps();
                let _ = apps_tx.send_blocking(apps);
            });
            let pm_apps = plugin_manager.clone();
            let insp_apps = inspector.clone();
            glib::spawn_future_local(async move {
                if let Ok(apps) = apps_rx.recv().await {
                    insp_apps.populate_installed_apps(&apps);
                    pm_apps.borrow_mut().installed_apps = apps;
                    *insp_apps.plugin_manager.borrow_mut() = Some(pm_apps.borrow().clone());
                }
            });
        }

        let prof_param = profile.clone();
        let insp_param = inspector.clone();
        let canvas_param = canvas.clone();
        let hw_param = hardware_manager.clone();
        let devs_param = connected_devices.clone();
        let act_param = active_device_idx.clone();
        let pm_param = plugin_manager.clone();
        let persist_param = persist_profiles.clone();

        *inspector.on_parameter_changed.borrow_mut() = Some(std::boxed::Box::new(move |key, val| {
            let target = *insp_param.current_target.borrow();
            let mut appearance_affected = false;
            let mut settings_json = HashMap::new();
            {
                let mut prof = prof_param.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.parameters.insert(key.clone(), val.clone());
                    let typed_val = if val == "true" {
                        serde_json::Value::Bool(true)
                    } else if val == "false" {
                        serde_json::Value::Bool(false)
                    } else if let Ok(i) = val.parse::<i64>() {
                        serde_json::Value::Number(i.into())
                    } else if let Ok(f) = val.parse::<f64>() {
                        serde_json::Number::from_f64(f)
                            .map(serde_json::Value::Number)
                            .unwrap_or_else(|| serde_json::Value::String(val.clone()))
                    } else {
                        serde_json::Value::String(val.clone())
                    };
                    action.raw_settings.insert(key.clone(), typed_val);
                    settings_json = action.merged_settings_json();

                    // Parameter changes affecting visual display
                    if key == "custom_label" || key == "label" || key == "title" {
                        action.appearance.title = val.clone();
                        for s in action.states.iter_mut() {
                            s.title = val.clone();
                        }
                        appearance_affected = true;
                    } else if key == "show_label" {
                        let show = val.parse::<bool>().unwrap_or(true);
                        action.appearance.show_title = show;
                        for s in action.states.iter_mut() {
                            s.show_title = show;
                        }
                        appearance_affected = true;
                    } else if key == "icon_name" || key == "icon_path" {
                        if let Some(resolved) = crate::ui::canvas::resolve_icon_path(&val, 128) {
                            action.appearance.icon_path = Some(resolved.clone());
                            for s in action.states.iter_mut() {
                                s.icon_path = Some(resolved.clone());
                            }
                        }
                        appearance_affected = true;
                    } else if key == "channel" || key == "server" {
                        if !val.is_empty() && (action.appearance.title.is_empty() || action.appearance.title == action.action_name) {
                            action.appearance.title = val.clone();
                            for s in action.states.iter_mut() {
                                s.title = val.clone();
                            }
                            appearance_affected = true;
                        }
                    }
                }
            }

            // Forward parameter update to StreamController compatibility bridge
            let dkey = active_device_key_for(&devs_param, &act_param, prof_param.borrow().device_kind);
            pm_param.borrow().bridge.update_action_settings(
                &scoped_bridge_slot(&dkey, &format_target_slot(target)),
                settings_json,
            );

            // Immediately re-render canvas and sync hardware for any parameter change
            persist_param();
            let prof = prof_param.borrow();
            canvas_param.render_profile_page(&prof);
            if appearance_affected {
                if let Some(act) = get_target_ref(&prof, target) {
                    insp_param.update_preview_only(act);
                }
            }
            drop(prof);
            Self::sync_hardware(&devs_param, *act_param.borrow(), &hw_param, &prof_param.borrow());
        }));

        // ===================== Native Rust Plugin Host Command Dispatcher =====================
        let deck_sleeping: Rc<RefCell<HashMap<String, bool>>> = Rc::new(RefCell::new(HashMap::new()));
        let system_sleeping = Rc::new(Cell::new(false));
        let screen_locked = Rc::new(Cell::new(initially_locked));
        let (multi_ui_tx, multi_ui_rx) = async_channel::unbounded::<(String, MultiActionStep)>();

        let dispatch_native: NativeCommandDispatcher = {
            let prof_nat = profile.clone();
            let profs_nat = profiles.clone();
            let act_id_nat = active_profile_id.clone();
            let act_by_dev_nat = active_by_device.clone();
            let hdr_nat = header.clone();
            let canvas_nat = canvas.clone();
            let insp_nat = inspector.clone();
            let pm_nat = plugin_manager.clone();
            let nh_nat = native_host.clone();
            let hw_nat = hardware_manager.clone();
            let devs_nat = connected_devices.clone();
            let act_nat = active_device_idx.clone();
            let win_nat = window.clone();
            let persist_nat = persist_profiles.clone();
            let sleep_nat = deck_sleeping.clone();
            let sys_sleep_nat = system_sleeping.clone();
            let multi_tx = multi_ui_tx.clone();

            Rc::new(move |device_key, slot, cmds, updated_settings| {
                let target = parse_slot_target(&slot);
                let active_gui_key = active_device_key_for(
                    &devs_nat,
                    &act_nat,
                    prof_nat.borrow().device_kind,
                );
                let is_gui_deck = device_key == active_gui_key;
                let target_dev = devs_nat
                    .borrow()
                    .iter()
                    .find(|d| d.stable_key() == device_key)
                    .cloned();
                let target_kind = target_dev
                    .as_ref()
                    .map(|d| d.kind)
                    .unwrap_or_else(|| prof_nat.borrow().device_kind);

                let mutate_dev_profile = |f: &mut dyn FnMut(&mut Profile)| {
                    if is_gui_deck {
                        let mut prof = prof_nat.borrow_mut();
                        f(&mut prof);
                    } else {
                        let mut prof_list = profs_nat.borrow_mut();
                        if let Some(p_idx) = find_profile_idx_for_device(
                            &prof_list,
                            &act_by_dev_nat.borrow(),
                            &device_key,
                            target_kind,
                        ) {
                            f(&mut prof_list[p_idx]);
                            if prof_list[p_idx].id == prof_nat.borrow().id {
                                *prof_nat.borrow_mut() = prof_list[p_idx].clone();
                            }
                        }
                    }
                };

                let read_dev_profile = || -> Option<Profile> {
                    if is_gui_deck {
                        Some(prof_nat.borrow().clone())
                    } else {
                        let prof_list = profs_nat.borrow();
                        find_profile_idx_for_device(
                            &prof_list,
                            &act_by_dev_nat.borrow(),
                            &device_key,
                            target_kind,
                        )
                        .map(|i| prof_list[i].clone())
                    }
                };

                let persist_target = || {
                    if is_gui_deck {
                        persist_nat();
                    } else {
                        ProfileStore::save_profiles(&profs_nat.borrow(), &act_by_dev_nat.borrow());
                    }
                };

                let mut need_persist = false;
                let mut need_full_render = false;
                let mut need_single_redraw = false;
                let mut need_full_hw_sync = false;
                let mut need_refresh_rows = false;
                let mut need_sync_page = false;

                // Apply updated native settings back into KeyAction persistence model (persist only if changed)
                if let Some(settings_map) = updated_settings {
                    let mut changed_any = false;
                    mutate_dev_profile(&mut |prof| {
                        if let Some(page) = prof.current_page_mut() {
                            if let Some(action) = match target {
                                InspectorTarget::Key(k) => page.keys.get_mut(&k),
                                InspectorTarget::Dial(d) => page.dial_actions.get_mut(&d),
                                InspectorTarget::Touchscreen => page.touchscreen_action.as_mut(),
                            } {
                                for (k, v) in &settings_map {
                                    if action.raw_settings.get(k) != Some(v) {
                                        changed_any = true;
                                        let val_str = match v {
                                            serde_json::Value::String(s) => s.clone(),
                                            serde_json::Value::Number(n) => n.to_string(),
                                            serde_json::Value::Bool(b) => b.to_string(),
                                            serde_json::Value::Null => String::new(),
                                            other => other.to_string(),
                                        };
                                        action.raw_settings.insert(k.clone(), v.clone());
                                        action.parameters.insert(k.clone(), val_str);
                                    }
                                }
                            }
                        }
                    });
                    if changed_any {
                        need_persist = true;
                    }
                }

                for cmd in cmds {
                    match cmd {
                        HostCommand::PersistSettings => {
                            need_persist = true;
                        }
                        HostCommand::DirtyVisual => {
                            need_single_redraw = true;
                        }
                        HostCommand::RefreshConfigRows => {
                            need_refresh_rows = true;
                        }
                        HostCommand::SetLabel {
                            position,
                            text,
                            font_size,
                            color,
                        } => {
                            mutate_dev_profile(&mut |prof| {
                                if let Some(page) = prof.current_page_mut() {
                                    if let Some(action) = match target {
                                        InspectorTarget::Key(k) => page.keys.get_mut(&k),
                                        InspectorTarget::Dial(d) => page.dial_actions.get_mut(&d),
                                        InspectorTarget::Touchscreen => page.touchscreen_action.as_mut(),
                                    } {
                                        let apply = |app: &mut KeyAppearance| {
                                            let opt = if text.is_empty() { None } else { Some(text.clone()) };
                                            match position {
                                                LabelPos::Top => app.top_label = opt,
                                                LabelPos::Center => app.center_label = opt,
                                                LabelPos::Bottom => app.bottom_label = opt,
                                            }
                                            if let Some(fs) = font_size {
                                                app.font_size = fs;
                                            }
                                            if let Some(ref c) = color {
                                                app.text_color = c.clone();
                                            }
                                        };
                                        apply(&mut action.appearance);
                                        let st_i = action.current_state;
                                        if st_i < action.states.len() {
                                            apply(&mut action.states[st_i]);
                                        }
                                        need_full_render = true;
                                        need_single_redraw = true;
                                    }
                                }
                            });
                        }
                        HostCommand::SetTitle(new_title) => {
                            mutate_dev_profile(&mut |prof| {
                                if let Some(page) = prof.current_page_mut() {
                                    if let Some(action) = match target {
                                        InspectorTarget::Key(k) => page.keys.get_mut(&k),
                                        InspectorTarget::Dial(d) => page.dial_actions.get_mut(&d),
                                        InspectorTarget::Touchscreen => page.touchscreen_action.as_mut(),
                                    } {
                                        action.appearance.title = new_title.clone();
                                        action.appearance.show_title = !new_title.is_empty();
                                        for st in action.states.iter_mut() {
                                            st.title = new_title.clone();
                                            st.show_title = !new_title.is_empty();
                                        }
                                        need_persist = true;
                                        need_full_render = true;
                                        need_single_redraw = true;
                                    }
                                }
                            });
                        }
                        HostCommand::SetMedia { path, icon_name } => {
                            mutate_dev_profile(&mut |prof| {
                                if let Some(page) = prof.current_page_mut() {
                                    if let Some(action) = match target {
                                        InspectorTarget::Key(k) => page.keys.get_mut(&k),
                                        InspectorTarget::Dial(d) => page.dial_actions.get_mut(&d),
                                        InspectorTarget::Touchscreen => page.touchscreen_action.as_mut(),
                                    } {
                                        action.appearance.icon_path = path.clone();
                                        for st in action.states.iter_mut() {
                                            st.icon_path = path.clone();
                                        }
                                        if let Some(ref iname) = icon_name {
                                            action.parameters.insert("icon_name".into(), iname.clone());
                                        }
                                        need_persist = true;
                                        need_full_render = true;
                                        need_single_redraw = true;
                                    }
                                }
                            });
                        }
                        HostCommand::SetBackgroundColor(bg) => {
                            mutate_dev_profile(&mut |prof| {
                                if let Some(page) = prof.current_page_mut() {
                                    if let Some(action) = match target {
                                        InspectorTarget::Key(k) => page.keys.get_mut(&k),
                                        InspectorTarget::Dial(d) => page.dial_actions.get_mut(&d),
                                        InspectorTarget::Touchscreen => page.touchscreen_action.as_mut(),
                                    } {
                                        action.appearance.bg_color = bg.clone();
                                        for st in action.states.iter_mut() {
                                            st.bg_color = bg.clone();
                                        }
                                        need_persist = true;
                                        need_full_render = true;
                                        need_single_redraw = true;
                                    }
                                }
                            });
                        }
                        HostCommand::EnterFolder {
                            folder_id,
                            folder_name,
                        } => {
                            mutate_dev_profile(&mut |prof| {
                                prof.enter_folder(&folder_id, &folder_name);
                            });
                            need_persist = true;
                            need_sync_page = true;
                            need_full_render = true;
                            need_full_hw_sync = true;
                        }
                        HostCommand::ExitFolder => {
                            let mut exited = false;
                            mutate_dev_profile(&mut |prof| {
                                exited = prof.exit_folder();
                            });
                            if exited {
                                need_sync_page = true;
                                need_full_render = true;
                                need_full_hw_sync = true;
                            }
                        }
                        HostCommand::SwitchPage(nav) => {
                            let mut changed = false;
                            mutate_dev_profile(&mut |prof| {
                                changed = match nav {
                                    PageNav::Next => prof.next_page(),
                                    PageNav::Prev => prof.prev_page(),
                                    PageNav::Index(i) => prof.set_page(i),
                                };
                            });
                            if changed {
                                need_persist = true;
                                need_sync_page = true;
                                need_full_render = true;
                                need_full_hw_sync = true;
                            }
                        }
                        HostCommand::SwitchProfile(target_name) => {
                            persist_target();
                            let profs_b = profs_nat.borrow();
                            let found = profs_b
                                .iter()
                                .find(|p| p.device_kind == target_kind && p.name == target_name)
                                .cloned();
                            if let Some(next_p) = found {
                                act_by_dev_nat
                                    .borrow_mut()
                                    .insert(device_key.clone(), next_p.id.clone());
                                if is_gui_deck {
                                    *act_id_nat.borrow_mut() = next_p.id.clone();
                                    *prof_nat.borrow_mut() = next_p.clone();
                                }
                                let p_names: Vec<String> = profs_b
                                    .iter()
                                    .filter(|p| p.device_kind == target_kind)
                                    .map(|p| p.name.clone())
                                    .collect();
                                let sel_pos = p_names
                                    .iter()
                                    .position(|n| n == &next_p.name)
                                    .unwrap_or(0);
                                ProfileStore::save_profiles(&profs_b, &act_by_dev_nat.borrow());
                                drop(profs_b);
                                if is_gui_deck {
                                    hdr_nat.update_profiles(&p_names, sel_pos);
                                }
                                need_sync_page = true;
                                need_full_render = true;
                                need_full_hw_sync = true;
                            }
                        }
                        HostCommand::SetBrightness(op) => {
                            if let Some(ref dev) = target_dev {
                                let skey = dev.stable_key();
                                let mut ws = WindowState::load();
                                let cur = ws.brightness_for(&skey);
                                let next_b = match op {
                                    BrightnessOp::Set(v) => v.min(100),
                                    BrightnessOp::Delta(d) => (cur as i16 + d).clamp(0, 100) as u8,
                                };
                                ws.set_brightness_for(&skey, next_b);
                                ws.save();
                                sleep_nat.borrow_mut().insert(skey.clone(), next_b == 0);
                                nh_nat.set_current_brightness(&skey, next_b);
                                let _ = hw_nat.borrow().set_brightness(&dev.dev_path, next_b);
                            }
                        }
                        HostCommand::ToggleSleep => {
                            if let Some(ref dev) = target_dev {
                                let skey = dev.stable_key();
                                let ws = WindowState::load();
                                let was_sleeping =
                                    sleep_nat.borrow().get(&skey).copied().unwrap_or(false);
                                if was_sleeping {
                                    sleep_nat.borrow_mut().insert(skey.clone(), false);
                                    let restored = ws.brightness_for(&skey).max(10);
                                    nh_nat.set_current_brightness(&skey, restored);
                                    let _ = hw_nat.borrow().set_brightness(&dev.dev_path, restored);
                                } else {
                                    sleep_nat.borrow_mut().insert(skey.clone(), true);
                                    nh_nat.set_current_brightness(&skey, 0);
                                    let _ = hw_nat.borrow().set_brightness(&dev.dev_path, 0);
                                }
                            }
                        }
                        HostCommand::OpenAppPicker => {
                            // Live-rescan System, Flatpak, Snap, and AppImage .desktop entries so newly installed apps show immediately
                            let fresh_apps = PluginManager::collect_installed_apps();
                            if !fresh_apps.is_empty() {
                                insp_nat.populate_installed_apps(&fresh_apps);
                                pm_nat.borrow_mut().installed_apps = fresh_apps.clone();
                            }
                            let apps = if fresh_apps.is_empty() {
                                insp_nat.installed_apps.borrow().clone()
                            } else {
                                fresh_apps
                            };
                            let prof_p = prof_nat.clone();
                            let nh_p = nh_nat.clone();
                            let can_p = canvas_nat.clone();
                            let insp_p = insp_nat.clone();
                            let hw_p = hw_nat.clone();
                            let devs_p = devs_nat.clone();
                            let act_p = act_nat.clone();
                            let persist_p = persist_nat.clone();
                            let dkey_p = device_key.clone();
                            let slot_p = slot.clone();
                            let tgt_p = target;

                            super::inspector::show_app_picker_dialog(&win_nat, &apps, move |picked| {
                                let resolved_icon =
                                    super::canvas::resolve_icon_path(&picked.icon, 128);
                                let mut updates: HashMap<String, serde_json::Value> = HashMap::new();
                                updates.insert("desktop_id".into(), serde_json::json!(picked.desktop_id));
                                updates.insert("desktop_path".into(), serde_json::json!(picked.desktop_path));
                                updates.insert("exec".into(), serde_json::json!(picked.exec));
                                updates.insert("app_name".into(), serde_json::json!(picked.name));
                                updates.insert("icon_name".into(), serde_json::json!(picked.icon));
                                if let Some(ref ip) = resolved_icon {
                                    updates.insert("icon_path".into(), serde_json::json!(ip));
                                }
                                {
                                    let mut prof = prof_p.borrow_mut();
                                    if let Some(page) = prof.current_page_mut() {
                                        let action = get_target_mut(page, tgt_p);
                                        action.parameters.insert("desktop_id".into(), picked.desktop_id.clone());
                                        action.parameters.insert("desktop_path".into(), picked.desktop_path.clone());
                                        action.parameters.insert("exec".into(), picked.exec.clone());
                                        action.parameters.insert("app_name".into(), picked.name.clone());
                                        action.parameters.insert("icon_name".into(), picked.icon.clone());
                                        if let Some(ref ip) = resolved_icon {
                                            action.parameters.insert("icon_path".into(), ip.clone());
                                        }
                                        for (k, v) in &updates {
                                            action.raw_settings.insert(k.clone(), v.clone());
                                        }
                                        action.appearance.title = picked.name.clone();
                                        action.appearance.show_title = true;
                                        action.appearance.icon_path = resolved_icon.clone();
                                        for st in action.states.iter_mut() {
                                            st.title = picked.name.clone();
                                            st.show_title = true;
                                            st.icon_path = resolved_icon.clone();
                                        }
                                    }
                                }
                                let _ = nh_p.update_slot_settings(&dkey_p, &slot_p, updates);
                                persist_p();
                                let prof = prof_p.borrow();
                                can_p.render_profile_page(&prof);
                                insp_p.set_key_action(get_target_ref(&prof, tgt_p));
                                drop(prof);
                                Self::sync_hardware(&devs_p, *act_p.borrow(), &hw_p, &prof_p.borrow());
                            });
                        }
                        HostCommand::OpenFilePicker { directories_only } => {
                            let dialog = gtk::FileDialog::builder()
                                .title(if directories_only { "Choose Folder" } else { "Choose File" })
                                .modal(true)
                                .build();
                            let prof_f = prof_nat.clone();
                            let nh_f = nh_nat.clone();
                            let can_f = canvas_nat.clone();
                            let insp_f = insp_nat.clone();
                            let hw_f = hw_nat.clone();
                            let devs_f = devs_nat.clone();
                            let act_f = act_nat.clone();
                            let persist_f = persist_nat.clone();
                            let dkey_f = device_key.clone();
                            let slot_f = slot.clone();
                            let tgt_f = target;
                            let apply_chosen = move |res: Result<gio::File, glib::Error>| {
                                if let Ok(file) = res {
                                    if let Some(path) = file.path() {
                                        let p_str = path.to_string_lossy().to_string();
                                        let label = path
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or(&p_str)
                                            .to_string();
                                        let mut updates = HashMap::new();
                                        updates.insert("file_path".into(), serde_json::json!(p_str));
                                        {
                                            let mut prof = prof_f.borrow_mut();
                                            if let Some(page) = prof.current_page_mut() {
                                                let action = get_target_mut(page, tgt_f);
                                                action.parameters.insert("file_path".into(), p_str.clone());
                                                action.raw_settings.insert("file_path".into(), serde_json::json!(p_str));
                                                action.appearance.title = label.clone();
                                                action.appearance.show_title = true;
                                                for st in action.states.iter_mut() {
                                                    st.title = label.clone();
                                                    st.show_title = true;
                                                }
                                            }
                                        }
                                        let _ = nh_f.update_slot_settings(&dkey_f, &slot_f, updates);
                                        persist_f();
                                        let prof = prof_f.borrow();
                                        can_f.render_profile_page(&prof);
                                        insp_f.set_key_action(get_target_ref(&prof, tgt_f));
                                        drop(prof);
                                        Self::sync_hardware(&devs_f, *act_f.borrow(), &hw_f, &prof_f.borrow());
                                    }
                                }
                            };
                            if directories_only {
                                dialog.select_folder(Some(&win_nat), gio::Cancellable::NONE, apply_chosen);
                            } else {
                                dialog.open(Some(&win_nat), gio::Cancellable::NONE, apply_chosen);
                            }
                        }
                        HostCommand::ExecuteMultiAction { steps } => {
                            let tx_bg = multi_tx.clone();
                            let dkey_bg = device_key.clone();
                            std::thread::spawn(move || {
                                for step in steps {
                                    match step {
                                        MultiActionStep::Delay { ms } => {
                                            std::thread::sleep(std::time::Duration::from_millis(ms));
                                        }
                                        MultiActionStep::RunCommand { command } => {
                                            crate::plugins::ballista_core::run_command::execute_shell_command(&command, "", false);
                                        }
                                        MultiActionStep::Hotkey { keys } => {
                                            crate::plugins::ballista_core::hotkey::fire_hotkey(&keys);
                                        }
                                        MultiActionStep::TypeText { text, press_enter } => {
                                            crate::plugins::ballista_core::hotkey::fire_type_text(&text, press_enter);
                                        }
                                        MultiActionStep::LaunchApp { desktop_id, .. } => {
                                            crate::plugins::ballista_core::launcher::launch_desktop_app(&desktop_id, "");
                                        }
                                        MultiActionStep::OpenUrl { url } => {
                                            crate::plugins::ballista_core::launcher::open_url_or_path(&url);
                                        }
                                        MultiActionStep::Media { action } => {
                                            crate::plugins::ballista_core::media::trigger_mpris_action(&action);
                                        }
                                        ui_step @ (MultiActionStep::SwitchPage { .. }
                                        | MultiActionStep::Brightness { .. }
                                        | MultiActionStep::TriggerKey { .. }) => {
                                            let _ = tx_bg.send_blocking((dkey_bg.clone(), ui_step));
                                        }
                                    }
                                }
                            });
                        }
                    }
                }

                if need_persist {
                    persist_target();
                }

                let is_deck_sleeping = sleep_nat
                    .borrow()
                    .get(&device_key)
                    .copied()
                    .unwrap_or(false);

                if need_sync_page {
                    if let Some(dev_prof) = read_dev_profile() {
                        let p_names: Vec<String> = profs_nat
                            .borrow()
                            .iter()
                            .filter(|p| p.device_kind == target_kind)
                            .map(|p| p.name.clone())
                            .collect();
                        let cur_b = WindowState::load().brightness_for(&device_key);
                        sync_device_runtime(
                            &device_key,
                            &dev_prof,
                            p_names,
                            cur_b,
                            &pm_nat.borrow(),
                            &nh_nat,
                        );
                        if is_gui_deck {
                            canvas_nat.render_profile_page(&dev_prof);
                            let cur_target = *insp_nat.current_target.borrow();
                            insp_nat.set_key_action(get_target_ref(&dev_prof, cur_target));
                        }
                        if need_full_hw_sync && !is_deck_sleeping && !sys_sleep_nat.get() {
                            if let Some(ref dev) = target_dev {
                                hw_nat.borrow().sync_hardware_page(
                                    &dev.dev_path,
                                    &device_key,
                                    dev.kind,
                                    &dev_prof,
                                );
                            }
                        }
                    }
                    return;
                }

                if is_gui_deck && need_full_render {
                    let prof = prof_nat.borrow();
                    canvas_nat.render_profile_page(&prof);
                    if target == *insp_nat.current_target.borrow() {
                        if let Some(act) = get_target_ref(&prof, target) {
                            insp_nat.update_preview_only(act);
                        }
                    }
                }

                if is_gui_deck && need_refresh_rows {
                    let cur_t = *insp_nat.current_target.borrow();
                    if format_target_slot(cur_t) == slot {
                        if let Some(rows) = nh_nat.config_rows(&device_key, &slot) {
                            insp_nat.render_live_config_rows(&slot, &rows);
                        }
                        let prof = prof_nat.borrow();
                        if let Some(act) = get_target_ref(&prof, cur_t) {
                            if act.action_id == "ballista.multi_action" {
                                insp_nat.render_multi_action_editor(&slot, act);
                            }
                        }
                    }
                }

                if need_single_redraw {
                    if is_gui_deck {
                        canvas_nat.redraw_native_slot(&slot);
                        insp_nat.redraw_native_preview(&slot);
                    }
                    if !is_deck_sleeping && !sys_sleep_nat.get() {
                        if let (Some(dev), Some(dev_prof)) = (target_dev.as_ref(), read_dev_profile()) {
                            hw_nat.borrow().sync_single_slot(
                                &dev.dev_path,
                                &device_key,
                                dev.kind,
                                &dev_prof,
                                &slot,
                            );
                        }
                    }
                } else if need_full_hw_sync && !is_deck_sleeping && !sys_sleep_nat.get() {
                    if let (Some(dev), Some(dev_prof)) = (target_dev.as_ref(), read_dev_profile()) {
                        hw_nat.borrow().sync_hardware_page(
                            &dev.dev_path,
                            &device_key,
                            dev.kind,
                            &dev_prof,
                        );
                    }
                }
            })
        };

        // Wire Folder Breadcrumb Back Button
        {
            let disp_back = dispatch_native.clone();
            let devs_back = connected_devices.clone();
            let act_back = active_device_idx.clone();
            let prof_back = profile.clone();
            canvas.set_folder_back_callback(move || {
                let dkey = active_device_key_for(&devs_back, &act_back, prof_back.borrow().device_kind);
                disp_back(dkey, "folder".into(), vec![HostCommand::ExitFolder], None);
            });
        }

        // Wire Multi-Action Main-Thread Step Receiver (SwitchPage, Brightness, TriggerKey)
        {
            let disp_multi = dispatch_native.clone();
            let prof_multi = profile.clone();
            let profs_multi = profiles.clone();
            let act_by_dev_multi = active_by_device.clone();
            let devs_multi = connected_devices.clone();
            let act_multi = active_device_idx.clone();
            let pm_multi = plugin_manager.clone();
            let nh_multi = native_host.clone();
            let can_multi = canvas.clone();
            glib::spawn_future_local(async move {
                while let Ok((dkey, step)) = multi_ui_rx.recv().await {
                    let is_gui = dkey
                        == active_device_key_for(
                            &devs_multi,
                            &act_multi,
                            prof_multi.borrow().device_kind,
                        );
                    match step {
                        MultiActionStep::SwitchPage { target } => {
                            let nav = match target.as_str() {
                                "next" => PageNav::Next,
                                "prev" => PageNav::Prev,
                                num => PageNav::Index(num.parse::<usize>().unwrap_or(1).saturating_sub(1)),
                            };
                            disp_multi(dkey, "multi".into(), vec![HostCommand::SwitchPage(nav)], None);
                        }
                        MultiActionStep::Brightness { percent } => {
                            disp_multi(
                                dkey,
                                "multi".into(),
                                vec![HostCommand::SetBrightness(BrightnessOp::Set(percent))],
                                None,
                            );
                        }
                        MultiActionStep::TriggerKey { key_index } => {
                            let slot = format!("key_{key_index}");
                            if is_gui {
                                can_multi.pulse_key_press(key_index, 130);
                            }
                            if nh_multi.has_slot(&dkey, &slot) {
                                let (cmds, updated) = nh_multi.handle_key_event(&dkey, &slot, true);
                                disp_multi(dkey.clone(), slot.clone(), cmds, updated);
                                let nh_up = nh_multi.clone();
                                let disp_up = disp_multi.clone();
                                let dkey_up = dkey.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(100),
                                    move || {
                                        let (cmds2, upd2) = nh_up.handle_key_event(&dkey_up, &slot, false);
                                        disp_up(dkey_up, slot, cmds2, upd2);
                                    },
                                );
                            } else {
                                let dev_kind = devs_multi
                                    .borrow()
                                    .iter()
                                    .find(|d| d.stable_key() == dkey)
                                    .map(|d| d.kind)
                                    .unwrap_or_else(|| prof_multi.borrow().device_kind);
                                let is_bridge = if is_gui {
                                    prof_multi
                                        .borrow()
                                        .current_page()
                                        .and_then(|p| p.keys.get(&key_index))
                                        .map(|a| !a.plugin_id.is_empty() && a.plugin_id != "system")
                                        .unwrap_or(false)
                                } else {
                                    let list = profs_multi.borrow();
                                    find_profile_idx_for_device(
                                        &list,
                                        &act_by_dev_multi.borrow(),
                                        &dkey,
                                        dev_kind,
                                    )
                                    .and_then(|i| list[i].current_page())
                                    .and_then(|p| p.keys.get(&key_index))
                                    .map(|a| !a.plugin_id.is_empty() && a.plugin_id != "system")
                                    .unwrap_or(false)
                                };
                                if is_bridge {
                                    let scoped = scoped_bridge_slot(&dkey, &slot);
                                    pm_multi.borrow().bridge.send_key_event(&scoped, true);
                                    let pm_up = pm_multi.clone();
                                    glib::timeout_add_local_once(
                                        std::time::Duration::from_millis(110),
                                        move || {
                                            pm_up.borrow().bridge.send_key_event(&scoped, false);
                                        },
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            });
        }

        // Native Plugin Periodic Tick Loop (250ms cadence across ALL connected decks simultaneously!)
        {
            let nh_tick = native_host.clone();
            let disp_tick = dispatch_native.clone();
            let sys_sleep_tick = system_sleeping.clone();
            let locked_tick = screen_locked.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
                if sys_sleep_tick.get() || locked_tick.get() {
                    return glib::ControlFlow::Continue;
                }
                for (dev_key, slot, cmds) in nh_tick.tick_due() {
                    let upd = nh_tick.get_slot_settings(&dev_key, &slot);
                    disp_tick(dev_key, slot, cmds, upd);
                }
                glib::ControlFlow::Continue
            });
        }

        // Wire Multi-Action Step Sequence Editor saves
        {
            let prof_ms = profile.clone();
            let nh_ms = native_host.clone();
            let can_ms = canvas.clone();
            let insp_ms = inspector.clone();
            let hw_ms = hardware_manager.clone();
            let devs_ms = connected_devices.clone();
            let act_ms = active_device_idx.clone();
            let persist_ms = persist_profiles.clone();
            inspector.on_multi_steps_changed.borrow_mut().replace(std::boxed::Box::new(
                move |slot, state_key, steps| {
                    let target = parse_slot_target(&slot);
                    let dkey = active_device_key_for(&devs_ms, &act_ms, prof_ms.borrow().device_kind);
                    let steps_val = serde_json::to_value(&steps).unwrap_or(serde_json::json!([]));
                    let mut updates = HashMap::new();
                    updates.insert(state_key.clone(), steps_val.clone());
                    {
                        let mut prof = prof_ms.borrow_mut();
                        if let Some(page) = prof.current_page_mut() {
                            if let Some(act) = match target {
                                InspectorTarget::Key(k) => page.keys.get_mut(&k),
                                InspectorTarget::Dial(d) => page.dial_actions.get_mut(&d),
                                InspectorTarget::Touchscreen => page.touchscreen_action.as_mut(),
                            } {
                                act.raw_settings.insert(state_key, steps_val);
                            }
                        }
                    }
                    let _ = nh_ms.update_slot_settings(&dkey, &slot, updates);
                    persist_ms();
                    can_ms.redraw_native_slot(&slot);
                    insp_ms.redraw_native_preview(&slot);
                    let prof = prof_ms.borrow();
                    if let Some(act) = get_target_ref(&prof, target) {
                        insp_ms.render_multi_action_editor(&slot, act);
                    }
                    let devs = devs_ms.borrow();
                    if let Some(dev) = devs.get(*act_ms.borrow()) {
                        hw_ms
                            .borrow()
                            .sync_single_slot(&dev.dev_path, &dkey, dev.kind, &prof, &slot);
                    }
                },
            ));
        }

        let pm_row = plugin_manager.clone();
        let nh_row = native_host.clone();
        let disp_row = dispatch_native.clone();
        let devs_row = connected_devices.clone();
        let act_row = active_device_idx.clone();
        let prof_row = profile.clone();
        inspector.on_config_row_changed.borrow_mut().replace(std::boxed::Box::new(move |slot, index, selected, active, text, color, value| {
            let dkey = active_device_key_for(&devs_row, &act_row, prof_row.borrow().device_kind);
            if nh_row.has_slot(&dkey, &slot) {
                let (cmds, updated) = nh_row.on_config_changed(
                    &dkey,
                    &slot,
                    index,
                    ConfigChange {
                        selected,
                        active,
                        text,
                        value,
                        color,
                    },
                );
                disp_row(dkey, slot, cmds, updated);
            } else {
                pm_row.borrow().bridge.set_action_config_row(
                    &scoped_bridge_slot(&dkey, &slot),
                    index,
                    selected,
                    active,
                    text,
                    color,
                    value,
                );
            }
        }));

        let pm_btn = plugin_manager.clone();
        let nh_btn = native_host.clone();
        let disp_btn = dispatch_native.clone();
        let devs_btn = connected_devices.clone();
        let act_btn = active_device_idx.clone();
        let prof_btn = profile.clone();
        inspector.on_config_button_clicked.borrow_mut().replace(std::boxed::Box::new(move |slot, index| {
            let dkey = active_device_key_for(&devs_btn, &act_btn, prof_btn.borrow().device_kind);
            if nh_btn.has_slot(&dkey, &slot) {
                let (cmds, updated) = nh_btn.on_button_clicked(&dkey, &slot, index);
                disp_btn(dkey, slot, cmds, updated);
            } else {
                pm_btn
                    .borrow()
                    .bridge
                    .trigger_action_config_button(&scoped_bridge_slot(&dkey, &slot), index);
            }
        }));

        // Unified Action Assignment Helper (used by Sidebar click, post-install Sidebar, and Drag & Drop)
        let assign_action: Rc<dyn Fn(InspectorTarget, super::sidebar::SelectedAction, bool)> = {
            let prof_assign = profile.clone();
            let canvas_assign = canvas.clone();
            let insp_assign = inspector.clone();
            let hw_assign = hardware_manager.clone();
            let devs_assign = connected_devices.clone();
            let act_assign = active_device_idx.clone();
            let pm_assign = plugin_manager.clone();
            let nh_assign = native_host.clone();
            let disp_assign = dispatch_native.clone();
            let persist_assign = persist_profiles.clone();

            Rc::new(move |mut target, sel, mut select_in_ui| {
                // Enforce Action Button Type Exclusivity (`TargetCaps`):
                // - Direct drag-and-drop onto an incompatible slot is refused cleanly.
                // - Right-click "Add" auto-targets a compatible slot on the active deck when available.
                let is_native = nh_assign.is_native_plugin(&sel.plugin_id);
                let caps = if is_native {
                    nh_assign.action_caps(&sel.plugin_id, &sel.action_id)
                } else {
                    sel.caps
                };
                let target_surface = match target {
                    InspectorTarget::Key(_) => crate::native_sdk::SlotSurface::Key,
                    InspectorTarget::Dial(_) => crate::native_sdk::SlotSurface::Dial,
                    InspectorTarget::Touchscreen => crate::native_sdk::SlotSurface::Touchscreen,
                };
                if !caps.supports(target_surface) {
                    if select_in_ui {
                        // Direct drag-and-drop onto an incompatible surface: hard reject!
                        return;
                    }
                    // Sidebar right-click "Add": auto-route to the first empty compatible slot if the device supports it
                    let dev_kind = devs_assign
                        .borrow()
                        .get(*act_assign.borrow())
                        .map(|d| d.kind)
                        .unwrap_or(DeviceKind::StreamDeckPlus);
                    let prof = prof_assign.borrow();
                    let redirected = if caps.dials && dev_kind.has_dials() {
                        let d_idx = (0..dev_kind.dial_count())
                            .find(|d| {
                                prof.current_page()
                                    .map(|pg| !pg.dial_actions.contains_key(d))
                                    .unwrap_or(true)
                            })
                            .unwrap_or(0);
                        Some(InspectorTarget::Dial(d_idx))
                    } else if caps.keys && dev_kind.total_keys() > 0 {
                        let k_idx = (0..dev_kind.total_keys())
                            .find(|k| {
                                prof.current_page()
                                    .map(|pg| !pg.keys.contains_key(k))
                                    .unwrap_or(true)
                            })
                            .unwrap_or(0);
                        Some(InspectorTarget::Key(k_idx))
                    } else {
                        None
                    };
                    drop(prof);
                    if let Some(next_target) = redirected {
                        target = next_target;
                        select_in_ui = true;
                    } else {
                        tracing::info!(
                            "Action '{}' requires {:?} which is unavailable on {:?}",
                            sel.action_name,
                            caps,
                            dev_kind
                        );
                        return;
                    }
                }

                let dkey = active_device_key_for(&devs_assign, &act_assign, prof_assign.borrow().device_kind);
                let slot = format_target_slot(target);
                let scoped_slot = scoped_bridge_slot(&dkey, &slot);
                nh_assign.destroy_slot(&dkey, &slot);
                {
                    let pm = pm_assign.borrow();
                    pm.bridge.destroy_action(&scoped_slot);
                    pm.clear_live_config_rows(&scoped_slot);
                }

                let mut action = KeyAction::new(&sel.plugin_id, &sel.plugin_name, &sel.action_id, &sel.action_name);

                if is_native {
                    if let Some(desc) = nh_assign.find_descriptor(&sel.plugin_id, &sel.action_id) {
                        action.appearance.title = desc.default_title.clone();
                        action.appearance.show_title = !desc.default_title.is_empty();
                        if let Some(st) = action.states.first_mut() {
                            st.title = desc.default_title;
                            st.show_title = action.appearance.show_title;
                        }
                    }
                } else {
                    let action_stem = sel.action_id.split("::").last().unwrap_or(&sel.action_id);
                    let states = pm_assign.borrow().resolve_action_states(&sel.plugin_id, action_stem);
                    if !states.is_empty() {
                        action.appearance = states[0].clone();
                        action.states = states;
                        action.current_state = 0;
                    } else {
                        action.appearance.title = sel.action_name.clone();
                    }
                    if !sel.icon_name.is_empty() {
                        action.parameters.insert("icon_name".into(), sel.icon_name.clone());
                    }
                }

                let merged_settings = action.merged_settings_json();
                {
                    let mut prof = prof_assign.borrow_mut();
                    if let Some(page) = prof.current_page_mut() {
                        match target {
                            InspectorTarget::Key(k) => {
                                page.keys.insert(k, action);
                            }
                            InspectorTarget::Dial(d) => {
                                page.dial_actions.insert(d, action);
                            }
                            InspectorTarget::Touchscreen => {
                                page.touchscreen_action = Some(action);
                            }
                        }
                    }
                }

                if is_native {
                    let emitted = {
                        let prof = prof_assign.borrow();
                        prof.current_page()
                            .map(|pg| nh_assign.sync_page(&dkey, pg))
                            .unwrap_or_default()
                    };
                    for (s, cmds) in emitted {
                        let upd = nh_assign.get_slot_settings(&dkey, &s);
                        disp_assign(dkey.clone(), s, cmds, upd);
                    }
                } else if !sel.plugin_id.is_empty() && sel.plugin_id != "system" {
                    pm_assign.borrow().bridge.create_action(
                        &scoped_slot,
                        &sel.plugin_id,
                        &sel.action_id,
                        merged_settings,
                    );
                }

                if select_in_ui {
                    insp_assign.set_target(target);
                    match target {
                        InspectorTarget::Key(k) => canvas_assign.select_key(k),
                        InspectorTarget::Dial(d) => canvas_assign.select_dial_and_touch(d),
                        InspectorTarget::Touchscreen => canvas_assign.select_touchscreen_panoramic(),
                    }
                }

                let prof = prof_assign.borrow();
                canvas_assign.render_profile_page(&prof);
                insp_assign.set_key_action(get_target_ref(&prof, target));
                insp_assign.highlight_action_settings();
                drop(prof);
                persist_assign();
                Self::sync_hardware(&devs_assign, *act_assign.borrow(), &hw_assign, &prof_assign.borrow());
            })
        };

        // Unified Sidebar Refresh Helper (keeps Inspector PluginManager in sync & preserves full bridge action wiring)
        let refresh_sidebar: Rc<dyn Fn()> = {
            let sb_ref = sidebar.clone();
            let pm_ref = plugin_manager.clone();
            let nh_ref = native_host.clone();
            let insp_ref = inspector.clone();
            let assign_ref = assign_action.clone();
            Rc::new(move || {
                let pm = pm_ref.borrow();
                *insp_ref.plugin_manager.borrow_mut() = Some(pm.clone());
                let insp_click = insp_ref.clone();
                let assign_click = assign_ref.clone();
                sb_ref.populate_from_plugins(&pm, &nh_ref, move |sel| {
                    let target = *insp_click.current_target.borrow();
                    assign_click(target, sel, false);
                });
            })
        };

        // Wire StreamController Python Bridge Callbacks (plugin_initialized, plugin_settings_rows, set_media, set_label, set_state, set_settings, etc.)
        let (bridge_sender, bridge_receiver) = async_channel::unbounded::<crate::core::plugin_bridge::BridgeEvent>();
        plugin_manager.borrow().bridge.set_event_callback(move |event| {
            let _ = bridge_sender.send_blocking(event);
        });

        let prof_bridge = profile.clone();
        let profs_bridge = profiles.clone();
        let act_by_dev_bridge = active_by_device.clone();
        let canvas_bridge = canvas.clone();
        let insp_bridge = inspector.clone();
        let hw_bridge = hardware_manager.clone();
        let devs_bridge = connected_devices.clone();
        let act_bridge = active_device_idx.clone();
        let pm_bridge = plugin_manager.clone();
        let active_ps_bridge = active_plugin_settings.clone();
        let refresh_sidebar_bridge = refresh_sidebar.clone();
        let persist_bridge = persist_profiles.clone();

        glib::spawn_future_local(async move {
            while let Ok(event) = bridge_receiver.recv().await {
                let apply_bridge_slot_update =
                    |raw_slot: &str, mutator: &mut dyn FnMut(&mut KeyAction) -> bool, push_hw: bool| {
                        let active_gui_key = active_device_key_for(
                            &devs_bridge,
                            &act_bridge,
                            prof_bridge.borrow().device_kind,
                        );
                        let (opt_dkey, base_slot) = split_bridge_slot(raw_slot);
                        let dkey = opt_dkey
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| active_gui_key.clone());
                        let is_gui = dkey == active_gui_key;
                        let target = parse_slot_target(base_slot);
                        let target_dev = devs_bridge
                            .borrow()
                            .iter()
                            .find(|d| d.stable_key() == dkey)
                            .cloned();
                        let target_kind = target_dev
                            .as_ref()
                            .map(|d| d.kind)
                            .unwrap_or_else(|| prof_bridge.borrow().device_kind);

                        let mut updated = false;
                        let mut updated_prof: Option<Profile> = None;

                        if is_gui {
                            {
                                let mut prof = prof_bridge.borrow_mut();
                                if let Some(page) = prof.current_page_mut() {
                                    let action = get_target_mut(page, target);
                                    updated = mutator(action);
                                }
                                if updated {
                                    updated_prof = Some(prof.clone());
                                }
                            }
                            if updated {
                                persist_bridge();
                                if let Some(ref prof) = updated_prof {
                                    canvas_bridge.render_profile_page(prof);
                                    if target == *insp_bridge.current_target.borrow() {
                                        if let Some(act) = get_target_ref(prof, target) {
                                            insp_bridge.update_preview_only(act);
                                        }
                                    }
                                }
                            }
                        } else {
                            {
                                let mut list = profs_bridge.borrow_mut();
                                if let Some(p_idx) = find_profile_idx_for_device(
                                    &list,
                                    &act_by_dev_bridge.borrow(),
                                    &dkey,
                                    target_kind,
                                ) {
                                    if let Some(page) = list[p_idx].current_page_mut() {
                                        let action = get_target_mut(page, target);
                                        updated = mutator(action);
                                    }
                                    if updated {
                                        updated_prof = Some(list[p_idx].clone());
                                    }
                                }
                            }
                            if updated {
                                ProfileStore::save_profiles(
                                    &profs_bridge.borrow(),
                                    &act_by_dev_bridge.borrow(),
                                );
                            }
                        }

                        if updated && push_hw {
                            if let (Some(dev), Some(prof)) = (target_dev, updated_prof) {
                                hw_bridge.borrow().sync_hardware_page(
                                    &dev.dev_path,
                                    &dkey,
                                    dev.kind,
                                    &prof,
                                );
                            }
                        }
                    };

                match event {
                    crate::core::plugin_bridge::BridgeEvent::PluginInitialized {
                        plugin_id,
                        plugin_name,
                        has_settings,
                        actions,
                        ..
                    } => {
                        tracing::info!(
                            "Bridge initialized plugin '{}' ({}) with {} actions (has_settings={})",
                            plugin_name,
                            plugin_id,
                            actions.len(),
                            has_settings
                        );
                        {
                            let mut pm = pm_bridge.borrow_mut();
                            pm.apply_bridge_plugin_metadata(&plugin_id, &plugin_name, has_settings, &actions);
                        }
                        refresh_sidebar_bridge();
                    }
                    crate::core::plugin_bridge::BridgeEvent::PluginSettingsRows { plugin_id, rows } => {
                        pm_bridge.borrow().set_live_plugin_settings_rows(&plugin_id, rows.clone());
                        if let Some((ref open_pid, ref cb)) = *active_ps_bridge.borrow() {
                            if open_pid == &plugin_id {
                                cb(&rows);
                            }
                        }
                    }
                    crate::core::plugin_bridge::BridgeEvent::SetMedia { slot, media_path, size, .. } => {
                        apply_bridge_slot_update(
                            &slot,
                            &mut |action| {
                                let new_icon = if media_path.is_empty() {
                                    None
                                } else {
                                    Some(media_path.clone())
                                };
                                action.appearance.icon_path = new_icon.clone();
                                if let Some(sz) = size {
                                    let pct = (sz * 100.0).round().clamp(20.0, 100.0) as u32;
                                    action.appearance.icon_scale = pct;
                                }
                                let cur_st = action.current_state;
                                if cur_st < action.states.len() {
                                    action.states[cur_st].icon_path = new_icon;
                                    if let Some(sz) = size {
                                        let pct = (sz * 100.0).round().clamp(20.0, 100.0) as u32;
                                        action.states[cur_st].icon_scale = pct;
                                    }
                                }
                                true
                            },
                            true,
                        );
                    }
                    crate::core::plugin_bridge::BridgeEvent::SetLabel {
                        slot,
                        text,
                        position,
                        color,
                        font_size,
                        font_family,
                    } => {
                        let parsed_color = color.as_ref().and_then(parse_bridge_color);
                        let pos = position.as_deref().unwrap_or("bottom").to_string();
                        let label_val = if text.is_empty() { None } else { Some(text.clone()) };
                        apply_bridge_slot_update(
                            &slot,
                            &mut |action| {
                                let apply_label = |app: &mut KeyAppearance| {
                                    if app.custom_title_override {
                                        return;
                                    }
                                    match pos.as_str() {
                                        "top" => app.top_label = label_val.clone(),
                                        "center" => app.center_label = label_val.clone(),
                                        _ => app.bottom_label = label_val.clone(),
                                    }
                                    let summary = app.combined_display_summary();
                                    if !summary.is_empty() {
                                        app.title = summary;
                                    } else if !text.is_empty() {
                                        app.title = text.clone();
                                    } else if !app.has_plugin_labels() {
                                        app.title.clear();
                                    }
                                    if let Some(ref c) = parsed_color {
                                        app.text_color = c.clone();
                                    }
                                    if let Some(fs) = font_size {
                                        if fs > 0 {
                                            app.font_size = fs;
                                        }
                                    }
                                    if let Some(ref ff) = font_family {
                                        if !ff.is_empty() {
                                            app.font_family = ff.clone();
                                        }
                                    }
                                };
                                apply_label(&mut action.appearance);
                                let cur_st = action.current_state;
                                if cur_st < action.states.len() {
                                    apply_label(&mut action.states[cur_st]);
                                }
                                true
                            },
                            true,
                        );
                    }
                    crate::core::plugin_bridge::BridgeEvent::SetState { slot, state } => {
                        apply_bridge_slot_update(
                            &slot,
                            &mut |action| {
                                if state < action.states.len() {
                                    action.set_active_state(state);
                                    true
                                } else {
                                    false
                                }
                            },
                            true,
                        );
                    }
                    crate::core::plugin_bridge::BridgeEvent::SetSettings { slot, settings } => {
                        apply_bridge_slot_update(
                            &slot,
                            &mut |action| {
                                for (k, v) in &settings {
                                    let val_str = match v {
                                        serde_json::Value::String(s) => s.clone(),
                                        serde_json::Value::Number(n) => n.to_string(),
                                        serde_json::Value::Bool(b) => b.to_string(),
                                        serde_json::Value::Null => String::new(),
                                        other => other.to_string(),
                                    };
                                    action.raw_settings.insert(k.clone(), v.clone());
                                    action.parameters.insert(k.clone(), val_str);
                                }
                                true
                            },
                            false,
                        );
                    }
                    crate::core::plugin_bridge::BridgeEvent::ActionConfigRows { slot, rows } => {
                        pm_bridge.borrow().set_live_config_rows(&slot, rows.clone());
                        let active_gui_key = active_device_key_for(
                            &devs_bridge,
                            &act_bridge,
                            prof_bridge.borrow().device_kind,
                        );
                        let (opt_dkey, base_slot) = split_bridge_slot(&slot);
                        let matches_gui_dev = opt_dkey.map(|k| k == active_gui_key).unwrap_or(true);
                        let target = *insp_bridge.current_target.borrow();
                        if matches_gui_dev && format_target_slot(target) == base_slot {
                            insp_bridge.render_live_config_rows(base_slot, &rows);
                        }
                    }
                    crate::core::plugin_bridge::BridgeEvent::SetBackgroundColor { slot, color } => {
                        let hex_color = parse_bridge_color(&color);
                        apply_bridge_slot_update(
                            &slot,
                            &mut |action| {
                                action.appearance.bg_color = hex_color.clone();
                                let cur_st = action.current_state;
                                if cur_st < action.states.len() {
                                    action.states[cur_st].bg_color = hex_color.clone();
                                }
                                true
                            },
                            true,
                        );
                    }
                    crate::core::plugin_bridge::BridgeEvent::ShowError { slot, duration } => {
                        tracing::warn!("Plugin reported action error on slot {} (duration={}s)", slot, duration);
                    }
                    crate::core::plugin_bridge::BridgeEvent::HideError { slot } => {
                        tracing::debug!("Plugin cleared action error on slot {}", slot);
                    }
                    crate::core::plugin_bridge::BridgeEvent::Notification { title, body, .. } => {
                        tracing::info!("Plugin notification: {} — {}", title, body);
                    }
                    _ => {}
                }
            }
        });
        // Initial Canvas Render (Phase-1 parallel hardware boot already lit all connected decks!)
        canvas.update_scale(state.work_paned_position);
        canvas.render_profile_page(&profile.borrow());
        canvas.select_key(0);
        *inspector.current_key_idx.borrow_mut() = Some(0);
        let prof_init = profile.borrow();
        inspector.set_key_action(prof_init.current_page().and_then(|p| p.keys.get(&0)));
        drop(prof_init);

        // Wire canvas button click handlers
        Self::bind_canvas_events(
            &canvas,
            &inspector,
            &profile,
            &plugin_manager,
            &native_host,
            &dispatch_native,
            &connected_devices,
            &active_device_idx,
            &hardware_manager,
        );

        // ===================== Multi-Device Hardware Event Channel Setup =====================
        // Receives `(device_index, HardwareEvent)` from ALL connected Stream Decks simultaneously!
        let (sender, receiver) = async_channel::unbounded::<(usize, HardwareEvent)>();
        let rescan_cb_cell: Rc<RefCell<Option<Rc<dyn Fn(&str, bool) -> usize>>>> =
            Rc::new(RefCell::new(None));
        let last_auto_recovery: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));

        let canvas_hw = canvas.clone();
        let inspector_hw = inspector.clone();
        let profile_hw = profile.clone();
        let profiles_hw = profiles.clone();
        let active_by_dev_hw = active_by_device.clone();
        let pm_hw = plugin_manager.clone();
        let nh_hw = native_host.clone();
        let disp_hw = dispatch_native.clone();
        let sleep_hw = deck_sleeping.clone();
        let sys_sleep_hw = system_sleeping.clone();
        let locked_hw = screen_locked.clone();
        let rescan_hw_cell = rescan_cb_cell.clone();
        let last_rec_hw = last_auto_recovery.clone();
        let devs_hw = connected_devices.clone();
        let act_hw = active_device_idx.clone();
        let hwm_hw = hardware_manager.clone();
        let persist_hw = persist_profiles.clone();

        glib::spawn_future_local(async move {
            while let Ok((dev_idx, event)) = receiver.recv().await {
                if let HardwareEvent::DeviceLost { dev_path } = event {
                    if sys_sleep_hw.get() {
                        continue;
                    }
                    let now = std::time::Instant::now();
                    let recently_recovered = last_rec_hw
                        .get()
                        .map(|t| now.duration_since(t) < std::time::Duration::from_secs(3))
                        .unwrap_or(false);
                    if recently_recovered {
                        continue;
                    }
                    last_rec_hw.set(Some(now));
                    tracing::warn!(
                        "Stream Deck USB HID transport lost on {} — scheduling automatic recovery in 500ms",
                        dev_path.display()
                    );
                    let rescan_ref = rescan_hw_cell.clone();
                    let sys_sleep_ref = sys_sleep_hw.clone();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(500),
                        move || {
                            if sys_sleep_ref.get() {
                                return;
                            }
                            if let Some(cb) = rescan_ref.borrow().clone() {
                                let _ = cb("USB HID auto-recovery", true);
                            }
                        },
                    );
                    continue;
                }

                // Block all hardware keys, dials, and touch input while screen is locked or suspending
                if sys_sleep_hw.get() || locked_hw.get() {
                    continue;
                }

                let Some(dev) = devs_hw.borrow().get(dev_idx).cloned() else {
                    continue;
                };
                let dev_key = dev.stable_key();
                let is_active_deck = dev_idx == *act_hw.borrow();

                match event {
                    HardwareEvent::KeyDown { key_index } => {
                        tracing::info!(
                            "Physical Stream Deck Key Down [{}]: key {}",
                            dev.name,
                            key_index
                        );
                        if sleep_hw.borrow().get(&dev_key).copied().unwrap_or(false) {
                            disp_hw(dev_key.clone(), "sleep".into(), vec![HostCommand::ToggleSleep], None);
                            continue;
                        }

                        let prev_target = *inspector_hw.current_target.borrow();
                        let new_target = InspectorTarget::Key(key_index);
                        if is_active_deck {
                            canvas_hw.select_key(key_index);
                            canvas_hw.set_key_pressed(key_index, true);
                            inspector_hw.set_target(new_target);
                        }

                        let slot_str = format!("key_{key_index}");
                        if nh_hw.has_slot(&dev_key, &slot_str) {
                            let (cmds, updated) = nh_hw.handle_key_event(&dev_key, &slot_str, true);
                            disp_hw(dev_key.clone(), slot_str, cmds, updated);
                            if is_active_deck && prev_target != new_target {
                                let prof = profile_hw.borrow();
                                if let Some(page) = prof.current_page() {
                                    inspector_hw.set_key_action(page.keys.get(&key_index));
                                }
                            }
                            continue;
                        }

                        let mut is_plugin_action = false;
                        let mut toggled = false;
                        let mut updated_bg_prof: Option<Profile> = None;

                        if is_active_deck {
                            let mut prof = profile_hw.borrow_mut();
                            if let Some(page) = prof.current_page_mut() {
                                if let Some(action) = page.keys.get_mut(&key_index) {
                                    if !action.plugin_id.is_empty() && action.plugin_id != "system" {
                                        is_plugin_action = true;
                                    } else {
                                        if action.states.len() > 1 {
                                            action.toggle_state();
                                            toggled = true;
                                        }
                                        if action.action_id == "app_launcher.launch" {
                                            if let Some(desktop_id) = action.parameters.get("desktop_id") {
                                                let _ = pm_hw.borrow().launch_app_desktop(desktop_id);
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            let mut prof_list = profiles_hw.borrow_mut();
                            if let Some(p_idx) = find_profile_idx_for_device(
                                &prof_list,
                                &active_by_dev_hw.borrow(),
                                &dev_key,
                                dev.kind,
                            ) {
                                if let Some(page) = prof_list[p_idx].current_page_mut() {
                                    if let Some(action) = page.keys.get_mut(&key_index) {
                                        if !action.plugin_id.is_empty() && action.plugin_id != "system" {
                                            is_plugin_action = true;
                                        } else {
                                            if action.states.len() > 1 {
                                                action.toggle_state();
                                                toggled = true;
                                            }
                                            if action.action_id == "app_launcher.launch" {
                                                if let Some(desktop_id) = action.parameters.get("desktop_id") {
                                                    let _ = pm_hw.borrow().launch_app_desktop(desktop_id);
                                                }
                                            }
                                        }
                                    }
                                }
                                if toggled {
                                    updated_bg_prof = Some(prof_list[p_idx].clone());
                                }
                            }
                        }

                        if is_plugin_action {
                            pm_hw
                                .borrow()
                                .bridge
                                .send_key_event(&scoped_bridge_slot(&dev_key, &slot_str), true);
                        }

                        if is_active_deck {
                            let prof = profile_hw.borrow();
                            if toggled {
                                canvas_hw.render_profile_page(&prof);
                                Self::sync_hardware(&devs_hw, *act_hw.borrow(), &hwm_hw, &prof);
                            }
                            if prev_target != new_target || toggled {
                                if let Some(page) = prof.current_page() {
                                    inspector_hw.set_key_action(page.keys.get(&key_index));
                                }
                            }
                        } else if let Some(up) = updated_bg_prof {
                            hwm_hw
                                .borrow()
                                .sync_hardware_page(&dev.dev_path, &dev_key, dev.kind, &up);
                            ProfileStore::save_profiles(
                                &profiles_hw.borrow(),
                                &active_by_dev_hw.borrow(),
                            );
                        }
                    }
                    HardwareEvent::KeyUp { key_index } => {
                        tracing::info!(
                            "Physical Stream Deck Key Up [{}]: key {}",
                            dev.name,
                            key_index
                        );
                        if is_active_deck {
                            canvas_hw.set_key_pressed(key_index, false);
                        }
                        let slot_str = format!("key_{key_index}");
                        if nh_hw.has_slot(&dev_key, &slot_str) {
                            let (cmds, updated) = nh_hw.handle_key_event(&dev_key, &slot_str, false);
                            disp_hw(dev_key, slot_str, cmds, updated);
                            continue;
                        }
                        let is_plugin_action = if is_active_deck {
                            profile_hw
                                .borrow()
                                .current_page()
                                .and_then(|p| p.keys.get(&key_index))
                                .map(|a| !a.plugin_id.is_empty() && a.plugin_id != "system")
                                .unwrap_or(false)
                        } else {
                            let prof_list = profiles_hw.borrow();
                            find_profile_idx_for_device(
                                &prof_list,
                                &active_by_dev_hw.borrow(),
                                &dev_key,
                                dev.kind,
                            )
                            .and_then(|i| prof_list[i].current_page())
                            .and_then(|p| p.keys.get(&key_index))
                            .map(|a| !a.plugin_id.is_empty() && a.plugin_id != "system")
                            .unwrap_or(false)
                        };
                        if is_plugin_action {
                            pm_hw
                                .borrow()
                                .bridge
                                .send_key_event(&scoped_bridge_slot(&dev_key, &slot_str), false);
                        }
                    }
                    HardwareEvent::DialRotate { dial_index, delta } => {
                        tracing::info!(
                            "Physical Stream Deck Dial Rotate [{}]: dial {}, delta {}",
                            dev.name,
                            dial_index,
                            delta
                        );
                        if is_active_deck {
                            canvas_hw.rotate_dial(dial_index, delta);
                        }
                        let slot_str = format!("dial_{dial_index}");
                        if nh_hw.has_slot(&dev_key, &slot_str) {
                            let (cmds, updated) = nh_hw.handle_dial_rotate(&dev_key, &slot_str, delta);
                            disp_hw(dev_key, slot_str, cmds, updated);
                        } else {
                            pm_hw.borrow().bridge.send_dial_event(
                                &scoped_bridge_slot(&dev_key, &slot_str),
                                "turn",
                                delta,
                            );
                        }
                    }
                    HardwareEvent::DialPress { dial_index } => {
                        tracing::info!(
                            "Physical Stream Deck Dial Press [{}]: dial {}",
                            dev.name,
                            dial_index
                        );
                        if is_active_deck {
                            canvas_hw.set_dial_pressed(dial_index, true);
                        }
                        let slot_str = format!("dial_{dial_index}");
                        if nh_hw.has_slot(&dev_key, &slot_str) {
                            let (cmds, updated) = nh_hw.handle_dial_press(&dev_key, &slot_str, true);
                            disp_hw(dev_key, slot_str, cmds, updated);
                        } else {
                            pm_hw.borrow().bridge.send_dial_event(
                                &scoped_bridge_slot(&dev_key, &slot_str),
                                "press",
                                0,
                            );
                        }
                    }
                    HardwareEvent::DialRelease { dial_index } => {
                        tracing::info!(
                            "Physical Stream Deck Dial Release [{}]: dial {}",
                            dev.name,
                            dial_index
                        );
                        if is_active_deck {
                            canvas_hw.set_dial_pressed(dial_index, false);
                        }
                        let slot_str = format!("dial_{dial_index}");
                        if nh_hw.has_slot(&dev_key, &slot_str) {
                            let (cmds, updated) = nh_hw.handle_dial_press(&dev_key, &slot_str, false);
                            disp_hw(dev_key, slot_str, cmds, updated);
                        } else {
                            pm_hw.borrow().bridge.send_dial_event(
                                &scoped_bridge_slot(&dev_key, &slot_str),
                                "release",
                                0,
                            );
                        }
                    }
                    HardwareEvent::TouchTap { slot, x, y } => {
                        tracing::info!(
                            "Physical Stream Deck Touch Tap [{}]: slot {}, x {}, y {}",
                            dev.name,
                            slot,
                            x,
                            y
                        );
                        if is_active_deck {
                            canvas_hw.pulse_dial_press(slot, 115);
                        }
                        let slot_str = format!("dial_{slot}");
                        let local_x = (x as f64 - (slot as f64 * 200.0)).clamp(0.0, 200.0);
                        let norm_x = (local_x / 200.0).clamp(0.0, 1.0);
                        let norm_y = (y as f64 / 100.0).clamp(0.0, 1.0);
                        if nh_hw.has_slot(&dev_key, &slot_str) {
                            let (cmds, updated) =
                                nh_hw.handle_touch_tap(&dev_key, &slot_str, norm_x, norm_y);
                            disp_hw(dev_key, slot_str, cmds, updated);
                        } else {
                            pm_hw.borrow().bridge.send_dial_tap(
                                &scoped_bridge_slot(&dev_key, &slot_str),
                                norm_x,
                                norm_y,
                            );
                        }
                    }
                    HardwareEvent::TouchSwipe { is_right } => {
                        tracing::info!(
                            "Physical Stream Deck Touch Swipe [{}]: right={}",
                            dev.name,
                            is_right
                        );
                        let is_swipe_active = if is_active_deck {
                            let p = profile_hw.borrow();
                            p.current_page()
                                .and_then(|pg| pg.touchscreen_action.as_ref())
                                .map(|a| a.action_id != "disabled")
                                .unwrap_or(true)
                        } else {
                            let list = profiles_hw.borrow();
                            find_profile_idx_for_device(
                                &list,
                                &active_by_dev_hw.borrow(),
                                &dev_key,
                                dev.kind,
                            )
                            .and_then(|i| list[i].current_page())
                            .and_then(|pg| pg.touchscreen_action.as_ref())
                            .map(|a| a.action_id != "disabled")
                            .unwrap_or(true)
                        };
                        if is_swipe_active {
                            let p_names: Vec<String> = profiles_hw
                                .borrow()
                                .iter()
                                .filter(|p| p.device_kind == dev.kind)
                                .map(|p| p.name.clone())
                                .collect();
                            let cur_b = WindowState::load().brightness_for(&dev_key);
                            if is_active_deck {
                                let mut p = profile_hw.borrow_mut();
                                if is_right {
                                    p.prev_page();
                                } else {
                                    p.next_page();
                                }
                                let p_imm = p.clone();
                                drop(p);
                                persist_hw();
                                sync_device_runtime(
                                    &dev_key,
                                    &p_imm,
                                    p_names,
                                    cur_b,
                                    &pm_hw.borrow(),
                                    &nh_hw,
                                );
                                canvas_hw.render_profile_page(&p_imm);
                                let cur_target = *inspector_hw.current_target.borrow();
                                inspector_hw.set_key_action(get_target_ref(&p_imm, cur_target));
                                Self::sync_hardware(&devs_hw, *act_hw.borrow(), &hwm_hw, &p_imm);
                            } else {
                                let mut updated_prof: Option<Profile> = None;
                                {
                                    let mut list = profiles_hw.borrow_mut();
                                    if let Some(p_idx) = find_profile_idx_for_device(
                                        &list,
                                        &active_by_dev_hw.borrow(),
                                        &dev_key,
                                        dev.kind,
                                    ) {
                                        if is_right {
                                            list[p_idx].prev_page();
                                        } else {
                                            list[p_idx].next_page();
                                        }
                                        updated_prof = Some(list[p_idx].clone());
                                    }
                                }
                                if let Some(p_imm) = updated_prof {
                                    ProfileStore::save_profiles(
                                        &profiles_hw.borrow(),
                                        &active_by_dev_hw.borrow(),
                                    );
                                    sync_device_runtime(
                                        &dev_key,
                                        &p_imm,
                                        p_names,
                                        cur_b,
                                        &pm_hw.borrow(),
                                        &nh_hw,
                                    );
                                    hwm_hw.borrow().sync_hardware_page(
                                        &dev.dev_path,
                                        &dev_key,
                                        dev.kind,
                                        &p_imm,
                                    );
                                }
                            }
                        }
                    }
                    HardwareEvent::DeviceLost { .. } => {}
                }
            }
        });

        // Start simultaneous USB HID listeners for EVERY connected Stream Deck
        {
            let devs = connected_devices.borrow();
            for (idx, dev) in devs.iter().enumerate() {
                let s = sender.clone();
                hardware_manager.borrow().start_listener(
                    dev.dev_path.clone(),
                    dev.kind,
                    move |event| {
                        let _ = s.send_blocking((idx, event));
                    },
                );
            }
        }

        // ===================== Synchronize Profile & Header =====================
        let sync_all_with_hw = {
            let profs = profiles.clone();
            let act_id = active_profile_id.clone();
            let act_by_dev = active_by_device.clone();
            let p_cur = profile.clone();
            let c_cur = canvas.clone();
            let insp_cur = inspector.clone();
            let pm_cur = plugin_manager.clone();
            let nh_cur = native_host.clone();
            let d_cur = connected_devices.clone();
            let act_idx = active_device_idx.clone();
            let h_cur = hardware_manager.clone();
            let hdr = header.clone();

            Rc::new(move |push_to_hw: bool| {
                let devs = d_cur.borrow();
                let cur_idx = *act_idx.borrow();
                let cur_dev = devs.get(cur_idx).cloned();
                let dev_kind = cur_dev.as_ref().map(|d| d.kind).unwrap_or(DeviceKind::StreamDeckPlus);
                let dev_key = cur_dev
                    .as_ref()
                    .map(|d| d.stable_key())
                    .unwrap_or_else(|| format!("{dev_kind:?}"));
                drop(devs);

                let mut profs_b = profs.borrow_mut();
                let matching_indices: Vec<usize> = profs_b
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.device_kind == dev_kind)
                    .map(|(i, _)| i)
                    .collect();

                let cur_id = act_id.borrow().clone();
                let found_idx = matching_indices
                    .iter()
                    .find(|&&i| profs_b[i].id == cur_id)
                    .copied()
                    .or_else(|| {
                        let mapped = act_by_dev.borrow().get(&dev_key).cloned();
                        mapped.and_then(|mid| matching_indices.iter().find(|&&i| profs_b[i].id == mid).copied())
                    })
                    .or_else(|| matching_indices.first().copied());

                if let Some(target_idx) = found_idx {
                    *act_id.borrow_mut() = profs_b[target_idx].id.clone();
                    *p_cur.borrow_mut() = profs_b[target_idx].clone();
                } else {
                    let new_p = Profile::new_for_device("Default Profile", dev_kind);
                    *act_id.borrow_mut() = new_p.id.clone();
                    *p_cur.borrow_mut() = new_p.clone();
                    profs_b.push(new_p);
                }

                act_by_dev.borrow_mut().insert(dev_key.clone(), act_id.borrow().clone());

                let p_names: Vec<String> = profs_b
                    .iter()
                    .filter(|p| p.device_kind == dev_kind)
                    .map(|p| p.name.clone())
                    .collect();
                let selected_pos = profs_b
                    .iter()
                    .filter(|p| p.device_kind == dev_kind)
                    .position(|p| p.id == *act_id.borrow())
                    .unwrap_or(0);
                ProfileStore::save_profiles(&profs_b, &act_by_dev.borrow());
                drop(profs_b);

                c_cur.set_active_device_key(&dev_key);
                insp_cur.set_active_device_key(&dev_key);
                hdr.update_profiles(&p_names, selected_pos);
                let prof = p_cur.borrow();
                let cur_b = WindowState::load().brightness_for(&dev_key);
                sync_device_runtime(
                    &dev_key,
                    &prof,
                    p_names,
                    cur_b,
                    &pm_cur.borrow(),
                    &nh_cur,
                );
                c_cur.render_profile_page(&prof);
                let cur_target = *insp_cur.current_target.borrow();
                insp_cur.set_key_action(get_target_ref(&prof, cur_target));
                if push_to_hw {
                    Self::sync_hardware(&d_cur, cur_idx, &h_cur, &prof);
                }
            })
        };

        let sync_all: Rc<dyn Fn()> = {
            let f = sync_all_with_hw.clone();
            Rc::new(move || f(true))
        };

        // Run initial UI & bridge sync for active device PLUS all background connected decks
        // so EVERY deck has live native + bridge action runtimes immediately from boot!
        sync_all_with_hw(false);
        {
            let devs = connected_devices.borrow();
            let cur_idx = *active_device_idx.borrow();
            let prof_list = profiles.borrow();
            let act_map = active_by_device.borrow();
            let store_snap = ProfileStore {
                profiles: prof_list.clone(),
                active_by_device: act_map.clone(),
            };
            let ws = WindowState::load();
            let pm = plugin_manager.borrow();
            for (idx, d) in devs.iter().enumerate() {
                if idx == cur_idx {
                    continue;
                }
                let skey = d.stable_key();
                let p = store_snap.active_profile_for(&skey, d.kind);
                let p_names: Vec<String> = prof_list
                    .iter()
                    .filter(|pr| pr.device_kind == d.kind)
                    .map(|pr| pr.name.clone())
                    .collect();
                let cur_b = ws.brightness_for(&skey);
                sync_device_runtime(&skey, &p, p_names, cur_b, &pm, &native_host);
            }
        }

        // Connect Header Profile Dropdown switching
        let profs_dd = profiles.clone();
        let act_id_dd = active_profile_id.clone();
        let devs_dd = connected_devices.clone();
        let act_idx_dd = active_device_idx.clone();
        let sync_dd = sync_all.clone();
        let persist_dd = persist_profiles.clone();
        header.profile_dropdown.connect_selected_notify(move |dd| {
            let sel = dd.selected() as usize;
            let devs = devs_dd.borrow();
            let cur_idx = *act_idx_dd.borrow();
            let dev_kind = devs.get(cur_idx).map(|d| d.kind).unwrap_or(DeviceKind::StreamDeckPlus);
            drop(devs);

            let profs_b = profs_dd.borrow();
            let matching: Vec<_> = profs_b.iter().filter(|p| p.device_kind == dev_kind).collect();
            if let Some(target) = matching.get(sel) {
                if target.id != *act_id_dd.borrow() {
                    let target_id = target.id.clone();
                    drop(profs_b);
                    persist_dd();
                    *act_id_dd.borrow_mut() = target_id;
                    sync_dd();
                }
            }
        });

        // ===================== Connect Device Dropdown Switcher =====================
        let canvas_dev = canvas.clone();
        let profile_dev = profile.clone();
        let inspector_dev = inspector.clone();
        let pm_dev = plugin_manager.clone();
        let nh_dev = native_host.clone();
        let disp_dev = dispatch_native.clone();
        let devs_select = connected_devices.clone();
        let hw_dev_switch = hardware_manager.clone();
        let active_idx_switch = active_device_idx.clone();
        let act_id_dev = active_profile_id.clone();
        let act_by_dev_sw = active_by_device.clone();
        let sync_dev_change = sync_all.clone();
        let persist_dev = persist_profiles.clone();
        let work_paned_dev = work_paned.clone();
        let syncing_device_dropdown = Rc::new(Cell::new(false));
        let sync_dd_guard = syncing_device_dropdown.clone();

        header.device_dropdown.connect_selected_notify(move |dd| {
            if sync_dd_guard.get() {
                return;
            }
            let sel_idx = dd.selected() as usize;
            // Save edits on the previous device before switching active canvas view
            persist_dev();
            let dev_opt = devs_select.borrow().get(sel_idx).cloned();
            if let Some(dev) = dev_opt {
                *active_idx_switch.borrow_mut() = sel_idx;

                if let Some(saved_pid) = act_by_dev_sw.borrow().get(&dev.stable_key()).cloned() {
                    *act_id_dev.borrow_mut() = saved_pid;
                }

                // 1. Rebuild canvas grid, touchscreen, and dials for the selected device
                canvas_dev.build_hardware_layout(dev.kind);
                canvas_dev.update_scale(work_paned_dev.position());
                canvas_dev.select_key(0);
                inspector_dev.set_target(InspectorTarget::Key(0));
                sync_dev_change();

                // 2. Rebind click events to newly created buttons and dials
                Self::bind_canvas_events(
                    &canvas_dev,
                    &inspector_dev,
                    &profile_dev,
                    &pm_dev,
                    &nh_dev,
                    &disp_dev,
                    &devs_select,
                    &active_idx_switch,
                    &hw_dev_switch,
                );
            }
        });

        // ===================== Unified Hardware Rescan & Full-Sync Helper =====================
        // Used by System Resume (PrepareForSleep), Tray -> Rescan Hardware, and USB HID auto-recovery.
        let rescan_and_sync_hardware: Rc<dyn Fn(&str, bool) -> usize> = {
            let hw_r = hardware_manager.clone();
            let devs_r = connected_devices.clone();
            let hdr_r = header.clone();
            let tray_r = tray.clone();
            let sender_r = sender.clone();
            let profs_r = profiles.clone();
            let act_id_r = active_profile_id.clone();
            let act_map_r = active_by_device.clone();
            let sync_ui_r = sync_all_with_hw.clone();
            let persist_r = persist_profiles.clone();
            let canvas_r = canvas.clone();
            let work_paned_r = work_paned.clone();
            let insp_r = inspector.clone();
            let prof_r = profile.clone();
            let pm_r = plugin_manager.clone();
            let nh_r = native_host.clone();
            let disp_r = dispatch_native.clone();
            let act_idx_r = active_device_idx.clone();
            let sleep_r = deck_sleeping.clone();
            let locked_r = screen_locked.clone();
            let sync_dd_r = syncing_device_dropdown.clone();

            Rc::new(move |reason: &str, force_reset_transport: bool| -> usize {
                tracing::info!("Re-enumerating Elgato Stream Deck hardware ({reason})...");
                persist_r();

                let prev_stable_key = devs_r
                    .borrow()
                    .get(*act_idx_r.borrow())
                    .map(|d| d.stable_key());

                if force_reset_transport {
                    hw_r.borrow().reset_transport();
                } else {
                    hw_r.borrow().stop_all_listeners();
                    hw_r.borrow().stop_all_animations();
                }

                let new_devs = hw_r.borrow().scan_devices();
                let restored_idx = prev_stable_key
                    .as_deref()
                    .and_then(|prev_key| new_devs.iter().position(|d| d.stable_key() == prev_key))
                    .unwrap_or(0);

                let tray_names: Vec<String> = new_devs.iter().map(|d| d.name.clone()).collect();
                *devs_r.borrow_mut() = new_devs.clone();
                *act_idx_r.borrow_mut() = restored_idx;

                sync_dd_r.set(true);
                hdr_r.populate_devices(&new_devs);
                if restored_idx < new_devs.len() {
                    hdr_r.device_dropdown.set_selected(restored_idx as u32);
                }
                sync_dd_r.set(false);
                tray_r.borrow().update_devices(tray_names);

                if let Some(active_dev) = new_devs.get(restored_idx) {
                    if let Some(saved_pid) = act_map_r.borrow().get(&active_dev.stable_key()).cloned() {
                        *act_id_r.borrow_mut() = saved_pid;
                    }
                    canvas_r.build_hardware_layout(active_dev.kind);
                    canvas_r.update_scale(work_paned_r.position());
                    Self::bind_canvas_events(
                        &canvas_r,
                        &insp_r,
                        &prof_r,
                        &pm_r,
                        &nh_r,
                        &disp_r,
                        &devs_r,
                        &act_idx_r,
                        &hw_r,
                    );
                }

                // Refresh UI, NativePluginHost slots, and Python bridge WITHOUT redundant sync hardware push
                sync_ui_r(false);

                let active_keys: std::collections::HashSet<String> =
                    new_devs.iter().map(|d| d.stable_key()).collect();
                nh_r.retain_devices(&active_keys);

                let st = WindowState::load();
                let profs_snap = profs_r.borrow().clone();
                let store_snap = ProfileStore {
                    profiles: profs_snap.clone(),
                    active_by_device: act_map_r.borrow().clone(),
                };
                let pm_guard = pm_r.borrow();
                let mut targets = Vec::new();
                for (idx, d) in new_devs.iter().enumerate() {
                    let skey = d.stable_key();
                    let p = store_snap.active_profile_for(&skey, d.kind);
                    let b = if sleep_r.borrow().get(&skey).copied().unwrap_or(false) {
                        0
                    } else {
                        st.brightness_for(&skey)
                    };
                    if idx != restored_idx {
                        let p_names: Vec<String> = profs_snap
                            .iter()
                            .filter(|pr| pr.device_kind == d.kind)
                            .map(|pr| pr.name.clone())
                            .collect();
                        sync_device_runtime(&skey, &p, p_names, b, &pm_guard, &nh_r);
                    }
                    targets.push((d.dev_path.clone(), skey, d.kind, p, b));
                    let s = sender_r.clone();
                    hw_r.borrow().start_listener(d.dev_path.clone(), d.kind, move |ev| {
                        let _ = s.send_blocking((idx, ev));
                    });
                }
                drop(pm_guard);
                if !targets.is_empty() {
                    if locked_r.get() {
                        let brand_targets: Vec<_> = targets
                            .iter()
                            .map(|(p, _, kind, _, b)| (p.clone(), *kind, *b))
                            .collect();
                        hw_r.borrow()
                            .fade_to_branding_all_async(brand_targets, force_reset_transport);
                    } else {
                        let assumed = if force_reset_transport { Some(0) } else { None };
                        hw_r.borrow().fade_in_all_devices_async(targets, assumed);
                    }
                }

                tracing::info!(
                    "Hardware re-enumeration ({reason}) complete: {} device(s) active",
                    new_devs.len()
                );
                new_devs.len()
            })
        };
        *rescan_cb_cell.borrow_mut() = Some(rescan_and_sync_hardware.clone());

        // ===================== Wire Native System Tray Events =====================
        {
            let win_tray = window.clone();
            let quit_tray = perform_full_quit.clone();
            let rescan_tray = rescan_and_sync_hardware.clone();

            glib::spawn_future_local(async move {
                while let Ok(cmd) = tray_rx.recv().await {
                    match cmd {
                        TrayCommand::ToggleWindow => {
                            if win_tray.is_visible() {
                                win_tray.set_visible(false);
                            } else {
                                win_tray.set_visible(true);
                                win_tray.present();
                            }
                        }
                        TrayCommand::ShowWindow => {
                            win_tray.set_visible(true);
                            win_tray.present();
                        }
                        TrayCommand::RescanHardware => {
                            let _ = rescan_tray("system tray", true);
                        }
                        TrayCommand::Quit => {
                            quit_tray();
                        }
                    }
                }
            });
        }

        // ===================== System Power (Suspend/Shutdown) & Lock Screen Monitor =====================
        {
            let hw_pwr = hardware_manager.clone();
            let devs_pwr = connected_devices.clone();
            let prof_pwr = profile.clone();
            let profs_pwr = profiles.clone();
            let act_map_pwr = active_by_device.clone();
            let nh_pwr = native_host.clone();
            let disp_pwr = dispatch_native.clone();
            let sleep_decks_pwr = deck_sleeping.clone();
            let persist_pwr = persist_profiles.clone();
            let sys_sleep_pwr = system_sleeping.clone();
            let locked_pwr = screen_locked.clone();
            let shut_done_pwr = shutdown_transition_done.clone();
            let rescan_pwr = rescan_and_sync_hardware.clone();
            let pm_cell_cb = power_monitor_cell.clone();
            let expected_before_sleep = Rc::new(Cell::new(connected_devices.borrow().len()));
            let resume_gen = Rc::new(Cell::new(0u64));

            let monitor = Rc::new(PowerMonitor::start(move |ev| match ev {
                PowerEvent::ScreenLocked => {
                    locked_pwr.set(true);
                    if sys_sleep_pwr.get() {
                        return;
                    }
                    let devs = devs_pwr.borrow().clone();
                    let st = WindowState::load();
                    let brand_targets: Vec<_> = devs
                        .iter()
                        .map(|d| {
                            let skey = d.stable_key();
                            let b = if sleep_decks_pwr.borrow().get(&skey).copied().unwrap_or(false)
                            {
                                0
                            } else {
                                st.brightness_for(&skey)
                            };
                            (d.dev_path.clone(), d.kind, b)
                        })
                        .collect();
                    tracing::info!(
                        "Screen locked — blocking Stream Deck input and fading {} device(s) to Ballista branding",
                        brand_targets.len()
                    );
                    if !brand_targets.is_empty() {
                        hw_pwr
                            .borrow()
                            .fade_to_branding_all_async(brand_targets, false);
                    } else {
                        hw_pwr.borrow().set_branding_locked(true);
                    }
                }
                PowerEvent::ScreenUnlocked => {
                    locked_pwr.set(false);
                    if sys_sleep_pwr.get() {
                        return;
                    }
                    // Immediately advance due native plugin ticks (Clock, System Metrics, Media)
                    // BEFORE fading in so unlocked decks display the exact current time on frame 1!
                    for (dev_key, slot, cmds) in nh_pwr.tick_due() {
                        let upd = nh_pwr.get_slot_settings(&dev_key, &slot);
                        disp_pwr(dev_key, slot, cmds, upd);
                    }

                    let devs = devs_pwr.borrow().clone();
                    let st = WindowState::load();
                    let active_gui_prof = prof_pwr.borrow().clone();
                    let mut profs_snap = profs_pwr.borrow().clone();
                    if let Some(slot) = profs_snap.iter_mut().find(|p| p.id == active_gui_prof.id) {
                        *slot = active_gui_prof;
                    }
                    let store_snap = ProfileStore {
                        profiles: profs_snap,
                        active_by_device: act_map_pwr.borrow().clone(),
                    };
                    let targets: Vec<_> = devs
                        .iter()
                        .map(|d| {
                            let skey = d.stable_key();
                            let p = store_snap.active_profile_for(&skey, d.kind);
                            let b = if sleep_decks_pwr.borrow().get(&skey).copied().unwrap_or(false)
                            {
                                0
                            } else {
                                st.brightness_for(&skey)
                            };
                            (d.dev_path.clone(), skey, d.kind, p, b)
                        })
                        .collect();
                    tracing::info!(
                        "Screen unlocked — fading {} device(s) back into live Action UI",
                        targets.len()
                    );
                    if !targets.is_empty() {
                        hw_pwr.borrow().fade_in_all_devices_async(targets, None);
                    } else {
                        hw_pwr.borrow().set_branding_locked(false);
                    }
                }
                PowerEvent::ShuttingDown => {
                    resume_gen.set(resume_gen.get().wrapping_add(1));
                    sys_sleep_pwr.set(true);
                    locked_pwr.set(true);
                    tracing::info!(
                        "System preparing for shutdown/reboot — fading {} device(s) to Ballista branding -> 0%...",
                        devs_pwr.borrow().len()
                    );
                    persist_pwr();
                    if !shut_done_pwr.replace(true) {
                        hw_pwr.borrow().prepare_for_shutdown(true);
                    }
                    if let Some(ref pm) = *pm_cell_cb.borrow() {
                        pm.release_shutdown_inhibitor();
                        pm.release_sleep_inhibitor();
                    }
                }
                PowerEvent::Suspending => {
                    resume_gen.set(resume_gen.get().wrapping_add(1));
                    let dev_count = devs_pwr.borrow().len();
                    expected_before_sleep.set(dev_count);
                    sys_sleep_pwr.set(true);
                    tracing::info!(
                        "System preparing for suspend — fading {} device(s) to Ballista branding -> 0% and releasing USB HID handles...",
                        dev_count
                    );
                    persist_pwr();
                    hw_pwr.borrow().prepare_for_suspend();
                    if let Some(ref pm) = *pm_cell_cb.borrow() {
                        pm.release_sleep_inhibitor();
                    }
                }
                PowerEvent::Resumed => {
                    let cur_gen = resume_gen.get().wrapping_add(1);
                    resume_gen.set(cur_gen);
                    sys_sleep_pwr.set(true);
                    tracing::info!(
                        "System resumed from sleep/suspend — scheduling Stream Deck restoration..."
                    );
                    if let Some(ref pm) = *pm_cell_cb.borrow() {
                        pm.acquire_sleep_inhibitor();
                    }

                    let rescan_1 = rescan_pwr.clone();
                    let sys_sleep_1 = sys_sleep_pwr.clone();
                    let locked_1 = locked_pwr.clone();
                    let pm_cell_1 = pm_cell_cb.clone();
                    let gen_1 = resume_gen.clone();
                    let expected = expected_before_sleep.get();

                    // Pass 1 (+750ms): Allow USB root hubs to settle, recreate libusb context,
                    // check whether GNOME woke onto the Lock Screen, and fade in branding or Action UI
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(750),
                        move || {
                            if gen_1.get() != cur_gen {
                                return;
                            }
                            if let Some(ref pm) = *pm_cell_1.borrow() {
                                let is_locked = pm.is_screen_locked();
                                locked_1.set(is_locked);
                                tracing::info!(
                                    "Resume pass 1 screen lock state: locked={is_locked}"
                                );
                            }
                            sys_sleep_1.set(false);
                            let found = rescan_1("system resume", true);
                            if found < expected {
                                tracing::info!(
                                    "Resume pass 1 found {found}/{expected} Stream Deck(s) — scheduling follow-up scan in 2.0s..."
                                );
                                let rescan_2 = rescan_1.clone();
                                let sys_sleep_2 = sys_sleep_1.clone();
                                let gen_2 = gen_1.clone();
                                glib::timeout_add_local_once(
                                    std::time::Duration::from_millis(2000),
                                    move || {
                                        if gen_2.get() != cur_gen || sys_sleep_2.get() {
                                            return;
                                        }
                                        let found2 = rescan_2("system resume retry", true);
                                        if found2 < expected {
                                            tracing::info!(
                                                "Resume pass 2 found {found2}/{expected} Stream Deck(s) — scheduling final scan in 3.0s..."
                                            );
                                            let rescan_3 = rescan_2.clone();
                                            let sys_sleep_3 = sys_sleep_2.clone();
                                            let gen_3 = gen_2.clone();
                                            glib::timeout_add_local_once(
                                                std::time::Duration::from_millis(3000),
                                                move || {
                                                    if gen_3.get() != cur_gen || sys_sleep_3.get() {
                                                        return;
                                                    }
                                                    let _ = rescan_3(
                                                        "system resume final retry",
                                                        true,
                                                    );
                                                },
                                            );
                                        }
                                    },
                                );
                            }
                        },
                    );
                }
            }));
            *power_monitor_cell.borrow_mut() = Some(monitor);
        }

        // ===================== Connect Header Actions =====================
        let win_clone1 = window.clone();
        let hw_settings = hardware_manager.clone();
        let devs_settings = connected_devices.clone();
        let active_idx_settings = active_device_idx.clone();
        let profiles_settings = profiles.clone();
        let active_prof_id_settings = active_profile_id.clone();
        let sync_settings = sync_all.clone();

        header.settings_button.connect_clicked(move |_| {
            let cur_idx = *active_idx_settings.borrow();
            let settings_win = create_settings_window(
                &win_clone1,
                hw_settings.clone(),
                devs_settings.clone(),
                cur_idx,
                profiles_settings.clone(),
                active_prof_id_settings.clone(),
                sync_settings.clone(),
            );
            settings_win.present();
        });

        // Store Window
        let win_clone2 = window.clone();
        let pm_store = plugin_manager.clone();
        let refresh_sidebar_store = refresh_sidebar.clone();
        let open_ps_store = open_plugin_settings.clone();

        header.store_button.connect_clicked(move |_| {
            let on_installed = refresh_sidebar_store.clone();
            let store_win = create_store_window(
                &win_clone2,
                pm_store.clone(),
                on_installed,
                open_ps_store.clone(),
            );
            store_win.present();
        });

        // ===================== Connect Pagination Actions =====================
        let prof_page_add = profile.clone();
        let profs_page_add = profiles.clone();
        let canvas_page_add = canvas.clone();
        let insp_page_add = inspector.clone();
        let pm_page_add = plugin_manager.clone();
        let nh_page_add = native_host.clone();
        let hw_page_add = hardware_manager.clone();
        let devs_page_add = connected_devices.clone();
        let act_page_add = active_device_idx.clone();
        let persist_page_add = persist_profiles.clone();

        canvas.add_page_btn.connect_clicked(move |_| {
            {
                let mut p = prof_page_add.borrow_mut();
                p.add_page();
            }
            persist_page_add();
            let p = prof_page_add.borrow();
            let dkey = active_device_key_for(&devs_page_add, &act_page_add, p.device_kind);
            let p_names: Vec<String> = profs_page_add
                .borrow()
                .iter()
                .filter(|pr| pr.device_kind == p.device_kind)
                .map(|pr| pr.name.clone())
                .collect();
            let cur_b = WindowState::load().brightness_for(&dkey);
            sync_device_runtime(&dkey, &p, p_names, cur_b, &pm_page_add.borrow(), &nh_page_add);
            canvas_page_add.render_profile_page(&p);
            let cur_target = *insp_page_add.current_target.borrow();
            insp_page_add.set_key_action(get_target_ref(&p, cur_target));
            drop(p);
            Self::sync_hardware(&devs_page_add, *act_page_add.borrow(), &hw_page_add, &prof_page_add.borrow());
        });

        let prof_page_del = profile.clone();
        let profs_page_del = profiles.clone();
        let canvas_page_del = canvas.clone();
        let insp_page_del = inspector.clone();
        let pm_page_del = plugin_manager.clone();
        let nh_page_del = native_host.clone();
        let hw_page_del = hardware_manager.clone();
        let devs_page_del = connected_devices.clone();
        let act_page_del = active_device_idx.clone();
        let persist_page_del = persist_profiles.clone();

        canvas.remove_page_btn.connect_clicked(move |_| {
            {
                let mut p = prof_page_del.borrow_mut();
                p.remove_current_page();
            }
            persist_page_del();
            let p = prof_page_del.borrow();
            let dkey = active_device_key_for(&devs_page_del, &act_page_del, p.device_kind);
            let p_names: Vec<String> = profs_page_del
                .borrow()
                .iter()
                .filter(|pr| pr.device_kind == p.device_kind)
                .map(|pr| pr.name.clone())
                .collect();
            let cur_b = WindowState::load().brightness_for(&dkey);
            sync_device_runtime(&dkey, &p, p_names, cur_b, &pm_page_del.borrow(), &nh_page_del);
            canvas_page_del.render_profile_page(&p);
            let cur_target = *insp_page_del.current_target.borrow();
            insp_page_del.set_key_action(get_target_ref(&p, cur_target));
            drop(p);
            Self::sync_hardware(&devs_page_del, *act_page_del.borrow(), &hw_page_del, &prof_page_del.borrow());
        });

        let prof_page_sel = profile.clone();
        let profs_page_sel = profiles.clone();
        let canvas_page_sel = canvas.clone();
        let insp_page_sel = inspector.clone();
        let pm_page_sel = plugin_manager.clone();
        let nh_page_sel = native_host.clone();
        let hw_page_sel = hardware_manager.clone();
        let devs_page_sel = connected_devices.clone();
        let act_page_sel = active_device_idx.clone();
        let persist_page_sel = persist_profiles.clone();

        canvas.set_page_callback(move |page_idx| {
            {
                let mut p = prof_page_sel.borrow_mut();
                p.set_page(page_idx);
            }
            persist_page_sel();
            let p = prof_page_sel.borrow();
            let dkey = active_device_key_for(&devs_page_sel, &act_page_sel, p.device_kind);
            let p_names: Vec<String> = profs_page_sel
                .borrow()
                .iter()
                .filter(|pr| pr.device_kind == p.device_kind)
                .map(|pr| pr.name.clone())
                .collect();
            let cur_b = WindowState::load().brightness_for(&dkey);
            sync_device_runtime(&dkey, &p, p_names, cur_b, &pm_page_sel.borrow(), &nh_page_sel);
            canvas_page_sel.render_profile_page(&p);
            let cur_target = *insp_page_sel.current_target.borrow();
            insp_page_sel.set_key_action(get_target_ref(&p, cur_target));
            drop(p);
            Self::sync_hardware(&devs_page_sel, *act_page_sel.borrow(), &hw_page_sel, &prof_page_sel.borrow());
        });

        // ===================== Connect Inspector Controls =====================

        // 1. Title Entry Changed
        let prof_title = profile.clone();
        let canvas_title = canvas.clone();
        let insp_target_title = inspector.current_target.clone();
        let hw_title = hardware_manager.clone();
        let devs_title = connected_devices.clone();
        let act_title = active_device_idx.clone();
        let insp_updating_title = inspector.is_updating.clone();
        let persist_title = persist_profiles.clone();

        let insp_ui_title = inspector.clone();
        inspector.title_entry.connect_changed(move |entry| {
            if insp_updating_title.get() {
                return;
            }
            let target = *insp_target_title.borrow();
            let text = entry.text().to_string();
            let st = insp_ui_title.selected_state_idx.get();
            let mut show_now = true;
            let mut align_now = TextVerticalAlign::Bottom;
            {
                let mut prof = prof_title.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| {
                        app.title = text.clone();
                        app.top_label = None;
                        app.center_label = None;
                        app.bottom_label = None;
                        app.custom_title_override = true;
                        show_now = app.show_title;
                        align_now = app.vertical_align;
                    });
                }
            }
            persist_title();
            insp_ui_title.preview_top_label.set_text("");
            insp_ui_title.preview_top_label.set_visible(false);
            insp_ui_title.preview_center_label.set_text("");
            insp_ui_title.preview_center_label.set_visible(false);
            if show_now && !text.is_empty() {
                insp_ui_title.preview_inner_label.set_text(&text);
                insp_ui_title.preview_inner_label.set_visible(true);
            } else {
                insp_ui_title.preview_inner_label.set_text("");
                insp_ui_title.preview_inner_label.set_visible(false);
            }
            match align_now {
                TextVerticalAlign::Top => insp_ui_title.preview_inner_label.set_valign(gtk::Align::Start),
                TextVerticalAlign::Center => insp_ui_title.preview_inner_label.set_valign(gtk::Align::Center),
                TextVerticalAlign::Bottom => insp_ui_title.preview_inner_label.set_valign(gtk::Align::End),
            }
            if !text.is_empty() {
                insp_ui_title.mark_assigned();
            }
            let prof = prof_title.borrow();
            canvas_title.render_profile_page(&prof);
            drop(prof);
            Self::sync_hardware(&devs_title, *act_title.borrow(), &hw_title, &prof_title.borrow());
        });

        // 1b. Title Visibility Toggle (Hide / Show Title on Key)
        let prof_title_toggle = profile.clone();
        let canvas_title_toggle = canvas.clone();
        let insp_title_toggle = inspector.clone();
        let hw_title_toggle = hardware_manager.clone();
        let devs_title_toggle = connected_devices.clone();
        let act_title_toggle = active_device_idx.clone();
        let persist_title_toggle = persist_profiles.clone();

        inspector.title_toggle_btn.connect_clicked(move |btn| {
            let target = *insp_title_toggle.current_target.borrow();
            let st = insp_title_toggle.selected_state_idx.get();
            let mut is_shown = true;
            {
                let mut prof = prof_title_toggle.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| {
                        app.show_title = !app.show_title;
                        is_shown = app.show_title;
                    });
                }
            }
            persist_title_toggle();
            btn.set_icon_name(if is_shown { "view-reveal-symbolic" } else { "view-conceal-symbolic" });
            btn.set_tooltip_text(Some(if is_shown { "Hide Title on Key" } else { "Show Title on Key" }));

            let prof = prof_title_toggle.borrow();
            canvas_title_toggle.render_profile_page(&prof);
            let act_opt = match target {
                InspectorTarget::Key(k) => prof.current_page().and_then(|p| p.keys.get(&k)),
                InspectorTarget::Dial(d) => prof.current_page().and_then(|p| p.dial_actions.get(&d)),
                InspectorTarget::Touchscreen => prof.current_page().and_then(|p| p.touchscreen_action.as_ref()),
            };
            insp_title_toggle.set_key_action(act_opt);
            drop(prof);
            Self::sync_hardware(&devs_title_toggle, *act_title_toggle.borrow(), &hw_title_toggle, &prof_title_toggle.borrow());
        });

        // 2. Font Dialog Button (Typography)
        let prof_font = profile.clone();
        let canvas_font = canvas.clone();
        let insp_font = inspector.clone();
        let hw_font = hardware_manager.clone();
        let devs_font = connected_devices.clone();
        let act_font = active_device_idx.clone();
        let persist_font = persist_profiles.clone();

        inspector.font_dialog_button.connect_font_desc_notify(move |fdb| {
            if insp_font.is_updating.get() {
                return;
            }
            let target = *insp_font.current_target.borrow();
            let st = insp_font.selected_state_idx.get();
            if let Some(desc) = fdb.font_desc() {
                let fam = desc.family().map(|s| s.to_string()).unwrap_or_else(|| "Cantarell".into());
                let sz = (desc.size() / pango::SCALE).max(6) as u32;
                {
                    let mut prof = prof_font.borrow_mut();
                    if let Some(page) = prof.current_page_mut() {
                        mutate_target_appearance(page, target, st, |app| {
                            app.font_family = fam;
                            app.font_size = sz;
                        });
                    }
                }
                persist_font();
                insp_font.is_updating.set(true);
                insp_font.font_size_spin.set_value(sz as f64);
                insp_font.is_updating.set(false);

                let prof = prof_font.borrow();
                canvas_font.render_profile_page(&prof);
                drop(prof);
                Self::sync_hardware(&devs_font, *act_font.borrow(), &hw_font, &prof_font.borrow());
            }
        });

        // 3. Font Size Stepper
        let prof_spin = profile.clone();
        let canvas_spin = canvas.clone();
        let insp_spin = inspector.clone();
        let hw_spin = hardware_manager.clone();
        let devs_spin = connected_devices.clone();
        let act_spin = active_device_idx.clone();
        let persist_spin = persist_profiles.clone();

        inspector.font_size_spin.connect_value_changed(move |spin| {
            if insp_spin.is_updating.get() {
                return;
            }
            let target = *insp_spin.current_target.borrow();
            let st = insp_spin.selected_state_idx.get();
            let sz = spin.value() as u32;
            {
                let mut prof = prof_spin.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| app.font_size = sz);
                }
            }
            persist_spin();
            let prof = prof_spin.borrow();
            canvas_spin.render_profile_page(&prof);
            drop(prof);
            Self::sync_hardware(&devs_spin, *act_spin.borrow(), &hw_spin, &prof_spin.borrow());
        });

        // 4. Button Color Picker
        let prof_bg = profile.clone();
        let canvas_bg = canvas.clone();
        let insp_target_bg = inspector.current_target.clone();
        let hw_bg = hardware_manager.clone();
        let devs_bg = connected_devices.clone();
        let act_bg = active_device_idx.clone();
        let insp_updating_bg = inspector.is_updating.clone();
        let insp_bg_state = inspector.clone();
        let persist_bg = persist_profiles.clone();

        inspector.btn_color_picker.connect_rgba_notify(move |cp| {
            if insp_updating_bg.get() {
                return;
            }
            let target = *insp_target_bg.borrow();
            let st = insp_bg_state.selected_state_idx.get();
            let hex = rgba_to_hex(&cp.rgba());
            {
                let mut prof = prof_bg.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| app.bg_color = Some(hex));
                }
            }
            persist_bg();
            let prof = prof_bg.borrow();
            canvas_bg.render_profile_page(&prof);
            drop(prof);
            Self::sync_hardware(&devs_bg, *act_bg.borrow(), &hw_bg, &prof_bg.borrow());
        });

        // 5. Button Color Reset
        let prof_bg_rst = profile.clone();
        let canvas_bg_rst = canvas.clone();
        let insp_bg_rst = inspector.clone();
        let hw_bg_rst = hardware_manager.clone();
        let devs_bg_rst = connected_devices.clone();
        let act_bg_rst = active_device_idx.clone();
        let persist_bg_rst = persist_profiles.clone();

        inspector.btn_color_reset.connect_clicked(move |_| {
            let target = *insp_bg_rst.current_target.borrow();
            let st = insp_bg_rst.selected_state_idx.get();
            {
                let mut prof = prof_bg_rst.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| app.bg_color = None);
                }
            }
            persist_bg_rst();
            insp_bg_rst.is_updating.set(true);
            insp_bg_rst.btn_color_picker.set_rgba(&RGBA::new(0.098, 0.098, 0.125, 1.0));
            insp_bg_rst.is_updating.set(false);

            let prof = prof_bg_rst.borrow();
            canvas_bg_rst.render_profile_page(&prof);
            drop(prof);
            Self::sync_hardware(&devs_bg_rst, *act_bg_rst.borrow(), &hw_bg_rst, &prof_bg_rst.borrow());
        });

        // 6. Text Color Picker
        let prof_fg = profile.clone();
        let canvas_fg = canvas.clone();
        let insp_target_fg = inspector.current_target.clone();
        let hw_fg = hardware_manager.clone();
        let devs_fg = connected_devices.clone();
        let act_fg = active_device_idx.clone();
        let insp_updating_fg = inspector.is_updating.clone();
        let insp_fg_state = inspector.clone();
        let persist_fg = persist_profiles.clone();

        inspector.text_color_picker.connect_rgba_notify(move |cp| {
            if insp_updating_fg.get() {
                return;
            }
            let target = *insp_target_fg.borrow();
            let st = insp_fg_state.selected_state_idx.get();
            let hex = rgba_to_hex(&cp.rgba());
            {
                let mut prof = prof_fg.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| app.text_color = hex);
                }
            }
            persist_fg();
            let prof = prof_fg.borrow();
            canvas_fg.render_profile_page(&prof);
            drop(prof);
            Self::sync_hardware(&devs_fg, *act_fg.borrow(), &hw_fg, &prof_fg.borrow());
        });

        // 7. Browse Icon File Dialog (Supporting PNG, SVG, JPG, GIF, WebP)
        let prof_icon = profile.clone();
        let canvas_icon = canvas.clone();
        let insp_icon = inspector.clone();
        let hw_icon = hardware_manager.clone();
        let devs_icon = connected_devices.clone();
        let act_icon = active_device_idx.clone();
        let win_icon = window.clone();
        let persist_icon = persist_profiles.clone();

        inspector.browse_icon_btn.connect_clicked(move |_| {
            let target = *insp_icon.current_target.borrow();
            let dialog_title = match target {
                InspectorTarget::Touchscreen => "Choose Touch Strip Wallpaper",
                InspectorTarget::Dial(d) => &format!("Choose Dial {} Card Icon", d + 1),
                InspectorTarget::Key(k) => &format!("Choose Key {} Icon", k + 1),
            };
            let dialog = gtk::FileDialog::builder().title(dialog_title).build();

            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Image & Animation Files (*.png, *.svg, *.jpg, *.gif, *.webp)"));
            filter.add_mime_type("image/png");
            filter.add_mime_type("image/svg+xml");
            filter.add_mime_type("image/jpeg");
            filter.add_mime_type("image/gif");
            filter.add_mime_type("image/webp");
            filter.add_pattern("*.png");
            filter.add_pattern("*.svg");
            filter.add_pattern("*.jpg");
            filter.add_pattern("*.jpeg");
            filter.add_pattern("*.gif");
            filter.add_pattern("*.webp");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));

            let prof_c = prof_icon.clone();
            let can_c = canvas_icon.clone();
            let insp_c = insp_icon.clone();
            let hw_c = hw_icon.clone();
            let devs_c = devs_icon.clone();
            let act_c = act_icon.clone();
            let persist_c = persist_icon.clone();

            dialog.open(Some(&win_icon), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let raw_str = path.to_string_lossy().to_string();
                        let path_str = resolve_possible_ellipsis_path(&raw_str);
                        let st = insp_c.selected_state_idx.get();
                        {
                            let mut p = prof_c.borrow_mut();
                            if let Some(page) = p.current_page_mut() {
                                mutate_target_appearance(page, target, st, |app| app.icon_path = Some(path_str.clone()));
                            }
                        }
                        persist_c();
                        insp_c.mark_assigned();
                        let p = prof_c.borrow();
                        let act_opt = p.current_page().and_then(|pg| match target {
                            InspectorTarget::Key(k) => pg.keys.get(&k),
                            InspectorTarget::Dial(d) => pg.dial_actions.get(&d),
                            InspectorTarget::Touchscreen => pg.touchscreen_action.as_ref(),
                        });
                        insp_c.set_key_action(act_opt);
                        can_c.render_profile_page(&p);
                        drop(p);
                        Self::sync_hardware(&devs_c, *act_c.borrow(), &hw_c, &prof_c.borrow());
                    }
                }
            });
        });

        // 8. Clear / Restore Default Icon
        let prof_rst_icon = profile.clone();
        let canvas_rst_icon = canvas.clone();
        let insp_rst_icon = inspector.clone();
        let hw_rst_icon = hardware_manager.clone();
        let devs_rst_icon = connected_devices.clone();
        let act_rst_icon = active_device_idx.clone();
        let persist_rst_icon = persist_profiles.clone();

        inspector.clear_icon_btn.connect_clicked(move |_| {
            let target = *insp_rst_icon.current_target.borrow();
            let st = insp_rst_icon.selected_state_idx.get();
            {
                let mut p = prof_rst_icon.borrow_mut();
                if let Some(page) = p.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| app.icon_path = None);
                }
            }
            persist_rst_icon();
            insp_rst_icon.preview_icon.set_visible(false);
            let p = prof_rst_icon.borrow();
            canvas_rst_icon.render_profile_page(&p);
            drop(p);
            Self::sync_hardware(&devs_rst_icon, *act_rst_icon.borrow(), &hw_rst_icon, &prof_rst_icon.borrow());
        });

        // 9. Icon Scale Stepper (20% to 100%)
        let prof_scale = profile.clone();
        let canvas_scale = canvas.clone();
        let insp_scale = inspector.clone();
        let hw_scale = hardware_manager.clone();
        let devs_scale = connected_devices.clone();
        let act_scale = active_device_idx.clone();
        let persist_scale = persist_profiles.clone();

        inspector.icon_scale_spin.connect_value_changed(move |spin| {
            if insp_scale.is_updating.get() {
                return;
            }
            let target = *insp_scale.current_target.borrow();
            let st = insp_scale.selected_state_idx.get();
            let val = spin.value() as u32;
            {
                let mut prof = prof_scale.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    mutate_target_appearance(page, target, st, |app| app.icon_scale = val);
                }
            }
            persist_scale();

            let prof = prof_scale.borrow();
            let act_opt = prof.current_page().and_then(|pg| match target {
                InspectorTarget::Key(k) => pg.keys.get(&k),
                InspectorTarget::Dial(d) => pg.dial_actions.get(&d),
                InspectorTarget::Touchscreen => pg.touchscreen_action.as_ref(),
            });
            insp_scale.set_key_action(act_opt);
            canvas_scale.render_profile_page(&prof);
            drop(prof);
            Self::sync_hardware(&devs_scale, *act_scale.borrow(), &hw_scale, &prof_scale.borrow());
        });

        // 9. Position Buttons (Top, Center, Bottom)
        let connect_pos = |btn: &gtk::Button,
                           pos: TextVerticalAlign,
                           prof: Rc<RefCell<Profile>>,
                           can: Rc<Canvas>,
                           target_ref: Rc<RefCell<InspectorTarget>>,
                           insp: Rc<PropertyInspector>,
                           hw: Rc<RefCell<HardwareManager>>,
                           devs: Rc<RefCell<Vec<ConnectedDevice>>>,
                           act: Rc<RefCell<usize>>,
                           persist: Rc<dyn Fn()>| {
            btn.connect_clicked(move |_| {
                let target = *target_ref.borrow();
                let st = insp.selected_state_idx.get();
                {
                    let mut p = prof.borrow_mut();
                    if let Some(page) = p.current_page_mut() {
                        mutate_target_appearance(page, target, st, |app| {
                            app.vertical_align = pos;
                            if app.has_plugin_labels() {
                                let promoted = app.combined_display_summary();
                                if !promoted.is_empty() && app.title.is_empty() {
                                    app.title = promoted;
                                }
                                app.top_label = None;
                                app.center_label = None;
                                app.bottom_label = None;
                            }
                        });
                    }
                }
                persist();
                let p = prof.borrow();
                can.render_profile_page(&p);

                insp.pos_top_btn.remove_css_class("suggested-action");
                insp.pos_mid_btn.remove_css_class("suggested-action");
                insp.pos_bot_btn.remove_css_class("suggested-action");
                match pos {
                    TextVerticalAlign::Top => {
                        insp.pos_top_btn.add_css_class("suggested-action");
                        insp.preview_inner_label.set_valign(gtk::Align::Start);
                    }
                    TextVerticalAlign::Center => {
                        insp.pos_mid_btn.add_css_class("suggested-action");
                        insp.preview_inner_label.set_valign(gtk::Align::Center);
                    }
                    TextVerticalAlign::Bottom => {
                        insp.pos_bot_btn.add_css_class("suggested-action");
                        insp.preview_inner_label.set_valign(gtk::Align::End);
                    }
                }
                drop(p);
                Self::sync_hardware(&devs, *act.borrow(), &hw, &prof.borrow());
            });
        };

        connect_pos(&inspector.pos_top_btn, TextVerticalAlign::Top, profile.clone(), canvas.clone(), inspector.current_target.clone(), inspector.clone(), hardware_manager.clone(), connected_devices.clone(), active_device_idx.clone(), persist_profiles.clone());
        connect_pos(&inspector.pos_mid_btn, TextVerticalAlign::Center, profile.clone(), canvas.clone(), inspector.current_target.clone(), inspector.clone(), hardware_manager.clone(), connected_devices.clone(), active_device_idx.clone(), persist_profiles.clone());
        connect_pos(&inspector.pos_bot_btn, TextVerticalAlign::Bottom, profile.clone(), canvas.clone(), inspector.current_target.clone(), inspector.clone(), hardware_manager.clone(), connected_devices.clone(), active_device_idx.clone(), persist_profiles.clone());

        // 10. Multi-State Selector Radio Buttons (State 0 / State 1)
        let prof_state = profile.clone();
        let canvas_state = canvas.clone();
        let insp_state = inspector.clone();
        let hw_state = hardware_manager.clone();
        let devs_state = connected_devices.clone();
        let act_state = active_device_idx.clone();
        let persist_state = persist_profiles.clone();

        let handle_state_toggle = move |target_state: usize| {
            if insp_state.is_updating.get() {
                return;
            }
            let target = *insp_state.current_target.borrow();
            insp_state.selected_state_idx.set(target_state);

            {
                let mut prof = prof_state.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.set_active_state(target_state);
                }
            }
            persist_state();

            let prof = prof_state.borrow();
            canvas_state.render_profile_page(&prof);
            if let Some(page) = prof.current_page() {
                let act = match target {
                    InspectorTarget::Key(k) => page.keys.get(&k),
                    InspectorTarget::Dial(d) => page.dial_actions.get(&d),
                    InspectorTarget::Touchscreen => page.touchscreen_action.as_ref(),
                };
                insp_state.set_key_action(act);
            }
            drop(prof);
            Self::sync_hardware(&devs_state, *act_state.borrow(), &hw_state, &prof_state.borrow());
        };

        let h0 = handle_state_toggle.clone();
        inspector.radio_state_0.connect_toggled(move |btn| {
            if btn.is_active() {
                h0(0);
            }
        });

        let h1 = handle_state_toggle;
        inspector.radio_state_1.connect_toggled(move |btn| {
            if btn.is_active() {
                h1(1);
            }
        });

        // App Dropdown Selection
        let prof_app_dd = profile.clone();
        let canvas_app_dd = canvas.clone();
        let insp_app_dd = inspector.clone();
        let hw_app = hardware_manager.clone();
        let devs_app = connected_devices.clone();
        let act_app = active_device_idx.clone();
        let pm_app_dd = plugin_manager.clone();
        let persist_app_dd = persist_profiles.clone();

        inspector.app_dropdown.connect_selected_notify(move |dd| {
            if insp_app_dd.is_updating.get() {
                return;
            }
            let sel_idx = dd.selected() as usize;
            let target = *insp_app_dd.current_target.borrow();

            // Guard: Only update if the target action is explicitly the built-in App Launcher action!
            {
                let prof = prof_app_dd.borrow();
                let current_act = get_target_ref(&prof, target);
                let is_builtin_app_launcher =
                    current_act.map_or(false, |a| a.action_id == "app_launcher.launch");
                if !is_builtin_app_launcher {
                    return;
                }
            }

            let apps = insp_app_dd.installed_apps.borrow();
            if let Some(app) = apps.get(sel_idx) {
                let app_name = app.name.clone();
                let app_icon = app.icon.clone();
                let resolved_icon = crate::ui::canvas::resolve_icon_path(&app_icon, 128);

                let settings_json = {
                    let mut prof = prof_app_dd.borrow_mut();
                    if let Some(page) = prof.current_page_mut() {
                        let action = get_target_mut(page, target);
                        action.parameters.insert("desktop_id".into(), app.desktop_id.clone());
                        action.parameters.insert("app_name".into(), app.name.clone());
                        action.parameters.insert("icon_name".into(), app.icon.clone());
                        if let Some(ref rip) = resolved_icon {
                            action.parameters.insert("icon_path".into(), rip.clone());
                        } else {
                            action.parameters.remove("icon_path");
                        }

                        // Update action.appearance to the chosen app's title and icon
                        action.appearance.title = app_name.clone();
                        action.appearance.icon_path = resolved_icon.clone();

                        // Synchronize all states so active_appearance() immediately displays the app icon & title
                        for state in action.states.iter_mut() {
                            state.title = app_name.clone();
                            state.icon_path = resolved_icon.clone();
                        }
                        if action.states.is_empty() {
                            action.states.push(action.appearance.clone());
                        }
                        action.merged_settings_json()
                    } else {
                        HashMap::new()
                    }
                };
                persist_app_dd();

                // Notify bridge if relevant
                let dkey = active_device_key_for(&devs_app, &act_app, prof_app_dd.borrow().device_kind);
                pm_app_dd.borrow().bridge.update_action_settings(
                    &scoped_bridge_slot(&dkey, &format_target_slot(target)),
                    settings_json,
                );

                insp_app_dd.is_updating.set(true);
                insp_app_dd.title_entry.set_text(&app_name);
                insp_app_dd.is_updating.set(false);

                let prof = prof_app_dd.borrow();
                canvas_app_dd.render_profile_page(&prof);
                insp_app_dd.set_key_action(get_target_ref(&prof, target));
                drop(prof);
                Self::sync_hardware(&devs_app, *act_app.borrow(), &hw_app, &prof_app_dd.borrow());
            }
        });

        // App Arguments Entry Changed
        let prof_args = profile.clone();
        let insp_args = inspector.clone();
        let persist_args = persist_profiles.clone();
        inspector.args_entry.connect_changed(move |entry| {
            if insp_args.is_updating.get() {
                return;
            }
            let target = *insp_args.current_target.borrow();
            {
                let mut prof = prof_args.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.parameters.insert("args".into(), entry.text().to_string());
                }
            }
            persist_args();
        });

        // Touch Strip Swipe Dropdown Selection
        let prof_swipe_dd = profile.clone();
        let insp_swipe_dd = inspector.clone();
        let persist_swipe_dd = persist_profiles.clone();
        inspector.swipe_dropdown.connect_selected_notify(move |dd| {
            if insp_swipe_dd.is_updating.get() {
                return;
            }
            let sel = dd.selected();
            {
                let mut prof = prof_swipe_dd.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = page.get_or_create_touchscreen();
                    if sel == 1 {
                        action.action_id = "disabled".into();
                    } else {
                        action.action_id = "switch_pages".into();
                    }
                }
            }
            persist_swipe_dd();
        });

        // Delete (Clear Action) Button
        let prof_del_key = profile.clone();
        let canvas_del_key = canvas.clone();
        let insp_del_key = inspector.clone();
        let hw_del = hardware_manager.clone();
        let devs_del = connected_devices.clone();
        let act_del = active_device_idx.clone();
        let pm_del_key = plugin_manager.clone();
        let nh_del_key = native_host.clone();
        let persist_del = persist_profiles.clone();

        inspector.delete_button.connect_clicked(move |_| {
            let target = *insp_del_key.current_target.borrow();
            let dkey = active_device_key_for(&devs_del, &act_del, prof_del_key.borrow().device_kind);
            let slot = format_target_slot(target);
            let scoped_slot = scoped_bridge_slot(&dkey, &slot);
            nh_del_key.destroy_slot(&dkey, &slot);
            {
                let pm = pm_del_key.borrow();
                pm.bridge.destroy_action(&scoped_slot);
                pm.clear_live_config_rows(&scoped_slot);
            }
            {
                let mut prof = prof_del_key.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    match target {
                        InspectorTarget::Key(k) => { page.keys.remove(&k); },
                        InspectorTarget::Dial(d) => { page.dial_actions.remove(&d); },
                        InspectorTarget::Touchscreen => { page.touchscreen_action = None; },
                    }
                }
            }
            persist_del();
            let prof = prof_del_key.borrow();
            canvas_del_key.render_profile_page(&prof);
            insp_del_key.set_key_action(None);
            drop(prof);
            Self::sync_hardware(&devs_del, *act_del.borrow(), &hw_del, &prof_del_key.borrow());
        });

        // Populate Dynamic Sidebar using unified helper
        refresh_sidebar();

        // Wire Drag and Drop from Sidebar onto Canvas Keys, Dials, and Touchscreen
        let assign_drop = assign_action.clone();
        canvas.set_action_dropped_callback(move |target, sel| {
            assign_drop(target, sel, true);
        });

        // Wire Canvas Drag-and-Drop Move / Swap between Buttons & Dials
        {
            let prof_mv = profile.clone();
            let canvas_mv = canvas.clone();
            let insp_mv = inspector.clone();
            let hw_mv = hardware_manager.clone();
            let devs_mv = connected_devices.clone();
            let act_mv = active_device_idx.clone();
            let pm_mv = plugin_manager.clone();
            let nh_mv = native_host.clone();
            let disp_mv = dispatch_native.clone();
            let persist_mv = persist_profiles.clone();

            canvas.set_slot_moved_callback(move |from, to| {
                if from == to
                    || matches!(from, InspectorTarget::Touchscreen)
                    || matches!(to, InspectorTarget::Touchscreen)
                {
                    return;
                }

                let from_slot = format_target_slot(from);
                let to_slot = format_target_slot(to);

                let from_surface = match from {
                    InspectorTarget::Key(_) => crate::native_sdk::SlotSurface::Key,
                    InspectorTarget::Dial(_) => crate::native_sdk::SlotSurface::Dial,
                    InspectorTarget::Touchscreen => crate::native_sdk::SlotSurface::Touchscreen,
                };
                let to_surface = match to {
                    InspectorTarget::Key(_) => crate::native_sdk::SlotSurface::Key,
                    InspectorTarget::Dial(_) => crate::native_sdk::SlotSurface::Dial,
                    InspectorTarget::Touchscreen => crate::native_sdk::SlotSurface::Touchscreen,
                };

                // 1. Mutate profile page cleanly (Move if destination empty, Swap if occupied) with TargetCaps check
                let (moved_to_action, swapped_from_action) = {
                    let mut prof = prof_mv.borrow_mut();
                    let Some(page) = prof.current_page_mut() else {
                        return;
                    };
                    let src_caps = match from {
                        InspectorTarget::Key(k) => page.keys.get(&k),
                        InspectorTarget::Dial(d) => page.dial_actions.get(&d),
                        InspectorTarget::Touchscreen => None,
                    }
                    .map(|a| nh_mv.action_caps(&a.plugin_id, &a.action_id));
                    if let Some(sc) = src_caps {
                        if !sc.supports(to_surface) {
                            return;
                        }
                    } else {
                        return;
                    }
                    let dst_caps = match to {
                        InspectorTarget::Key(k) => page.keys.get(&k),
                        InspectorTarget::Dial(d) => page.dial_actions.get(&d),
                        InspectorTarget::Touchscreen => None,
                    }
                    .map(|a| nh_mv.action_caps(&a.plugin_id, &a.action_id));
                    if let Some(dc) = dst_caps {
                        if !dc.supports(from_surface) {
                            return;
                        }
                    }

                    let src_act = match from {
                        InspectorTarget::Key(k) => page.keys.remove(&k),
                        InspectorTarget::Dial(d) => page.dial_actions.remove(&d),
                        InspectorTarget::Touchscreen => None,
                    };
                    let Some(src_act) = src_act else {
                        return;
                    };
                    let dst_act = match to {
                        InspectorTarget::Key(k) => page.keys.remove(&k),
                        InspectorTarget::Dial(d) => page.dial_actions.remove(&d),
                        InspectorTarget::Touchscreen => None,
                    };

                    let moved_clone = src_act.clone();
                    match to {
                        InspectorTarget::Key(k) => {
                            page.keys.insert(k, src_act);
                        }
                        InspectorTarget::Dial(d) => {
                            page.dial_actions.insert(d, src_act);
                        }
                        InspectorTarget::Touchscreen => {}
                    }

                    let swapped_clone = dst_act.clone();
                    if let Some(swapped) = dst_act {
                        match from {
                            InspectorTarget::Key(k) => {
                                page.keys.insert(k, swapped);
                            }
                            InspectorTarget::Dial(d) => {
                                page.dial_actions.insert(d, swapped);
                            }
                            InspectorTarget::Touchscreen => {}
                        }
                    }
                    (moved_clone, swapped_clone)
                };

                // 2. Granular runtime teardown ONLY on the 2 affected slots (zero disruption to other keys or decks!)
                let dkey = active_device_key_for(&devs_mv, &act_mv, prof_mv.borrow().device_kind);
                let from_scoped = scoped_bridge_slot(&dkey, &from_slot);
                let to_scoped = scoped_bridge_slot(&dkey, &to_slot);
                nh_mv.destroy_slot(&dkey, &from_slot);
                nh_mv.destroy_slot(&dkey, &to_slot);
                {
                    let pm = pm_mv.borrow();
                    pm.bridge.destroy_action(&from_scoped);
                    pm.clear_live_config_rows(&from_scoped);
                    pm.bridge.destroy_action(&to_scoped);
                    pm.clear_live_config_rows(&to_scoped);
                }

                // 3. Rebind native plugin slots (diff-based) + external StreamController slots if applicable
                let emitted = {
                    let prof = prof_mv.borrow();
                    prof.current_page()
                        .map(|pg| nh_mv.sync_page(&dkey, pg))
                        .unwrap_or_default()
                };
                for (s, cmds) in emitted {
                    let upd = nh_mv.get_slot_settings(&dkey, &s);
                    disp_mv(dkey.clone(), s, cmds, upd);
                }

                let rebind_external = |slot: &str, act: &KeyAction| {
                    if !act.plugin_id.is_empty()
                        && act.plugin_id != "system"
                        && !nh_mv.is_native_plugin(&act.plugin_id)
                    {
                        pm_mv.borrow().bridge.create_action(
                            &scoped_bridge_slot(&dkey, slot),
                            &act.plugin_id,
                            &act.action_id,
                            act.merged_settings_json(),
                        );
                    }
                };
                rebind_external(&to_slot, &moved_to_action);
                if let Some(ref swapped) = swapped_from_action {
                    rebind_external(&from_slot, swapped);
                }

                // 4. Follow selection to the destination slot, refresh UI, persist, and sync hardware
                insp_mv.set_target(to);
                match to {
                    InspectorTarget::Key(k) => canvas_mv.select_key(k),
                    InspectorTarget::Dial(d) => canvas_mv.select_dial_and_touch(d),
                    InspectorTarget::Touchscreen => canvas_mv.select_touchscreen_panoramic(),
                }

                let prof = prof_mv.borrow();
                canvas_mv.render_profile_page(&prof);
                insp_mv.set_key_action(get_target_ref(&prof, to));
                drop(prof);
                persist_mv();
                Self::sync_hardware(&devs_mv, *act_mv.borrow(), &hw_mv, &prof_mv.borrow());
            });
        }

        Self {
            window,
            header,
            canvas,
            inspector,
            sidebar,
            profile,
            profiles,
            active_profile_id,
            plugin_manager,
            native_host,
            hardware_manager,
            connected_devices,
            active_device_idx,
            tray,
            power_monitor: power_monitor_cell,
        }
    }

    pub fn show_and_present(&self) {
        self.window.set_visible(true);
        self.window.present();
    }

    /// Syncs active profile page to physical hardware keys
    pub fn sync_hardware(
        devices: &Rc<RefCell<Vec<ConnectedDevice>>>,
        active_idx: usize,
        hw: &Rc<RefCell<HardwareManager>>,
        profile: &Profile,
    ) {
        let devs = devices.borrow();
        if let Some(dev) = devs.get(active_idx) {
            hw.borrow()
                .sync_hardware_page(&dev.dev_path, &dev.stable_key(), dev.kind, profile);
        }
    }

    /// Binds click events for key buttons, touchscreen segments, and dials
    pub fn bind_canvas_events(
        canvas: &Rc<Canvas>,
        inspector: &Rc<PropertyInspector>,
        profile: &Rc<RefCell<Profile>>,
        plugin_manager: &Rc<RefCell<PluginManager>>,
        native_host: &NativePluginHost,
        dispatch_native: &NativeCommandDispatcher,
        devices: &Rc<RefCell<Vec<ConnectedDevice>>>,
        active_device_idx: &Rc<RefCell<usize>>,
        hw: &Rc<RefCell<HardwareManager>>,
    ) {
        let keys = canvas.key_buttons.borrow();
        for (idx, btn) in keys.iter().enumerate() {
            let can_ref = canvas.clone();
            let insp_ref = inspector.clone();
            let prof_ref = profile.clone();
            let devs_ref = devices.clone();
            let act_dev_ref = active_device_idx.clone();
            let hw_ref = hw.clone();
            let pm_can = plugin_manager.clone();
            let nh_can = native_host.clone();
            let disp_can = dispatch_native.clone();

            // Dedicated GestureClick (Capture phase):
            // - Single-click selects the key cleanly for editing without accidentally firing actions
            // - Double-click fires the key action with a tactile press-in / spring-back animation
            let key_click = gtk::GestureClick::new();
            key_click.set_button(1);
            key_click.set_propagation_phase(gtk::PropagationPhase::Capture);
            key_click.connect_pressed(move |_gesture, n_press, _x, _y| {
                let prev_target = *insp_ref.current_target.borrow();
                let new_target = InspectorTarget::Key(idx);
                can_ref.select_key(idx);
                insp_ref.set_target(new_target);

                let mut toggled = false;
                if n_press == 2 {
                    // Double-click triggers the action + plays tactile push-in / spring-back animation
                    can_ref.pulse_key_press(idx, 135);

                    let dkey = active_device_key_for(&devs_ref, &act_dev_ref, prof_ref.borrow().device_kind);
                    let slot_str = format!("key_{idx}");
                    if nh_can.has_slot(&dkey, &slot_str) {
                        let (cmds, updated) = nh_can.handle_key_event(&dkey, &slot_str, true);
                        disp_can(dkey.clone(), slot_str.clone(), cmds, updated);
                        let nh_up = nh_can.clone();
                        let disp_up = disp_can.clone();
                        let dkey_up = dkey.clone();
                        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                            let (c2, u2) = nh_up.handle_key_event(&dkey_up, &slot_str, false);
                            disp_up(dkey_up, slot_str, c2, u2);
                        });
                        return;
                    }

                    let mut is_plugin_action = false;
                    {
                        let mut prof = prof_ref.borrow_mut();
                        if let Some(page) = prof.current_page_mut() {
                            if let Some(act) = page.keys.get_mut(&idx) {
                                if !act.plugin_id.is_empty() && act.plugin_id != "system" {
                                    is_plugin_action = true;
                                } else {
                                    if act.states.len() > 1 {
                                        act.toggle_state();
                                        toggled = true;
                                    }
                                    if act.action_id == "app_launcher.launch" {
                                        if let Some(did) = act.parameters.get("desktop_id") {
                                            let _ = pm_can.borrow().launch_app_desktop(did);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if is_plugin_action {
                        let scoped = scoped_bridge_slot(&dkey, &slot_str);
                        pm_can.borrow().bridge.send_key_event(&scoped, true);
                        let pm_up = pm_can.clone();
                        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                            pm_up.borrow().bridge.send_key_event(&scoped, false);
                        });
                    }
                }

                let prof = prof_ref.borrow();
                if toggled {
                    can_ref.render_profile_page(&prof);
                    Self::sync_hardware(&devs_ref, *act_dev_ref.borrow(), &hw_ref, &prof);
                }
                if prev_target != new_target || toggled {
                    if let Some(page) = prof.current_page() {
                        let act = page.keys.get(&idx);
                        insp_ref.set_key_action(act);
                    }
                }
            });
            btn.add_controller(key_click);
        }

        // Rotary Dials Click:
        // "when the dial is clicked, it touchscreen area is selected"
        let can_dial = canvas.clone();
        let insp_dial = inspector.clone();
        let prof_dial = profile.clone();
        canvas.set_dial_click_callback(move |dial_idx| {
            can_dial.select_dial_and_touch(dial_idx);
            insp_dial.set_target(InspectorTarget::Dial(dial_idx));
            let prof = prof_dial.borrow();
            if let Some(page) = prof.current_page() {
                insp_dial.set_key_action(page.dial_actions.get(&dial_idx));
            }
        });

        // Rotary Dials Double-Click: tactile circular press pulse + native or bridge press/release
        let can_dial_press = canvas.clone();
        let pm_dial_press = plugin_manager.clone();
        let nh_dial_press = native_host.clone();
        let disp_dial_press = dispatch_native.clone();
        let devs_dial_press = devices.clone();
        let act_dial_press = active_device_idx.clone();
        let prof_dial_press = profile.clone();
        canvas.set_dial_press_callback(move |dial_idx| {
            can_dial_press.pulse_dial_press(dial_idx, 135);
            let dkey = active_device_key_for(&devs_dial_press, &act_dial_press, prof_dial_press.borrow().device_kind);
            let slot_str = format!("dial_{dial_idx}");
            if nh_dial_press.has_slot(&dkey, &slot_str) {
                let (cmds, updated) = nh_dial_press.handle_dial_press(&dkey, &slot_str, true);
                disp_dial_press(dkey.clone(), slot_str.clone(), cmds, updated);
                let nh_up = nh_dial_press.clone();
                let disp_up = disp_dial_press.clone();
                let dkey_up = dkey.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                    let (c2, u2) = nh_up.handle_dial_press(&dkey_up, &slot_str, false);
                    disp_up(dkey_up, slot_str, c2, u2);
                });
            } else {
                let scoped = scoped_bridge_slot(&dkey, &slot_str);
                pm_dial_press.borrow().bridge.send_dial_event(&scoped, "press", 0);
                let pm_up = pm_dial_press.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
                    pm_up.borrow().bridge.send_dial_event(&scoped, "release", 0);
                });
            }
        });

        // Touchscreen Capsule Click:
        // "when the touchscreen is clicks, no matter where, the whole screen is selected as one whole button"
        let can_capsule = canvas.clone();
        let insp_capsule = inspector.clone();
        let prof_capsule = profile.clone();
        canvas.set_touch_capsule_click_callback(move || {
            can_capsule.select_touchscreen_panoramic();
            insp_capsule.set_target(InspectorTarget::Touchscreen);
            let prof = prof_capsule.borrow();
            if let Some(page) = prof.current_page() {
                insp_capsule.set_key_action(page.touchscreen_action.as_ref());
            }
        });

        if let Some(ref touch_overlay) = *canvas.touch_overlay.borrow() {
            let swipe_gesture = gtk::GestureSwipe::new();
            let prof_swipe = profile.clone();
            let can_swipe = canvas.clone();
            let insp_swipe = inspector.clone();
            let pm_swipe = plugin_manager.clone();
            let nh_swipe = native_host.clone();
            let hw_swipe = hw.clone();
            let devs_swipe = devices.clone();
            let act_swipe = active_device_idx.clone();
            swipe_gesture.connect_swipe(move |_g, vx, _vy| {
                let is_swipe_active = {
                    let p = prof_swipe.borrow();
                    p.current_page()
                        .and_then(|pg| pg.touchscreen_action.as_ref())
                        .map(|a| a.action_id != "disabled")
                        .unwrap_or(true)
                };
                if !is_swipe_active {
                    return;
                }

                let mut p = prof_swipe.borrow_mut();
                if vx < -50.0 {
                    // Swipe left -> Next Page
                    p.next_page();
                } else if vx > 50.0 {
                    // Swipe right -> Previous Page
                    p.prev_page();
                } else {
                    return;
                }
                let p_imm = p.clone();
                drop(p);
                let dkey = active_device_key_for(&devs_swipe, &act_swipe, p_imm.device_kind);
                let cur_b = WindowState::load().brightness_for(&dkey);
                sync_device_runtime(
                    &dkey,
                    &p_imm,
                    vec![p_imm.name.clone()],
                    cur_b,
                    &pm_swipe.borrow(),
                    &nh_swipe,
                );
                can_swipe.render_profile_page(&p_imm);
                let cur_target = *insp_swipe.current_target.borrow();
                insp_swipe.set_key_action(get_target_ref(&p_imm, cur_target));
                Self::sync_hardware(&devs_swipe, *act_swipe.borrow(), &hw_swipe, &p_imm);
            });
            touch_overlay.add_controller(swipe_gesture);
        }

        let pm_rot = plugin_manager.clone();
        let nh_rot = native_host.clone();
        let disp_rot = dispatch_native.clone();
        let devs_rot = devices.clone();
        let act_rot = active_device_idx.clone();
        let prof_rot = profile.clone();
        canvas.set_dial_rotate_callback(move |dial_idx, delta| {
            tracing::info!("Software Dial Rotate: dial {}, delta {}", dial_idx, delta);
            let dkey = active_device_key_for(&devs_rot, &act_rot, prof_rot.borrow().device_kind);
            let slot_str = format!("dial_{dial_idx}");
            if nh_rot.has_slot(&dkey, &slot_str) {
                let (cmds, updated) = nh_rot.handle_dial_rotate(&dkey, &slot_str, delta);
                disp_rot(dkey, slot_str, cmds, updated);
            } else {
                pm_rot
                    .borrow()
                    .bridge
                    .send_dial_event(&scoped_bridge_slot(&dkey, &slot_str), "turn", delta);
            }
        });
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn get_target_ref(profile: &Profile, target: InspectorTarget) -> Option<&KeyAction> {
    let page = profile.current_page()?;
    match target {
        InspectorTarget::Key(k) => page.keys.get(&k),
        InspectorTarget::Dial(d) => page.dial_actions.get(&d),
        InspectorTarget::Touchscreen => page.touchscreen_action.as_ref(),
    }
}

fn parse_bridge_color(color: &serde_json::Value) -> Option<String> {
    if let Some(arr) = color.as_array() {
        if arr.len() >= 3 {
            let r = arr[0].as_u64().unwrap_or(0).min(255) as u8;
            let g = arr[1].as_u64().unwrap_or(0).min(255) as u8;
            let b = arr[2].as_u64().unwrap_or(0).min(255) as u8;
            return Some(format!("#{:02X}{:02X}{:02X}", r, g, b));
        }
    } else if let Some(s) = color.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn active_device_key_for(
    devices: &Rc<RefCell<Vec<ConnectedDevice>>>,
    active_idx: &Rc<RefCell<usize>>,
    fallback_kind: DeviceKind,
) -> String {
    devices
        .borrow()
        .get(*active_idx.borrow())
        .map(|d| d.stable_key())
        .unwrap_or_else(|| format!("{fallback_kind:?}"))
}

fn find_profile_idx_for_device(
    profiles: &[Profile],
    active_by_device: &HashMap<String, String>,
    device_key: &str,
    dev_kind: DeviceKind,
) -> Option<usize> {
    active_by_device
        .get(device_key)
        .and_then(|pid| {
            profiles
                .iter()
                .position(|p| p.id == *pid && p.device_kind == dev_kind)
        })
        .or_else(|| profiles.iter().position(|p| p.device_kind == dev_kind))
}

fn scoped_bridge_slot(device_key: &str, slot: &str) -> String {
    format!("{device_key}::{slot}")
}

fn split_bridge_slot(raw: &str) -> (Option<&str>, &str) {
    if let Some((dkey, base)) = raw.split_once("::") {
        (Some(dkey), base)
    } else {
        (None, raw)
    }
}

fn sync_device_runtime(
    device_key: &str,
    profile: &Profile,
    available_profiles: Vec<String>,
    brightness: u8,
    pm: &PluginManager,
    native_host: &NativePluginHost,
) {
    native_host.set_deck_state(
        device_key,
        available_profiles,
        profile.name.clone(),
        profile.current_page_idx,
        profile.pages.len(),
        brightness,
    );
    pm.bridge.clear_device_actions(device_key);
    pm.clear_device_live_config_rows(device_key);
    if let Some(page) = profile.current_page() {
        let _ = native_host.sync_page(device_key, page);
        for (k, act) in &page.keys {
            if !act.plugin_id.is_empty()
                && act.plugin_id != "system"
                && !native_host.is_native_plugin(&act.plugin_id)
            {
                pm.bridge.create_action(
                    &scoped_bridge_slot(device_key, &format!("key_{k}")),
                    &act.plugin_id,
                    &act.action_id,
                    act.merged_settings_json(),
                );
            }
        }
        for (d, act) in &page.dial_actions {
            if !act.plugin_id.is_empty()
                && act.plugin_id != "system"
                && !native_host.is_native_plugin(&act.plugin_id)
            {
                pm.bridge.create_action(
                    &scoped_bridge_slot(device_key, &format!("dial_{d}")),
                    &act.plugin_id,
                    &act.action_id,
                    act.merged_settings_json(),
                );
            }
        }
        if let Some(ref act) = page.touchscreen_action {
            if !act.plugin_id.is_empty()
                && act.plugin_id != "system"
                && !native_host.is_native_plugin(&act.plugin_id)
            {
                pm.bridge.create_action(
                    &scoped_bridge_slot(device_key, "touchscreen"),
                    &act.plugin_id,
                    &act.action_id,
                    act.merged_settings_json(),
                );
            }
        }
    }
}

fn get_target_mut(page: &mut crate::models::Page, target: InspectorTarget) -> &mut KeyAction {
    match target {
        InspectorTarget::Key(k) => page.get_or_create_key(k),
        InspectorTarget::Dial(d) => page.get_or_create_dial(d),
        InspectorTarget::Touchscreen => page.get_or_create_touchscreen(),
    }
}

fn mutate_target_appearance<F>(page: &mut crate::models::Page, target: InspectorTarget, state_idx: usize, f: F)
where
    F: FnOnce(&mut KeyAppearance),
{
    let action = get_target_mut(page, target);
    let app = action.state_appearance_mut(state_idx);
    f(app);
    action.appearance = action.active_appearance().clone();
}

pub fn parse_slot_target(slot: &str) -> InspectorTarget {
    if let Some(rest) = slot.strip_prefix("key_") {
        if let Ok(idx) = rest.parse::<usize>() {
            return InspectorTarget::Key(idx);
        }
    } else if let Some(rest) = slot.strip_prefix("dial_") {
        if let Ok(idx) = rest.parse::<usize>() {
            return InspectorTarget::Dial(idx);
        }
    }
    InspectorTarget::Touchscreen
}

pub fn format_target_slot(target: InspectorTarget) -> String {
    match target {
        InspectorTarget::Key(k) => format!("key_{k}"),
        InspectorTarget::Dial(d) => format!("dial_{d}"),
        InspectorTarget::Touchscreen => "touchscreen".into(),
    }
}

pub fn resolve_possible_ellipsis_path(path_str: &str) -> String {
    if std::path::Path::new(path_str).exists() {
        return path_str.to_string();
    }
    if !path_str.contains("...") && !path_str.contains('…') {
        return path_str.to_string();
    }
    let p = std::path::Path::new(path_str);
    if let (Some(parent), Some(file_name)) = (p.parent(), p.file_name()) {
        let name = file_name.to_string_lossy();
        let sep = if name.contains("...") { "..." } else { "…" };
        let parts: Vec<&str> = name.split(sep).collect();
        if parts.len() == 2 {
            let prefix = parts[0].trim();
            let suffix = parts[1].trim();
            let match_prefix = if prefix.len() > 10 { &prefix[..10] } else { prefix };
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let cand_name = entry.file_name().to_string_lossy().to_string();
                    if cand_name.starts_with(match_prefix) && cand_name.ends_with(suffix) {
                        return entry.path().to_string_lossy().to_string();
                    }
                }
            }
        }
    }
    path_str.to_string()
}
