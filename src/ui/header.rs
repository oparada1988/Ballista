use crate::core::ConnectedDevice;
use adw::HeaderBar;
use gtk::prelude::*;
use gtk::{Box, Button, DropDown, Orientation, StringList};

#[derive(Clone)]
pub struct Header {
    pub header_bar: HeaderBar,
    pub device_dropdown: DropDown,
    pub profile_dropdown: DropDown,
    pub store_button: Button,
    pub settings_button: Button,
}

impl Header {
    pub fn new() -> Self {
        let header_bar = HeaderBar::new();
        header_bar.add_css_class("ballista-headerbar");
        header_bar.set_show_title(false);

        // Left Controls: Device & Profile dropdowns
        let left_box = Box::new(Orientation::Horizontal, 8);
        left_box.set_margin_start(4);

        let initial_devices = StringList::new(&["Scanning Devices..."]);
        let device_dropdown = DropDown::new(Some(initial_devices), Option::<gtk::Expression>::None);
        device_dropdown.add_css_class("device-dropdown");
        device_dropdown.set_tooltip_text(Some("Select Hardware Device"));

        let profiles = StringList::new(&[
            "Default Profile",
            "Streaming Setup",
            "Gaming Macro Setup",
        ]);
        let profile_dropdown = DropDown::new(Some(profiles), Option::<gtk::Expression>::None);
        profile_dropdown.add_css_class("profile-dropdown");
        profile_dropdown.set_tooltip_text(Some("Select Active Profile"));

        left_box.append(&device_dropdown);
        left_box.append(&profile_dropdown);
        header_bar.pack_start(&left_box);

        // Right Controls: Store & Settings
        let right_box = Box::new(Orientation::Horizontal, 4);
        right_box.set_margin_end(4);

        if let Some(display) = gtk::gdk::Display::default() {
            let icon_theme = gtk::IconTheme::for_display(&display);
            let mut icon_dirs = vec![
                glib::user_data_dir().join("ballista").join("icons"),
                std::path::PathBuf::from("/usr/local/share/ballista/icons"),
                std::path::PathBuf::from("/usr/share/ballista/icons"),
                std::path::PathBuf::from("resources/icons"),
            ];
            if let Ok(exe) = std::env::current_exe() {
                if let Some(exe_dir) = exe.parent() {
                    icon_dirs.push(exe_dir.join("resources").join("icons"));
                    icon_dirs.push(exe_dir.join("..").join("share").join("ballista").join("icons"));
                    icon_dirs.push(exe_dir.join("..").join("..").join("resources").join("icons"));
                }
            }
            for dir in icon_dirs {
                if dir.exists() {
                    icon_theme.add_search_path(&dir);
                }
            }
        }

        let store_button = Button::from_icon_name("store-badge-symbolic");
        store_button.set_tooltip_text(Some("Plugin Store / Marketplace"));
        store_button.add_css_class("flat");

        let settings_button = Button::from_icon_name("emblem-system-symbolic");
        settings_button.set_tooltip_text(Some("Preferences & Settings"));
        settings_button.add_css_class("flat");

        right_box.append(&store_button);
        right_box.append(&settings_button);
        header_bar.pack_end(&right_box);

        Self {
            header_bar,
            device_dropdown,
            profile_dropdown,
            store_button,
            settings_button,
        }
    }

    pub fn populate_devices(&self, devices: &[ConnectedDevice]) {
        let mut names: Vec<String> = Vec::new();
        if devices.is_empty() {
            names.push("No Devices Detected (Simulated)".into());
        } else {
            // Count device occurrences to cleanly disambiguate only if multiple of identical model are attached
            let mut model_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for d in devices {
                *model_counts.entry(d.name.clone()).or_insert(0) += 1;
            }
            let mut model_indices: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for d in devices {
                let count = *model_counts.get(&d.name).unwrap_or(&1);
                if count > 1 {
                    let idx = model_indices.entry(d.name.clone()).or_insert(0);
                    *idx += 1;
                    names.push(format!("{} #{}", d.name, idx));
                } else {
                    names.push(d.name.clone());
                }
            }
        }

        let str_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let list = StringList::new(&str_refs);
        self.device_dropdown.set_model(Some(&list));
    }

    pub fn update_profiles(&self, names: &[String], selected_idx: usize) {
        let str_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let list = StringList::new(&str_refs);
        self.profile_dropdown.set_model(Some(&list));
        self.profile_dropdown.set_selected(selected_idx as u32);
    }
}
