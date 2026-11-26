use super::action::KeyAction;
use super::device::DeviceKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TouchscreenMode {
    #[default]
    DialCards,
    Panoramic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: usize,
    pub name: String,
    pub keys: HashMap<usize, KeyAction>,
    #[serde(default)]
    pub dial_actions: HashMap<usize, KeyAction>,
    #[serde(default)]
    pub touchscreen_action: Option<KeyAction>,
    #[serde(default)]
    pub touchscreen_mode: TouchscreenMode,
}

impl Page {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            name: format!("Page {}", id + 1),
            keys: HashMap::new(),
            dial_actions: HashMap::new(),
            touchscreen_action: None,
            touchscreen_mode: TouchscreenMode::DialCards,
        }
    }

    pub fn get_or_create_key(&mut self, key_idx: usize) -> &mut KeyAction {
        self.keys
            .entry(key_idx)
            .or_insert_with(|| KeyAction::new("system", "System", "custom", ""))
    }

    pub fn get_or_create_dial(&mut self, dial_idx: usize) -> &mut KeyAction {
        self.dial_actions.entry(dial_idx).or_insert_with(|| {
            let mut a = KeyAction::new("system", "System", "dial", "");
            a.appearance.vertical_align = crate::models::TextVerticalAlign::Center;
            a
        })
    }

    pub fn get_or_create_touchscreen(&mut self) -> &mut KeyAction {
        if self.touchscreen_action.is_none() {
            let mut a = KeyAction::new("system", "System", "touchscreen", "");
            a.appearance.vertical_align = crate::models::TextVerticalAlign::Center;
            self.touchscreen_action = Some(a);
        }
        self.touchscreen_action.as_mut().unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_device_kind")]
    pub device_kind: DeviceKind,
    pub current_page_idx: usize,
    pub pages: Vec<Page>,
    /// Nested folder pages keyed by folder UUID (flat map allows unlimited folder nesting)
    #[serde(default)]
    pub folders: HashMap<String, Page>,
    /// Transient runtime folder navigation stack: Vec<(folder_id, display_name)>
    #[serde(skip)]
    pub folder_stack: Vec<(String, String)>,
}

fn default_device_kind() -> DeviceKind {
    DeviceKind::StreamDeckPlus
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        Self::new_for_device(name, DeviceKind::StreamDeckPlus)
    }

    pub fn new_for_device(name: impl Into<String>, device_kind: DeviceKind) -> Self {
        Self {
            id: glib::uuid_string_random().to_string(),
            name: name.into(),
            device_kind,
            current_page_idx: 0,
            pages: vec![Page::new(0)],
            folders: HashMap::new(),
            folder_stack: Vec::new(),
        }
    }

    pub fn current_page(&self) -> Option<&Page> {
        if let Some((fid, _)) = self.folder_stack.last() {
            if let Some(folder_page) = self.folders.get(fid) {
                return Some(folder_page);
            }
        }
        self.pages.get(self.current_page_idx)
    }

    pub fn current_page_mut(&mut self) -> Option<&mut Page> {
        if let Some((fid, _)) = self.folder_stack.last().cloned() {
            if self.folders.contains_key(&fid) {
                return self.folders.get_mut(&fid);
            }
        }
        self.pages.get_mut(self.current_page_idx)
    }

    pub fn is_in_folder(&self) -> bool {
        !self.folder_stack.is_empty()
    }

    pub fn folder_breadcrumb(&self) -> String {
        let mut parts = vec![format!("Page {}", self.current_page_idx + 1)];
        for (_, name) in &self.folder_stack {
            parts.push(name.clone());
        }
        parts.join("  ›  ")
    }

    pub fn enter_folder(&mut self, folder_id: &str, folder_name: &str) {
        let page = self
            .folders
            .entry(folder_id.to_string())
            .or_insert_with(|| {
                let mut p = Page::new(0);
                p.name = folder_name.to_string();
                p
            });
        // Always ensure Key 0 is the native Back button (matching Elgato Folder convention)
        let has_back = page
            .keys
            .get(&0)
            .map(|a| a.action_id == "ballista.folder_back")
            .unwrap_or(false);
        if !has_back && !page.keys.contains_key(&0) {
            let mut back = KeyAction::new(
                crate::native_sdk::BALLISTA_CORE_PLUGIN_ID,
                crate::native_sdk::BALLISTA_CORE_PLUGIN_NAME,
                "ballista.folder_back",
                "Back",
            );
            back.appearance.show_title = false;
            if let Some(st) = back.states.first_mut() {
                st.show_title = false;
            }
            page.keys.insert(0, back);
        }
        self.folder_stack
            .push((folder_id.to_string(), folder_name.to_string()));
    }

    pub fn exit_folder(&mut self) -> bool {
        self.folder_stack.pop().is_some()
    }

    pub fn exit_all_folders(&mut self) {
        self.folder_stack.clear();
    }

    pub fn add_page(&mut self) -> Option<usize> {
        if self.pages.len() >= 10 {
            return None; // Limit to 10 pages max
        }
        self.folder_stack.clear();
        let new_id = self.pages.len();
        self.pages.push(Page::new(new_id));
        self.current_page_idx = new_id;
        Some(new_id)
    }

    pub fn remove_current_page(&mut self) -> bool {
        if self.pages.len() <= 1 {
            return false; // Preserve at least one page
        }
        self.folder_stack.clear();
        self.pages.remove(self.current_page_idx);
        if self.current_page_idx >= self.pages.len() {
            self.current_page_idx = self.pages.len() - 1;
        }
        true
    }

    pub fn set_page(&mut self, idx: usize) -> bool {
        if idx < self.pages.len() {
            self.folder_stack.clear();
            self.current_page_idx = idx;
            true
        } else {
            false
        }
    }

    pub fn next_page(&mut self) -> bool {
        if self.current_page_idx + 1 < self.pages.len() {
            self.folder_stack.clear();
            self.current_page_idx += 1;
            true
        } else {
            false
        }
    }

    pub fn prev_page(&mut self) -> bool {
        if self.current_page_idx > 0 {
            self.folder_stack.clear();
            self.current_page_idx -= 1;
            true
        } else {
            false
        }
    }
}

