use super::action::KeyAction;
use super::device::DeviceKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
        self.keys.entry(key_idx).or_insert_with(|| {
            KeyAction::new("system", "System", "custom", "")
        })
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
        }
    }

    pub fn current_page(&self) -> Option<&Page> {
        self.pages.get(self.current_page_idx)
    }

    pub fn current_page_mut(&mut self) -> Option<&mut Page> {
        self.pages.get_mut(self.current_page_idx)
    }

    pub fn add_page(&mut self) -> Option<usize> {
        if self.pages.len() >= 10 {
            return None; // Limit to 10 pages max
        }
        let new_id = self.pages.len();
        self.pages.push(Page::new(new_id));
        self.current_page_idx = new_id;
        Some(new_id)
    }

    pub fn remove_current_page(&mut self) -> bool {
        if self.pages.len() <= 1 {
            return false; // Preserve at least one page
        }
        self.pages.remove(self.current_page_idx);
        if self.current_page_idx >= self.pages.len() {
            self.current_page_idx = self.pages.len() - 1;
        }
        true
    }

    pub fn set_page(&mut self, idx: usize) -> bool {
        if idx < self.pages.len() {
            self.current_page_idx = idx;
            true
        } else {
            false
        }
    }

    pub fn next_page(&mut self) -> bool {
        if self.current_page_idx + 1 < self.pages.len() {
            self.current_page_idx += 1;
            true
        } else {
            false
        }
    }

    pub fn prev_page(&mut self) -> bool {
        if self.current_page_idx > 0 {
            self.current_page_idx -= 1;
            true
        } else {
            false
        }
    }
}
