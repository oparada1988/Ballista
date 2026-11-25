#![allow(deprecated)]

use adw::prelude::*;
use adw::{EntryRow, PasswordEntryRow, PreferencesGroup, PreferencesPage, PreferencesWindow};
use gtk::{Button, HeaderBar};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;
use std::cell::RefCell;

pub fn open_plugin_settings_dialog(
    parent: &impl IsA<gtk::Window>,
    plugin_id: &str,
    plugin_name: &str,
) {
    let window = PreferencesWindow::builder()
        .title(&format!("{plugin_name} Settings"))
        .transient_for(parent)
        .modal(true)
        .default_width(580)
        .default_height(480)
        .build();

    window.add_css_class("ballista-modal");
    window.add_css_class("ballista-midnight-window");

    let page = PreferencesPage::new();
    let group = PreferencesGroup::new();
    group.set_title(&format!("{plugin_name} Configuration"));
    group.set_description(Some("Global plugin configuration and credentials"));

    // 1. Locate and load settings from Ballista or StreamController
    let ballista_dir = glib::user_data_dir().join("ballista").join("settings").join("plugins").join(plugin_id);
    let ballista_file = ballista_dir.join("settings.json");

    let home = glib::home_dir();
    let streamcontroller_dir = home.join(".var/app/com.core447.StreamController/data/settings/plugins").join(plugin_id);
    let streamcontroller_file = streamcontroller_dir.join("settings.json");

    let mut current_settings = BTreeMap::<String, String>::new();

    // Prefer StreamController's settings if present, fallback to Ballista's
    let loaded_path = if streamcontroller_file.exists() {
        Some(streamcontroller_file.clone())
    } else if ballista_file.exists() {
        Some(ballista_file.clone())
    } else {
        None
    };

    if let Some(ref p) = loaded_path {
        if let Ok(content) = fs::read_to_string(p) {
            if let Ok(val) = serde_json::from_str::<Value>(&content) {
                if let Some(obj) = val.get("settings").and_then(|s| s.as_object()) {
                    for (k, v) in obj {
                        let str_val = match v {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => v.to_string(),
                        };
                        current_settings.insert(k.clone(), str_val);
                    }
                }
            }
        }
    }

    // Default template keys for common plugins if empty
    if current_settings.is_empty() {
        if plugin_id.contains("Govee") {
            current_settings.insert("api_key".into(), "".into());
        } else if plugin_id.contains("Discord") {
            current_settings.insert("client_id".into(), "".into());
            current_settings.insert("client_secret".into(), "".into());
            current_settings.insert("redirect_uri".into(), "http://localhost:9000".into());
        } else if plugin_id.contains("OBS") {
            current_settings.insert("host".into(), "localhost".into());
            current_settings.insert("port".into(), "4455".into());
            current_settings.insert("password".into(), "".into());
        } else if plugin_id.contains("HomeAssistant") {
            current_settings.insert("host".into(), "http://homeassistant.local:8123".into());
            current_settings.insert("access_token".into(), "".into());
        } else if plugin_id.contains("MusicAssistant") {
            current_settings.insert("server_url".into(), "http://localhost:8095".into());
        } else {
            current_settings.insert("api_key".into(), "".into());
        }
    }

    let entries = Rc::new(RefCell::new(BTreeMap::<String, (bool, gtk::Widget)>::new()));

    for (key, val) in &current_settings {
        let is_secret = key.contains("secret")
            || key.contains("password")
            || key.contains("token")
            || (key.contains("key") && !key.contains("public"));

        let friendly_title = format_friendly_title(key);

        if is_secret {
            let row = PasswordEntryRow::new();
            row.set_title(&friendly_title);
            row.set_text(val);
            group.add(&row);
            entries.borrow_mut().insert(key.clone(), (true, row.upcast()));
        } else {
            let row = EntryRow::new();
            row.set_title(&friendly_title);
            row.set_text(val);
            group.add(&row);
            entries.borrow_mut().insert(key.clone(), (false, row.upcast()));
        }
    }

    page.add(&group);
    window.add(&page);

    // Save Button in Header Bar
    let save_btn = Button::with_label("Save");
    save_btn.add_css_class("suggested-action");
    save_btn.set_valign(gtk::Align::Center);

    let entries_save = entries.clone();
    let win_save = window.clone();
    let p_id_save = plugin_id.to_string();

    save_btn.connect_clicked(move |_| {
        let mut save_map = serde_json::Map::new();
        for (k, (is_sec, widget)) in entries_save.borrow().iter() {
            let text = if *is_sec {
                widget.downcast_ref::<PasswordEntryRow>().map(|r| r.text().to_string()).unwrap_or_default()
            } else {
                widget.downcast_ref::<EntryRow>().map(|r| r.text().to_string()).unwrap_or_default()
            };
            save_map.insert(k.clone(), Value::String(text.trim().to_string()));
        }

        let out_json = json!({
            "file-version": "2.0",
            "settings": save_map,
        });

        if let Ok(formatted) = serde_json::to_string_pretty(&out_json) {
            // Save to Ballista settings dir
            let b_dir = glib::user_data_dir().join("ballista").join("settings").join("plugins").join(&p_id_save);
            let _ = fs::create_dir_all(&b_dir);
            let _ = fs::write(b_dir.join("settings.json"), &formatted);

            // Save to StreamController settings dir if accessible
            let sc_dir = glib::home_dir().join(".var/app/com.core447.StreamController/data/settings/plugins").join(&p_id_save);
            if sc_dir.parent().map_or(false, |p| p.exists()) {
                let _ = fs::create_dir_all(&sc_dir);
                let _ = fs::write(sc_dir.join("settings.json"), &formatted);
            }
        }

        win_save.close();
    });

    // Add save button to window header
    let header = HeaderBar::new();
    header.add_css_class("ballista-headerbar");
    header.pack_end(&save_btn);

    window.present();
}

fn format_friendly_title(key: &str) -> String {
    match key {
        "api_key" => "API Key".into(),
        "client_id" => "Client ID".into(),
        "client_secret" => "Client Secret".into(),
        "redirect_uri" => "Redirect URI".into(),
        "access_token" => "Access Token".into(),
        "refresh_token" => "Refresh Token".into(),
        "host" => "Host URL".into(),
        "port" => "Port".into(),
        "password" => "Password".into(),
        "server_url" => "Server URL".into(),
        _ => {
            let words: Vec<String> = key.split('_')
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    }
                })
                .collect();
            words.join(" ")
        }
    }
}
