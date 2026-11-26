use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    /// Plugin IDs whose category group is collapsed in the Actions sidebar
    #[serde(default)]
    pub collapsed_sidebar_plugins: HashSet<String>,
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
            collapsed_sidebar_plugins: HashSet::new(),
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

    pub fn is_sidebar_plugin_collapsed(&self, plugin_id: &str) -> bool {
        self.collapsed_sidebar_plugins.contains(plugin_id)
    }

    pub fn set_sidebar_plugin_collapsed(&mut self, plugin_id: &str, collapsed: bool) {
        if collapsed {
            self.collapsed_sidebar_plugins.insert(plugin_id.to_string());
        } else {
            self.collapsed_sidebar_plugins.remove(plugin_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_collapsed_state_persists_through_serde() {
        let mut st = WindowState::default();
        assert!(!st.is_sidebar_plugin_collapsed("com_oparada_MusicAssistant"));
        st.set_sidebar_plugin_collapsed("com_oparada_MusicAssistant", true);
        assert!(st.is_sidebar_plugin_collapsed("com_oparada_MusicAssistant"));

        let json = serde_json::to_string(&st).unwrap();
        let restored: WindowState = serde_json::from_str(&json).unwrap();
        assert!(restored.is_sidebar_plugin_collapsed("com_oparada_MusicAssistant"));
        assert!(!restored.is_sidebar_plugin_collapsed("com.ballista.core"));

        let mut restored = restored;
        restored.set_sidebar_plugin_collapsed("com_oparada_MusicAssistant", false);
        assert!(!restored.is_sidebar_plugin_collapsed("com_oparada_MusicAssistant"));
    }
}
