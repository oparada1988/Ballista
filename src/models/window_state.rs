use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloseBehavior {
    #[default]
    Ask,
    MinimizeToTray,
    Quit,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WindowState {
    pub window_width: i32,
    pub window_height: i32,
    pub sidebar_position: i32,
    pub work_paned_position: i32,
    #[serde(default = "default_store_width")]
    pub store_width: i32,
    #[serde(default = "default_store_height")]
    pub store_height: i32,
    #[serde(default)]
    pub close_behavior: CloseBehavior,
    /// Per-device brightness (keyed by device serial or fallback path), 0..=100
    #[serde(default)]
    pub device_brightness: HashMap<String, u8>,
}

fn default_store_width() -> i32 {
    940
}

fn default_store_height() -> i32 {
    840
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            window_width: 1160,
            window_height: 780,
            sidebar_position: 820,
            work_paned_position: 440,
            store_width: 940,
            store_height: 840,
            close_behavior: CloseBehavior::Ask,
            device_brightness: HashMap::new(),
        }
    }
}

impl WindowState {
    pub fn config_path() -> PathBuf {
        glib::user_config_dir()
            .join("ballista")
            .join("window_state.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(state) = serde_json::from_str::<Self>(&data) {
                    return state;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, &data).is_ok() {
                let _ = fs::rename(&tmp, &path);
            } else {
                let _ = fs::write(&path, &data);
            }
        }
    }

    pub fn brightness_for(&self, device_key: &str) -> u8 {
        self.device_brightness
            .get(device_key)
            .copied()
            .unwrap_or(80)
            .min(100)
    }

    pub fn set_brightness_for(&mut self, device_key: &str, pct: u8) {
        self.device_brightness
            .insert(device_key.to_string(), pct.min(100));
    }
}
