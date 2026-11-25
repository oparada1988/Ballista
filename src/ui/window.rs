use super::canvas::Canvas;
use super::header::Header;
use super::inspector::{rgba_to_hex, InspectorTarget, PropertyInspector};
use super::settings::create_settings_window;
use super::sidebar::Sidebar;
use super::store::create_store_window;
use crate::core::{ConnectedDevice, HardwareEvent, HardwareManager, PluginManager};
use crate::models::{DeviceKind, KeyAction, Profile, TextVerticalAlign, WindowState};
use adw::prelude::*;
use adw::{ApplicationWindow, ToolbarView};
use gtk::gdk::RGBA;
use gtk::{Orientation, Paned};
use std::cell::RefCell;
use std::rc::Rc;

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
    pub hardware_manager: Rc<RefCell<HardwareManager>>,
    pub connected_devices: Rc<RefCell<Vec<ConnectedDevice>>>,
    pub active_device_idx: Rc<RefCell<usize>>,
}

impl MainWindow {
    pub fn new(app: &adw::Application) -> Self {
        // Load saved window & layout state
        let state = WindowState::load();

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Ballista - Elgato Controller")
            .default_width(state.window_width)
            .default_height(state.window_height)
            .build();

        window.add_css_class("ballista-main");

        let toolbar_view = ToolbarView::new();

        // 1. Data Models & Engines
        let initial_profiles = vec![
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckPlus),
            Profile::new_for_device("Streaming & Audio", DeviceKind::StreamDeckPlus),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckXL),
            Profile::new_for_device("Studio Master", DeviceKind::StreamDeckXL),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckMK2),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckMini),
        ];
        let profiles = Rc::new(RefCell::new(initial_profiles));
        let plugin_manager = Rc::new(RefCell::new(PluginManager::new()));
        let hardware_manager = Rc::new(RefCell::new(HardwareManager::new()));

        // Scan real hardware
        let devices = hardware_manager.borrow().scan_devices();
        let connected_devices = Rc::new(RefCell::new(devices));
        let active_device_idx = Rc::new(RefCell::new(0));

        let initial_kind = connected_devices
            .borrow()
            .first()
            .map(|d| d.kind)
            .unwrap_or(DeviceKind::StreamDeckMK2);

        let initial_p = profiles.borrow().iter().find(|p| p.device_kind == initial_kind).cloned()
            .unwrap_or_else(|| profiles.borrow()[0].clone());
        let active_profile_id = Rc::new(RefCell::new(initial_p.id.clone()));
        let profile = Rc::new(RefCell::new(initial_p));

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
        canvas.container.set_size_request(-1, 280);
        work_paned.set_start_child(Some(&canvas.container));
        work_paned.set_resize_start_child(true);
        work_paned.set_shrink_start_child(false);

        // 5. Property Inspector in Scrollable Container
        let inspector = Rc::new(PropertyInspector::new());
        let insp_scroll = gtk::ScrolledWindow::new();
        insp_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        insp_scroll.set_child(Some(&inspector.container));
        insp_scroll.set_min_content_height(160);
        insp_scroll.set_size_request(-1, 160);

        work_paned.set_end_child(Some(&insp_scroll));
        work_paned.set_resize_end_child(true);
        work_paned.set_shrink_end_child(false);
        work_paned.set_position(state.work_paned_position.max(280));

        // 6. Sidebar (Full Height)
        let sidebar = Rc::new(Sidebar::new());
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

        // Persist on Close Request
        let win_close = window.clone();
        let paned_close = paned.clone();
        let work_paned_close = work_paned.clone();
        window.connect_close_request(move |_| {
            let w = win_close.width().max(600);
            let h = win_close.height().max(400);
            let s = WindowState {
                window_width: w,
                window_height: h,
                sidebar_position: paned_close.position(),
                work_paned_position: work_paned_close.position(),
                ..WindowState::load()
            };
            s.save();
            glib::Propagation::Proceed
        });

        toolbar_view.set_content(Some(&paned));
        window.set_content(Some(&toolbar_view));

        // Populate Installed Apps into Inspector
        let apps = plugin_manager.borrow().installed_apps.clone();
        inspector.populate_installed_apps(&apps);

        // Initial Canvas Render & Physical Hardware Sync
        canvas.update_scale(state.work_paned_position);
        canvas.render_profile_page(&profile.borrow());
        canvas.select_key(0);
        *inspector.current_key_idx.borrow_mut() = Some(0);
        let prof_init = profile.borrow();
        inspector.set_key_action(prof_init.current_page().and_then(|p| p.keys.get(&0)));
        drop(prof_init);
        Self::sync_hardware(&connected_devices, 0, &hardware_manager, &profile.borrow());

        // Wire canvas button click handlers
        Self::bind_canvas_events(
            &canvas,
            &inspector,
            &profile,
            &plugin_manager,
            &connected_devices,
            &active_device_idx,
            &hardware_manager,
        );

        // ===================== Hardware Event Channel Setup =====================
        let (sender, receiver) = async_channel::unbounded::<HardwareEvent>();

        let canvas_hw = canvas.clone();
        let inspector_hw = inspector.clone();
        let profile_hw = profile.clone();
        let pm_hw = plugin_manager.clone();
        let devs_hw = connected_devices.clone();
        let act_hw = active_device_idx.clone();
        let hwm_hw = hardware_manager.clone();

        glib::spawn_future_local(async move {
            while let Ok(event) = receiver.recv().await {
                match event {
                    HardwareEvent::KeyDown { key_index } => {
                        tracing::info!("Physical Stream Deck Key Down: key {}", key_index);
                        canvas_hw.select_key(key_index);
                        *inspector_hw.current_key_idx.borrow_mut() = Some(key_index);

                        let prof = profile_hw.borrow();
                        if let Some(page) = prof.current_page() {
                            let act = page.keys.get(&key_index);
                            inspector_hw.set_key_action(act);

                            if let Some(action) = act {
                                if let Some(desktop_id) = action.parameters.get("desktop_id") {
                                    let _ = pm_hw.borrow().launch_app_desktop(desktop_id);
                                }
                            }
                        }
                    }
                    HardwareEvent::KeyUp { key_index } => {
                        tracing::info!("Physical Stream Deck Key Up: key {}", key_index);
                    }
                    HardwareEvent::DialRotate { dial_index, delta } => {
                        tracing::info!("Physical Stream Deck Dial Rotate: dial {}, delta {}", dial_index, delta);
                        canvas_hw.rotate_dial(dial_index, delta);
                    }
                    HardwareEvent::DialPress { dial_index } => {
                        tracing::info!("Physical Stream Deck Dial Press: dial {}", dial_index);
                    }
                    HardwareEvent::DialRelease { dial_index } => {
                        tracing::info!("Physical Stream Deck Dial Release: dial {}", dial_index);
                    }
                    HardwareEvent::TouchTap { slot, x, y } => {
                        tracing::info!("Physical Stream Deck Touch Tap: slot {}, x {}, y {}", slot, x, y);
                        inspector_hw.title_label.set_text(&format!("Stream Deck + Touch Slot {}", slot + 1));
                    }
                    HardwareEvent::TouchSwipe { is_right } => {
                        tracing::info!("Physical Stream Deck Touch Swipe: right={}", is_right);
                        let is_swipe_active = {
                            let p = profile_hw.borrow();
                            p.current_page()
                                .and_then(|pg| pg.touchscreen_action.as_ref())
                                .map(|a| a.action_id != "disabled")
                                .unwrap_or(true)
                        };
                        if is_swipe_active {
                            let mut p = profile_hw.borrow_mut();
                            if is_right {
                                p.prev_page();
                            } else {
                                p.next_page();
                            }
                            let p_imm = p.clone();
                            drop(p);
                            canvas_hw.render_profile_page(&p_imm);
                            Self::sync_hardware(&devs_hw, *act_hw.borrow(), &hwm_hw, &p_imm);
                        }
                    }
                }
            }
        });

        // Start Hardware Listener for initial device
        let sender_hw_loop = sender.clone();
        if let Some(first_dev) = connected_devices.borrow().first() {
            let s = sender_hw_loop.clone();
            hardware_manager.borrow().start_listener(
                first_dev.dev_path.clone(),
                first_dev.kind,
                move |event| {
                    let _ = s.send_blocking(event);
                },
            );
            let _ = hardware_manager.borrow().set_brightness(&first_dev.dev_path, 80);
            hardware_manager.borrow().sync_hardware_page(&first_dev.dev_path, first_dev.kind, &profile.borrow());
        }

        // ===================== Synchronize Profile & Header =====================
        let sync_all = {
            let profs = profiles.clone();
            let act_id = active_profile_id.clone();
            let p_cur = profile.clone();
            let c_cur = canvas.clone();
            let d_cur = connected_devices.clone();
            let act_idx = active_device_idx.clone();
            let h_cur = hardware_manager.clone();
            let hdr = header.clone();

            Rc::new(move || {
                let devs = d_cur.borrow();
                let cur_idx = *act_idx.borrow();
                let dev_kind = devs.get(cur_idx).map(|d| d.kind).unwrap_or(DeviceKind::StreamDeckPlus);
                drop(devs);

                let mut profs_b = profs.borrow_mut();
                let matching_indices: Vec<usize> = profs_b
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.device_kind == dev_kind)
                    .map(|(i, _)| i)
                    .collect();

                let cur_id = act_id.borrow().clone();
                let found_idx = matching_indices.iter().find(|&&i| profs_b[i].id == cur_id)
                    .copied()
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
                drop(profs_b);

                hdr.update_profiles(&p_names, selected_pos);
                c_cur.render_profile_page(&p_cur.borrow());
                Self::sync_hardware(&d_cur, cur_idx, &h_cur, &p_cur.borrow());
            })
        };

        // Run initial synchronization
        sync_all();

        // Connect Header Profile Dropdown switching
        let profs_dd = profiles.clone();
        let act_id_dd = active_profile_id.clone();
        let devs_dd = connected_devices.clone();
        let act_idx_dd = active_device_idx.clone();
        let sync_dd = sync_all.clone();
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
                    *act_id_dd.borrow_mut() = target.id.clone();
                    drop(profs_b);
                    sync_dd();
                }
            }
        });

        // ===================== Connect Device Dropdown Switcher =====================
        let canvas_dev = canvas.clone();
        let profile_dev = profile.clone();
        let inspector_dev = inspector.clone();
        let pm_dev = plugin_manager.clone();
        let devs_select = connected_devices.clone();
        let hw_dev_switch = hardware_manager.clone();
        let active_idx_switch = active_device_idx.clone();
        let sender_switch = sender.clone();
        let sync_dev_change = sync_all.clone();

        header.device_dropdown.connect_selected_notify(move |dd| {
            let sel_idx = dd.selected() as usize;
            let devs = devs_select.borrow();
            if let Some(dev) = devs.get(sel_idx) {
                *active_idx_switch.borrow_mut() = sel_idx;

                // 1. Rebuild canvas grid, touchscreen, and dials for the selected device
                canvas_dev.build_hardware_layout(dev.kind);
                drop(devs);
                sync_dev_change();
                canvas_dev.select_key(0);
                *inspector_dev.current_key_idx.borrow_mut() = Some(0);
                let prof_dev_lock = profile_dev.borrow();
                inspector_dev.set_key_action(prof_dev_lock.current_page().and_then(|p| p.keys.get(&0)));
                drop(prof_dev_lock);

                // 2. Rebind click events to newly created buttons and dials
                Self::bind_canvas_events(
                    &canvas_dev,
                    &inspector_dev,
                    &profile_dev,
                    &pm_dev,
                    &devs_select,
                    &active_idx_switch,
                    &hw_dev_switch,
                );

                // 3. Reconnect hardware listener thread to this device
                let s = sender_switch.clone();
                let devs_lock = devs_select.borrow();
                if let Some(d) = devs_lock.get(sel_idx) {
                    hw_dev_switch.borrow().start_listener(
                        d.dev_path.clone(),
                        d.kind,
                        move |event| {
                            let _ = s.send_blocking(event);
                        },
                    );
                    let _ = hw_dev_switch.borrow().set_brightness(&d.dev_path, 80);
                    hw_dev_switch.borrow().sync_hardware_page(&d.dev_path, d.kind, &profile_dev.borrow());
                }
            }
        });

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
        let sidebar_store = sidebar.clone();
        let profile_store = profile.clone();
        let canvas_store = canvas.clone();
        let inspector_store = inspector.clone();
        let hw_store = hardware_manager.clone();
        let devs_store = connected_devices.clone();
        let active_idx_store = active_device_idx.clone();

        header.store_button.connect_clicked(move |_| {
            let sb_cb = sidebar_store.clone();
            let pm_cb = pm_store.clone();
            let prof_cb = profile_store.clone();
            let can_cb = canvas_store.clone();
            let insp_cb = inspector_store.clone();
            let hw_cb = hw_store.clone();
            let devs_cb = devs_store.clone();
            let act_cb = active_idx_store.clone();

            let on_installed = Rc::new(move || {
                let pm_ref = pm_cb.borrow();
                let prof_ref = prof_cb.clone();
                let can_ref = can_cb.clone();
                let insp_ref = insp_cb.clone();
                let hw_ref = hw_cb.clone();
                let d_ref = devs_cb.clone();
                let a_ref = act_cb.clone();

                sb_cb.populate_from_plugins(&pm_ref, move |sel| {
                    let cur_key = can_ref.selected_idx.borrow().unwrap_or(0);
                    {
                        let mut prof = prof_ref.borrow_mut();
                        if let Some(page) = prof.current_page_mut() {
                            let action = KeyAction::new(&sel.plugin_id, &sel.plugin_name, &sel.action_id, &sel.action_name);
                            page.keys.insert(cur_key, action);
                        }
                    }
                    let prof = prof_ref.borrow();
                    can_ref.render_profile_page(&prof);
                    if let Some(page) = prof.current_page() {
                        insp_ref.set_key_action(page.keys.get(&cur_key));
                    }
                    insp_ref.highlight_action_settings();
                    drop(prof);
                    Self::sync_hardware(&d_ref, *a_ref.borrow(), &hw_ref, &prof_ref.borrow());
                });
            });

            let store_win = create_store_window(&win_clone2, pm_store.clone(), on_installed);
            store_win.present();
        });

        // ===================== Connect Pagination Actions =====================
        let prof_page_add = profile.clone();
        let canvas_page_add = canvas.clone();
        let hw_page_add = hardware_manager.clone();
        let devs_page_add = connected_devices.clone();
        let act_page_add = active_device_idx.clone();

        canvas.add_page_btn.connect_clicked(move |_| {
            {
                let mut p = prof_page_add.borrow_mut();
                p.add_page();
            }
            let p = prof_page_add.borrow();
            canvas_page_add.render_profile_page(&p);
            drop(p);
            Self::sync_hardware(&devs_page_add, *act_page_add.borrow(), &hw_page_add, &prof_page_add.borrow());
        });

        let prof_page_del = profile.clone();
        let canvas_page_del = canvas.clone();
        let hw_page_del = hardware_manager.clone();
        let devs_page_del = connected_devices.clone();
        let act_page_del = active_device_idx.clone();

        canvas.remove_page_btn.connect_clicked(move |_| {
            {
                let mut p = prof_page_del.borrow_mut();
                p.remove_current_page();
            }
            let p = prof_page_del.borrow();
            canvas_page_del.render_profile_page(&p);
            drop(p);
            Self::sync_hardware(&devs_page_del, *act_page_del.borrow(), &hw_page_del, &prof_page_del.borrow());
        });

        let prof_page_sel = profile.clone();
        let canvas_page_sel = canvas.clone();
        let insp_page_sel = inspector.clone();
        let hw_page_sel = hardware_manager.clone();
        let devs_page_sel = connected_devices.clone();
        let act_page_sel = active_device_idx.clone();

        canvas.set_page_callback(move |page_idx| {
            {
                let mut p = prof_page_sel.borrow_mut();
                p.set_page(page_idx);
            }
            let p = prof_page_sel.borrow();
            canvas_page_sel.render_profile_page(&p);
            let cur_key = canvas_page_sel.selected_idx.borrow().unwrap_or(0);
            if let Some(page) = p.current_page() {
                insp_page_sel.set_key_action(page.keys.get(&cur_key));
            }
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

        let insp_ui_title = inspector.clone();
        inspector.title_entry.connect_changed(move |entry| {
            if insp_updating_title.get() {
                return;
            }
            let target = *insp_target_title.borrow();
            let text = entry.text().to_string();
            {
                let mut prof = prof_title.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.title = text.clone();
                }
            }
            insp_ui_title.preview_inner_label.set_text(&text);
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

        inspector.title_toggle_btn.connect_clicked(move |btn| {
            let target = *insp_title_toggle.current_target.borrow();
            let mut is_shown = true;
            {
                let mut prof = prof_title_toggle.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.show_title = !action.appearance.show_title;
                    is_shown = action.appearance.show_title;
                }
            }
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

        inspector.font_dialog_button.connect_font_desc_notify(move |fdb| {
            if insp_font.is_updating.get() {
                return;
            }
            let target = *insp_font.current_target.borrow();
            if let Some(desc) = fdb.font_desc() {
                let fam = desc.family().map(|s| s.to_string()).unwrap_or_else(|| "Cantarell".into());
                let sz = (desc.size() / pango::SCALE).max(6) as u32;
                {
                    let mut prof = prof_font.borrow_mut();
                    if let Some(page) = prof.current_page_mut() {
                        let action = get_target_mut(page, target);
                        action.appearance.font_family = fam;
                        action.appearance.font_size = sz;
                    }
                }
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

        inspector.font_size_spin.connect_value_changed(move |spin| {
            if insp_spin.is_updating.get() {
                return;
            }
            let target = *insp_spin.current_target.borrow();
            let sz = spin.value() as u32;
            {
                let mut prof = prof_spin.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.font_size = sz;
                }
            }
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

        inspector.btn_color_picker.connect_rgba_notify(move |cp| {
            if insp_updating_bg.get() {
                return;
            }
            let target = *insp_target_bg.borrow();
            let hex = rgba_to_hex(&cp.rgba());
            {
                let mut prof = prof_bg.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.bg_color = Some(hex);
                }
            }
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

        inspector.btn_color_reset.connect_clicked(move |_| {
            let target = *insp_bg_rst.current_target.borrow();
            {
                let mut prof = prof_bg_rst.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.bg_color = None;
                }
            }
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

        inspector.text_color_picker.connect_rgba_notify(move |cp| {
            if insp_updating_fg.get() {
                return;
            }
            let target = *insp_target_fg.borrow();
            let hex = rgba_to_hex(&cp.rgba());
            {
                let mut prof = prof_fg.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.text_color = hex;
                }
            }
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

            dialog.open(Some(&win_icon), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res {
                    if let Some(path) = file.path() {
                        let raw_str = path.to_string_lossy().to_string();
                        let path_str = resolve_possible_ellipsis_path(&raw_str);
                        {
                            let mut p = prof_c.borrow_mut();
                            if let Some(page) = p.current_page_mut() {
                                let action = get_target_mut(page, target);
                                action.appearance.icon_path = Some(path_str.clone());
                            }
                        }
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

        inspector.clear_icon_btn.connect_clicked(move |_| {
            let target = *insp_rst_icon.current_target.borrow();
            {
                let mut p = prof_rst_icon.borrow_mut();
                if let Some(page) = p.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.icon_path = None;
                }
            }
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

        inspector.icon_scale_spin.connect_value_changed(move |spin| {
            if insp_scale.is_updating.get() {
                return;
            }
            let target = *insp_scale.current_target.borrow();
            let val = spin.value() as u32;
            {
                let mut prof = prof_scale.borrow_mut();
                if let Some(page) = prof.current_page_mut() {
                    let action = get_target_mut(page, target);
                    action.appearance.icon_scale = val;
                }
            }

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
                           act: Rc<RefCell<usize>>| {
            btn.connect_clicked(move |_| {
                let target = *target_ref.borrow();
                {
                    let mut p = prof.borrow_mut();
                    if let Some(page) = p.current_page_mut() {
                        let action = get_target_mut(page, target);
                        action.appearance.vertical_align = pos;
                    }
                }
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

        connect_pos(&inspector.pos_top_btn, TextVerticalAlign::Top, profile.clone(), canvas.clone(), inspector.current_target.clone(), inspector.clone(), hardware_manager.clone(), connected_devices.clone(), active_device_idx.clone());
        connect_pos(&inspector.pos_mid_btn, TextVerticalAlign::Center, profile.clone(), canvas.clone(), inspector.current_target.clone(), inspector.clone(), hardware_manager.clone(), connected_devices.clone(), active_device_idx.clone());
        connect_pos(&inspector.pos_bot_btn, TextVerticalAlign::Bottom, profile.clone(), canvas.clone(), inspector.current_target.clone(), inspector.clone(), hardware_manager.clone(), connected_devices.clone(), active_device_idx.clone());

        // App Dropdown Selection
        let prof_app_dd = profile.clone();
        let canvas_app_dd = canvas.clone();
        let insp_app_dd = inspector.clone();
        let hw_app = hardware_manager.clone();
        let devs_app = connected_devices.clone();
        let act_app = active_device_idx.clone();

        inspector.app_dropdown.connect_selected_notify(move |dd| {
            let sel_idx = dd.selected() as usize;
            let target = *insp_app_dd.current_target.borrow();
            let apps = insp_app_dd.installed_apps.borrow();
            if let Some(app) = apps.get(sel_idx) {
                let mut needs_title_update = false;
                let app_name = app.name.clone();
                {
                    let mut prof = prof_app_dd.borrow_mut();
                    if let Some(page) = prof.current_page_mut() {
                        let action = get_target_mut(page, target);
                        action.parameters.insert("desktop_id".into(), app.desktop_id.clone());
                        action.parameters.insert("app_name".into(), app.name.clone());
                        action.parameters.insert("icon_name".into(), app.icon.clone());
                        if action.appearance.title.is_empty() || action.appearance.title == "Launch App" {
                            action.appearance.title = app_name.clone();
                            needs_title_update = true;
                        }
                    }
                }
                if needs_title_update {
                    insp_app_dd.is_updating.set(true);
                    insp_app_dd.title_entry.set_text(&app_name);
                    insp_app_dd.is_updating.set(false);
                }
                let prof = prof_app_dd.borrow();
                canvas_app_dd.render_profile_page(&prof);
                let act_opt = match target {
                    InspectorTarget::Key(k) => prof.current_page().and_then(|p| p.keys.get(&k)),
                    InspectorTarget::Dial(d) => prof.current_page().and_then(|p| p.dial_actions.get(&d)),
                    InspectorTarget::Touchscreen => prof.current_page().and_then(|p| p.touchscreen_action.as_ref()),
                };
                insp_app_dd.set_key_action(act_opt);
                drop(prof);
                Self::sync_hardware(&devs_app, *act_app.borrow(), &hw_app, &prof_app_dd.borrow());
            }
        });

        // Touch Strip Swipe Dropdown Selection
        let prof_swipe_dd = profile.clone();
        let insp_swipe_dd = inspector.clone();
        inspector.swipe_dropdown.connect_selected_notify(move |dd| {
            if insp_swipe_dd.is_updating.get() {
                return;
            }
            let sel = dd.selected();
            let mut prof = prof_swipe_dd.borrow_mut();
            if let Some(page) = prof.current_page_mut() {
                let action = page.get_or_create_touchscreen();
                if sel == 1 {
                    action.action_id = "disabled".into();
                } else {
                    action.action_id = "switch_pages".into();
                }
            }
        });

        // Delete (Clear Action) Button
        let prof_del_key = profile.clone();
        let canvas_del_key = canvas.clone();
        let insp_del_key = inspector.clone();
        let hw_del = hardware_manager.clone();
        let devs_del = connected_devices.clone();
        let act_del = active_device_idx.clone();

        inspector.delete_button.connect_clicked(move |_| {
            let target = *insp_del_key.current_target.borrow();
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
            let prof = prof_del_key.borrow();
            canvas_del_key.render_profile_page(&prof);
            insp_del_key.set_key_action(None);
            drop(prof);
            Self::sync_hardware(&devs_del, *act_del.borrow(), &hw_del, &prof_del_key.borrow());
        });

        // Populate Dynamic Sidebar
        let prof_sidebar = profile.clone();
        let canvas_sidebar = canvas.clone();
        let insp_sidebar = inspector.clone();
        let hw_sidebar = hardware_manager.clone();
        let devs_sidebar = connected_devices.clone();
        let act_sidebar = active_device_idx.clone();

        sidebar.populate_from_plugins(&plugin_manager.borrow(), move |sel| {
            let target = *insp_sidebar.current_target.borrow();
            let mut action = KeyAction::new(&sel.plugin_id, &sel.plugin_name, &sel.action_id, &sel.action_name);
            action.appearance.title = sel.action_name.clone();
            if !sel.icon_name.is_empty() {
                action.parameters.insert("icon_name".into(), sel.icon_name.clone());
            }
            {
                let mut prof = prof_sidebar.borrow_mut();
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
            let prof = prof_sidebar.borrow();
            canvas_sidebar.render_profile_page(&prof);
            if let Some(page) = prof.current_page() {
                let act_ref = match target {
                    InspectorTarget::Key(k) => page.keys.get(&k),
                    InspectorTarget::Dial(d) => page.dial_actions.get(&d),
                    InspectorTarget::Touchscreen => page.touchscreen_action.as_ref(),
                };
                insp_sidebar.set_key_action(act_ref);
            }
            insp_sidebar.highlight_action_settings();
            drop(prof);
            Self::sync_hardware(&devs_sidebar, *act_sidebar.borrow(), &hw_sidebar, &prof_sidebar.borrow());
        });

        // Wire Drag and Drop from Sidebar onto Canvas Keys, Dials, and Touchscreen
        let prof_drop = profile.clone();
        let canvas_drop = canvas.clone();
        let insp_drop = inspector.clone();
        let hw_drop = hardware_manager.clone();
        let devs_drop = connected_devices.clone();
        let act_drop = active_device_idx.clone();

        canvas.set_action_dropped_callback(move |target, sel| {
            let mut action = KeyAction::new(&sel.plugin_id, &sel.plugin_name, &sel.action_id, &sel.action_name);
            action.appearance.title = sel.action_name.clone();
            if !sel.icon_name.is_empty() {
                action.parameters.insert("icon_name".into(), sel.icon_name.clone());
            }
            {
                let mut prof = prof_drop.borrow_mut();
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

            // Automatically select the target and show Property Inspector
            insp_drop.set_target(target);
            match target {
                InspectorTarget::Key(k) => canvas_drop.select_key(k),
                InspectorTarget::Dial(d) => canvas_drop.select_dial_and_touch(d),
                InspectorTarget::Touchscreen => canvas_drop.select_touchscreen_panoramic(),
            }

            let prof = prof_drop.borrow();
            canvas_drop.render_profile_page(&prof);
            if let Some(page) = prof.current_page() {
                let act_ref = match target {
                    InspectorTarget::Key(k) => page.keys.get(&k),
                    InspectorTarget::Dial(d) => page.dial_actions.get(&d),
                    InspectorTarget::Touchscreen => page.touchscreen_action.as_ref(),
                };
                insp_drop.set_key_action(act_ref);
            }
            insp_drop.highlight_action_settings();
            drop(prof);
            Self::sync_hardware(&devs_drop, *act_drop.borrow(), &hw_drop, &prof_drop.borrow());
        });

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
            hardware_manager,
            connected_devices,
            active_device_idx,
        }
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
            hw.borrow().sync_hardware_page(&dev.dev_path, dev.kind, profile);
        }
    }

    /// Binds click events for key buttons, touchscreen segments, and dials
    pub fn bind_canvas_events(
        canvas: &Rc<Canvas>,
        inspector: &Rc<PropertyInspector>,
        profile: &Rc<RefCell<Profile>>,
        _plugin_manager: &Rc<RefCell<PluginManager>>,
        _devices: &Rc<RefCell<Vec<ConnectedDevice>>>,
        _active_device_idx: &Rc<RefCell<usize>>,
        _hw: &Rc<RefCell<HardwareManager>>,
    ) {
        let keys = canvas.key_buttons.borrow();
        for (idx, btn) in keys.iter().enumerate() {
            let can_ref = canvas.clone();
            let insp_ref = inspector.clone();
            let prof_ref = profile.clone();

            btn.connect_clicked(move |_| {
                can_ref.select_key(idx);
                insp_ref.set_target(InspectorTarget::Key(idx));
                let prof = prof_ref.borrow();
                if let Some(page) = prof.current_page() {
                    let act = page.keys.get(&idx);
                    insp_ref.set_key_action(act);
                }
            });
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
            let hw_swipe = _hw.clone();
            let devs_swipe = _devices.clone();
            let act_swipe = _active_device_idx.clone();
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
                can_swipe.render_profile_page(&p_imm);
                Self::sync_hardware(&devs_swipe, *act_swipe.borrow(), &hw_swipe, &p_imm);
            });
            touch_overlay.add_controller(swipe_gesture);
        }

        canvas.set_dial_rotate_callback(move |dial_idx, delta| {
            tracing::info!("Software Dial Rotate: dial {}, delta {}", dial_idx, delta);
        });
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn get_target_mut(page: &mut crate::models::Page, target: InspectorTarget) -> &mut KeyAction {
    match target {
        InspectorTarget::Key(k) => page.get_or_create_key(k),
        InspectorTarget::Dial(d) => page.get_or_create_dial(d),
        InspectorTarget::Touchscreen => page.get_or_create_touchscreen(),
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