/// Persistent container for all device profiles & per-device active profile mappings.
/// Saved atomically to `~/.local/share/ballista/profiles.json` so Ballista can light up
/// every connected Stream Deck in <250ms immediately on desktop login.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileStore {
    pub profiles: Vec<Profile>,
    /// Maps stable device identifier (serial number or fallback device kind name) -> active profile ID
    #[serde(default)]
    pub active_by_device: HashMap<String, String>,
}

impl ProfileStore {
    pub fn store_path() -> PathBuf {
        glib::user_data_dir().join("ballista").join("profiles.json")
    }

    pub fn default_profiles() -> Vec<Profile> {
        vec![
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckPlus),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckPlusXL),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckXL),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckMK2),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckOriginal),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckMini),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckNeo),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckPedal),
            Profile::new_for_device("Default Profile", DeviceKind::StreamDeckStudio),
        ]
    }

    pub fn load_or_default() -> Self {
        let path = Self::store_path();
        let mut store = if path.exists() {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<Self>(&s).ok())
                .unwrap_or_default()
        } else {
            Self::default()
        };

        // Ensure every DeviceKind has at least one profile available
        for default_p in Self::default_profiles() {
            if !store
                .profiles
                .iter()
                .any(|p| p.device_kind == default_p.device_kind)
            {
                store.profiles.push(default_p);
            }
        }

        store
    }

    pub fn save_profiles(profiles: &[Profile], active_by_device: &HashMap<String, String>) {
        let store = Self {
            profiles: profiles.to_vec(),
            active_by_device: active_by_device.clone(),
        };
        store.save();
    }

    pub fn save(&self) {
        let path = Self::store_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if fs::write(&tmp, &json).is_ok() {
                if let Err(e) = fs::rename(&tmp, &path) {
                    tracing::warn!("Atomic rename failed for profiles.json: {e}");
                    let _ = fs::write(&path, &json);
                }
            } else {
                let _ = fs::write(&path, &json);
            }
        }
    }

    /// Resolve the active profile for a specific device (by stable device key and kind)
    pub fn active_profile_for(&self, device_key: &str, kind: DeviceKind) -> Profile {
        if let Some(pid) = self.active_by_device.get(device_key) {
            if let Some(p) = self
                .profiles
                .iter()
                .find(|p| p.id == *pid && p.device_kind == kind)
            {
                return p.clone();
            }
        }
        self.profiles
            .iter()
            .find(|p| p.device_kind == kind)
            .cloned()
            .unwrap_or_else(|| Profile::new_for_device("Default Profile", kind))
    }
}
