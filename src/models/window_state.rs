use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
}

fn default_store_width() -> i32 {
    780
}

fn default_store_height() -> i32 {
    640
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            window_width: 1160,
            window_height: 780,
            sidebar_position: 820,
            work_paned_position: 440,
            store_width: 780,
            store_height: 640,
        }
    }
}

impl WindowState {
    pub fn config_path() -> PathBuf {
        glib::user_config_dir().join("ballista").join("window_state.json")
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
            let _ = fs::write(&path, data);
        }
    }
}
