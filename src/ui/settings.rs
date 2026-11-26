#![allow(deprecated)]

use crate::core::{autostart, ConnectedDevice, HardwareManager};
use crate::models::{CloseBehavior, DeviceKind, Profile, WindowState};
use adw::prelude::*;
use adw::{ActionRow, ComboRow, EntryRow, PreferencesGroup, PreferencesPage, PreferencesWindow, SwitchRow};
use gtk::{Adjustment, Box, Button, Image, ListBox, Orientation, Popover};
use std::cell::RefCell;
use std::rc::Rc;

pub fn create_settings_window(
    parent: &impl IsA<gtk::Window>,
    hardware_manager: Rc<RefCell<HardwareManager>>,
    connected_devices: Rc<RefCell<Vec<ConnectedDevice>>>,
    active_device_idx: usize,
    profiles: Rc<RefCell<Vec<Profile>>>,
    active_profile_id: Rc<RefCell<String>>,
    on_profile_updated: Rc<dyn Fn()>,
) -> PreferencesWindow {
    let window = PreferencesWindow::builder()
        .title("Preferences")
        .transient_for(parent)
        .modal(true)
        .default_width(620)
        .default_height(520)
        .build();

    window.add_css_class("ballista-modal");
    window.add_css_class("ballista-midnight-window");

    // ===================== Page 1: General (Matching Mockup) =====================
    let general_page = PreferencesPage::builder()
        .title("General")
        .icon_name("emblem-system-symbolic")
        .build();

    let hardware_group = PreferencesGroup::builder()
        .title("Hardware Configuration")
        .description("Adjust connected Stream Deck parameters in real-time")
        .build();

    let devs = connected_devices.borrow();
    let initial_idx = if active_device_idx < devs.len() { active_device_idx } else { 0 };
    let initial_dev = devs.get(initial_idx).cloned();

    let mut model_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for d in devs.iter() {
        *model_counts.entry(d.name.clone()).or_insert(0) += 1;
    }
    let mut model_indices: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut dev_names: Vec<String> = Vec::new();
    if devs.is_empty() {
        dev_names.push("No Devices Detected (Simulated)".into());
    } else {
        for d in devs.iter() {
            let count = *model_counts.get(&d.name).unwrap_or(&1);
            if count > 1 {
                let idx = model_indices.entry(d.name.clone()).or_insert(0);
                *idx += 1;
                dev_names.push(format!("{} #{}", d.name, idx));
            } else {
                dev_names.push(d.name.clone());
            }
        }
    }
    let main_window_dev_kind = initial_dev.as_ref().map(|d| d.kind).unwrap_or(DeviceKind::StreamDeckPlus);
    drop(devs);

    // 1. Device Selection Dropdown (Actual connected hardware)
    // Settings has its OWN target device selection; it does NOT alter the main studio GUI canvas!
    let dev_row = ActionRow::builder()
        .title("Stream Deck Device")
        .subtitle("Active hardware controller")
        .build();
    let dev_names_str: Vec<&str> = dev_names.iter().map(|s| s.as_str()).collect();
    let dev_dropdown = gtk::DropDown::from_strings(&dev_names_str);
    dev_dropdown.set_valign(gtk::Align::Center);
    if initial_idx < dev_names.len() {
        dev_dropdown.set_selected(initial_idx as u32);
    }
    dev_row.add_suffix(&dev_dropdown);
    hardware_group.add(&dev_row);

    // Settings-local device path & stable key tracker for brightness and name
    let settings_device_path = Rc::new(RefCell::new(initial_dev.as_ref().map(|d| d.dev_path.clone())));
    let settings_device_key = Rc::new(RefCell::new(initial_dev.as_ref().map(|d| d.stable_key())));

    // 2. Custom Device Name Entry (Matching Mockup)
    let initial_name = initial_dev.as_ref().map(|d| d.name.as_str()).unwrap_or("Stream Deck");
    let name_row = EntryRow::builder()
        .title("Device Name")
        .text(initial_name)
        .show_apply_button(true)
        .build();
    hardware_group.add(&name_row);

    // 3. Brightness Adjustment Slider (persisted per device in WindowState::device_brightness)
    let initial_bright = initial_dev
        .as_ref()
        .map(|d| WindowState::load().brightness_for(&d.stable_key()) as f64)
        .unwrap_or(80.0);
    let brightness_adj = Adjustment::new(initial_bright, 0.0, 100.0, 5.0, 10.0, 0.0);
    let syncing_bright = Rc::new(std::cell::Cell::new(false));

    // Handle device dropdown changes in Settings:
    // Only updates settings_device_path/key for brightness & name adjustment.
    // DOES NOT touch the main application GUI device or canvas.
    let act_path_for_dev = settings_device_path.clone();
    let act_key_for_dev = settings_device_key.clone();
    let conn_devs_for_dd = connected_devices.clone();
    let name_row_clone = name_row.clone();
    let bright_adj_dd = brightness_adj.clone();
    let sync_bright_dd = syncing_bright.clone();

    dev_dropdown.connect_selected_notify(move |dd| {
        let new_idx = dd.selected() as usize;
        let devs = conn_devs_for_dd.borrow();
        if let Some(dev) = devs.get(new_idx) {
            *act_path_for_dev.borrow_mut() = Some(dev.dev_path.clone());
            let skey = dev.stable_key();
            *act_key_for_dev.borrow_mut() = Some(skey.clone());
            name_row_clone.set_text(&dev.name);
            let saved_b = WindowState::load().brightness_for(&skey) as f64;
            sync_bright_dd.set(true);
            bright_adj_dd.set_value(saved_b);
            sync_bright_dd.set(false);
        }
    });

    let brightness_row = ActionRow::builder()
        .title("Brightness")
        .subtitle("Adjust physical hardware backlight intensity")
        .build();

    let bright_box = Box::new(Orientation::Horizontal, 10);
    bright_box.set_valign(gtk::Align::Center);
    bright_box.set_hexpand(true);

    let icon_dim = Image::from_icon_name("display-brightness-low-symbolic");
    icon_dim.set_pixel_size(18);
    icon_dim.add_css_class("dim-label");

    let scale = gtk::Scale::builder()
        .orientation(Orientation::Horizontal)
        .adjustment(&brightness_adj)
        .hexpand(true)
        .draw_value(false)
        .build();
    scale.set_size_request(220, -1);

    let icon_bright = Image::from_icon_name("display-brightness-high-symbolic");
    icon_bright.set_pixel_size(18);
    icon_bright.add_css_class("dim-label");

    let hw_bright = hardware_manager.clone();
    let dev_bright = settings_device_path.clone();
    let dev_key_bright = settings_device_key.clone();
    let sync_bright_chg = syncing_bright.clone();
    brightness_adj.connect_value_changed(move |adj| {
        if sync_bright_chg.get() {
            return;
        }
        let val = adj.value() as u8;
        if let Some(ref path) = *dev_bright.borrow() {
            let _ = hw_bright.borrow().set_brightness(path, val);
        }
        if let Some(ref skey) = *dev_key_bright.borrow() {
            let mut st = WindowState::load();
            st.set_brightness_for(skey, val);
            st.save();
        }
    });

    bright_box.append(&icon_dim);
    bright_box.append(&scale);
    bright_box.append(&icon_bright);
    brightness_row.add_suffix(&bright_box);
    hardware_group.add(&brightness_row);

    // 4. Sleep Timeout Dropdown
    let sleep_row = ActionRow::builder()
        .title("Sleep Timeout")
        .subtitle("Turn off screen when system is idle")
        .build();
    let sleep_dropdown = gtk::DropDown::from_strings(&["Never", "5 Minutes", "15 Minutes", "30 Minutes"]);
    sleep_dropdown.set_valign(gtk::Align::Center);
    sleep_row.add_suffix(&sleep_dropdown);
    hardware_group.add(&sleep_row);

    general_page.add(&hardware_group);
    window.add(&general_page);

    // ===================== Page 2: Profiles (Device-Specific, Interactive) =====================
    let profiles_page = PreferencesPage::builder()
        .title("Profiles")
        .icon_name("view-list-bullet-symbolic")
        .build();

    let dev_profile_group = PreferencesGroup::builder()
        .title("Device Profiles")
        .description("Profiles are device-specific based on hardware key layouts and dial capabilities")
        .build();

    // Device selector dropdown in Profiles tab - ONLY connected hardware
    let dev_select_row = ActionRow::builder()
        .title("Target Device")
        .subtitle("Select hardware model to manage its profiles")
        .build();

    let devs_lock = connected_devices.borrow();
    let target_dev_choices: Vec<(DeviceKind, String)> = if devs_lock.is_empty() {
        vec![(DeviceKind::StreamDeckPlus, "Simulated Stream Deck +".into())]
    } else {
        let mut seen = std::collections::HashSet::new();
        let mut list = Vec::new();
        for d in devs_lock.iter() {
            if seen.insert(d.kind) {
                list.push((d.kind, d.name.clone()));
            }
        }
        list
    };
    drop(devs_lock);

    let target_names: Vec<&str> = target_dev_choices.iter().map(|(_, name)| name.as_str()).collect();
    let target_dev_dropdown = gtk::DropDown::from_strings(&target_names);
    target_dev_dropdown.set_valign(gtk::Align::Center);

    let initial_pos = target_dev_choices.iter().position(|(k, _)| *k == main_window_dev_kind).unwrap_or(0);
    target_dev_dropdown.set_selected(initial_pos as u32);
    dev_select_row.add_suffix(&target_dev_dropdown);
    dev_profile_group.add(&dev_select_row);

    profiles_page.add(&dev_profile_group);

    // Profiles List Group
    let list_group = PreferencesGroup::builder()
        .title("Configured Profiles")
        .build();

    let profiles_list_box = ListBox::new();
    profiles_list_box.add_css_class("boxed-list");
    list_group.add(&profiles_list_box);

    let selected_device_kind = Rc::new(RefCell::new(main_window_dev_kind));

    // Dynamic rendering of the profiles list
    let render_profiles_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let render_profiles = {
        let list_box_ref = profiles_list_box.clone();
        let profs_ref = profiles.clone();
        let act_id_ref = active_profile_id.clone();
        let sel_dev_ref = selected_device_kind.clone();
        let on_update_cb = on_profile_updated.clone();
        let self_cell = render_profiles_cell.clone();

        Rc::new(move || {
            list_box_ref.select_row(None::<&gtk::ListBoxRow>);
            while let Some(child) = list_box_ref.first_child() {
                list_box_ref.remove(&child);
            }

            let cur_dev = *sel_dev_ref.borrow();
            let profs = profs_ref.borrow();
            let act_id = act_id_ref.borrow().clone();

            let matching_indices: Vec<usize> = profs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.device_kind == cur_dev)
                .map(|(i, _)| i)
                .collect();

            let matching_count = matching_indices.len();

            for &idx in &matching_indices {
                let p = &profs[idx];
                let is_active = p.id == act_id;
                let p_id = p.id.clone();
                let p_name = p.name.clone();

                let escaped_name = glib::markup_escape_text(&p.name);
                let row = ActionRow::builder()
                    .title(escaped_name.as_str())
                    .subtitle(if is_active { "Active Profile" } else { "Click to activate" })
                    .icon_name(if is_active { "starred-symbolic" } else { "view-list-bullet-symbolic" })
                    .activatable(true)
                    .build();

                let row_action_box = Box::new(Orientation::Horizontal, 4);
                row_action_box.set_valign(gtk::Align::Center);

                // Rename Button with Popover managed safely by MenuButton
                let rename_btn = gtk::MenuButton::new();
                rename_btn.set_icon_name("document-edit-symbolic");
                rename_btn.add_css_class("flat");
                rename_btn.set_tooltip_text(Some("Rename Profile"));

                let popover = Popover::new();
                let pop_box = Box::new(Orientation::Horizontal, 8);
                pop_box.set_margin_top(8);
                pop_box.set_margin_bottom(8);
                pop_box.set_margin_start(8);
                pop_box.set_margin_end(8);

                let entry = gtk::Entry::new();
                entry.set_text(&p_name);
                let save_btn = Button::with_label("Save");
                save_btn.add_css_class("suggested-action");

                pop_box.append(&entry);
                pop_box.append(&save_btn);
                popover.set_child(Some(&pop_box));
                rename_btn.set_popover(Some(&popover));

                let profs_rename = profs_ref.clone();
                let p_id_rename = p_id.clone();
                let on_update_rename = on_update_cb.clone();
                let self_cell_rename = self_cell.clone();
                let pop_save = popover.clone();
                let entry_save = entry.clone();

                let do_rename = move || {
                    let new_text = entry_save.text().to_string();
                    if !new_text.trim().is_empty() {
                        let mut pr = profs_rename.borrow_mut();
                        if let Some(target) = pr.iter_mut().find(|p| p.id == p_id_rename) {
                            target.name = new_text.trim().to_string();
                        }
                        drop(pr);
                        on_update_rename();
                        pop_save.popdown();
                        if let Some(ref r) = *self_cell_rename.borrow() {
                            r();
                        }
                    }
                };

                let do_rename_enter = do_rename.clone();
                entry.connect_activate(move |_| {
                    do_rename_enter();
                });
                save_btn.connect_clicked(move |_| {
                    do_rename();
                });

                row_action_box.append(&rename_btn);

                // Delete Button (only if more than 1 profile for this device)
                let del_btn = Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.set_tooltip_text(Some("Delete Profile"));

                if matching_count <= 1 {
                    del_btn.set_sensitive(false);
                    del_btn.set_tooltip_text(Some("Cannot delete the only profile for this device"));
                } else {
                    let profs_del = profs_ref.clone();
                    let p_id_del = p_id.clone();
                    let act_id_del = act_id_ref.clone();
                    let on_update_del = on_update_cb.clone();
                    let self_cell_del = self_cell.clone();

                    del_btn.connect_clicked(move |_| {
                        let mut pr = profs_del.borrow_mut();
                        pr.retain(|p| p.id != p_id_del);
                        if *act_id_del.borrow() == p_id_del {
                            if let Some(next_p) = pr.iter().find(|p| p.device_kind == cur_dev) {
                                *act_id_del.borrow_mut() = next_p.id.clone();
                            }
                        }
                        drop(pr);
                        on_update_del();
                        if let Some(ref r) = *self_cell_del.borrow() {
                            r();
                        }
                    });
                }
                row_action_box.append(&del_btn);

                row.add_suffix(&row_action_box);

                // Clicking the row activates it without switching the main application window device
                let act_id_click = act_id_ref.clone();
                let p_id_click = p_id.clone();
                let on_update_click = on_update_cb.clone();
                let self_cell_click = self_cell.clone();

                row.connect_activated(move |_| {
                    *act_id_click.borrow_mut() = p_id_click.clone();
                    if cur_dev == main_window_dev_kind {
                        on_update_click();
                    }
                    if let Some(ref r) = *self_cell_click.borrow() {
                        r();
                    }
                });

                list_box_ref.append(&row);
            }

            drop(profs);

            // Add Profile Action Row at bottom
            let add_row = ActionRow::builder()
                .title("Add New Profile")
                .subtitle("Create an additional layout configuration for this device")
                .build();

            let add_btn = Button::from_icon_name("list-add-symbolic");
            add_btn.add_css_class("flat");
            add_btn.set_valign(gtk::Align::Center);
            add_btn.set_tooltip_text(Some("Create New Profile"));

            let profs_add = profs_ref.clone();
            let act_id_add = act_id_ref.clone();
            let on_update_add = on_update_cb.clone();
            let self_cell_add = self_cell.clone();

            add_btn.connect_clicked(move |_| {
                let mut pr = profs_add.borrow_mut();
                let count = pr.iter().filter(|p| p.device_kind == cur_dev).count();
                let new_profile = Profile::new_for_device(format!("Profile {}", count + 1), cur_dev);
                *act_id_add.borrow_mut() = new_profile.id.clone();
                pr.push(new_profile);
                drop(pr);
                if cur_dev == main_window_dev_kind {
                    on_update_add();
                }
                if let Some(ref r) = *self_cell_add.borrow() {
                    r();
                }
            });

            add_row.add_suffix(&add_btn);
            list_box_ref.append(&add_row);
        })
    };
    *render_profiles_cell.borrow_mut() = Some(render_profiles.clone());

    // Initial render
    render_profiles();

    // When target device dropdown changes in Settings:
    let target_choices_clone = target_dev_choices.clone();
    let sel_dev_dd = selected_device_kind.clone();
    let render_dd = render_profiles.clone();
    target_dev_dropdown.connect_selected_notify(move |dd| {
        let sel = dd.selected() as usize;
        if let Some((kind, _)) = target_choices_clone.get(sel) {
            *sel_dev_dd.borrow_mut() = *kind;
            render_dd();
        }
    });

    profiles_page.add(&list_group);
    window.add(&profiles_page);

    // ===================== Page 3: System (Matching Mockup) =====================
    let system_page = PreferencesPage::builder()
        .title("System")
        .icon_name("preferences-system-symbolic")
        .build();

    let system_group = PreferencesGroup::builder()
        .title("System Integration")
        .description("Configure instant desktop login startup and background tray behavior")
        .build();

    let autostart_row = SwitchRow::builder()
        .title("Start on login")
        .subtitle("Launch Ballista background service automatically as soon as your desktop session starts")
        .active(autostart::is_autostart_enabled())
        .build();
    autostart_row.connect_active_notify(|sw| {
        if let Err(e) = autostart::set_autostart_enabled(sw.is_active()) {
            tracing::warn!("Failed to update autostart setting: {e}");
        }
    });
    system_group.add(&autostart_row);

    let close_options = gtk::StringList::new(&[
        "Ask Every Time",
        "Keep Running in Tray",
        "Quit Ballista",
    ]);
    let initial_close_idx = match WindowState::load().close_behavior {
        CloseBehavior::Ask => 0,
        CloseBehavior::MinimizeToTray => 1,
        CloseBehavior::Quit => 2,
    };
    let close_behavior_row = ComboRow::builder()
        .title("When Closing Window")
        .subtitle("Choose whether closing the window hides Ballista to the system tray or quits completely")
        .model(&close_options)
        .selected(initial_close_idx)
        .build();
    close_behavior_row.connect_selected_notify(|row| {
        let behavior = match row.selected() {
            1 => CloseBehavior::MinimizeToTray,
            2 => CloseBehavior::Quit,
            _ => CloseBehavior::Ask,
        };
        let mut st = WindowState::load();
        st.close_behavior = behavior;
        st.save();
    });
    system_group.add(&close_behavior_row);

    system_page.add(&system_group);

    let storage_group = PreferencesGroup::builder()
        .title("Plugin Storage")
        .description("Freedesktop StreamController and Ballista plugin location")
        .build();

    let path_row = ActionRow::builder()
        .title("Installed Plugins Directory")
        .subtitle("~/.local/share/ballista/plugins/")
        .icon_name("folder-symbolic")
        .build();

    let open_folder_btn = Button::with_label("Open Folder");
    open_folder_btn.set_valign(gtk::Align::Center);
    open_folder_btn.add_css_class("flat");
    open_folder_btn.connect_clicked(|_| {
        let path = shellexpand_plugins_dir();
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    });
    path_row.add_suffix(&open_folder_btn);
    storage_group.add(&path_row);

    system_page.add(&storage_group);
    window.add(&system_page);

    window
}

fn shellexpand_plugins_dir() -> String {
    glib::user_data_dir()
        .join("ballista")
        .join("plugins")
        .to_string_lossy()
        .to_string()
}