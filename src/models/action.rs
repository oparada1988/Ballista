use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextVerticalAlign {
    Top,
    Center,
    Bottom,
}

impl Default for TextVerticalAlign {
    fn default() -> Self {
        Self::Bottom
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyAppearance {
    pub title: String,
    #[serde(default = "default_show_title")]
    pub show_title: bool,
    pub font_family: String,
    pub font_size: u32,
    pub text_color: String,
    pub bg_color: Option<String>,
    pub vertical_align: TextVerticalAlign,
    pub icon_path: Option<String>,
    #[serde(default = "default_icon_scale")]
    pub icon_scale: u32,
}

fn default_show_title() -> bool {
    true
}

fn default_icon_scale() -> u32 {
    100
}

impl Default for KeyAppearance {
    fn default() -> Self {
        Self {
            title: String::new(),
            show_title: true,
            font_family: "Cantarell".into(),
            font_size: 11,
            text_color: "#ffffff".into(),
            bg_color: None,
            vertical_align: TextVerticalAlign::Bottom,
            icon_path: None,
            icon_scale: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyAction {
    pub uuid: String,
    pub plugin_id: String,
    pub plugin_name: String,
    pub action_id: String,
    pub action_name: String,
    pub appearance: KeyAppearance,
    #[serde(default)]
    pub states: Vec<KeyAppearance>,
    #[serde(default)]
    pub current_state: usize,
    pub parameters: HashMap<String, String>,
}

impl KeyAction {
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_name: impl Into<String>,
        action_id: impl Into<String>,
        action_name: impl Into<String>,
    ) -> Self {
        let appearance = KeyAppearance::default();
        Self {
            uuid: glib::uuid_string_random().to_string(),
            plugin_id: plugin_id.into(),
            plugin_name: plugin_name.into(),
            action_id: action_id.into(),
            action_name: action_name.into(),
            states: vec![appearance.clone()],
            current_state: 0,
            appearance,
            parameters: HashMap::new(),
        }
    }

    pub fn active_appearance(&self) -> &KeyAppearance {
        if !self.states.is_empty() {
            let idx = self.current_state.min(self.states.len() - 1);
            &self.states[idx]
        } else {
            &self.appearance
        }
    }

    pub fn active_appearance_mut(&mut self) -> &mut KeyAppearance {
        if self.states.is_empty() {
            self.states.push(self.appearance.clone());
        }
        let idx = self.current_state.min(self.states.len() - 1);
        &mut self.states[idx]
    }

    pub fn state_appearance(&self, state_idx: usize) -> Option<&KeyAppearance> {
        self.states.get(state_idx).or(Some(&self.appearance))
    }

    pub fn state_appearance_mut(&mut self, state_idx: usize) -> &mut KeyAppearance {
        if self.states.is_empty() {
            self.states.push(self.appearance.clone());
        }
        while self.states.len() <= state_idx {
            self.states.push(self.states.last().cloned().unwrap_or_default());
        }
        &mut self.states[state_idx]
    }

    pub fn toggle_state(&mut self) -> usize {
        if self.states.len() > 1 {
            self.current_state = (self.current_state + 1) % self.states.len();
            self.appearance = self.states[self.current_state].clone();
        }
        self.current_state
    }

    pub fn set_active_state(&mut self, idx: usize) {
        if !self.states.is_empty() {
            self.current_state = idx.min(self.states.len() - 1);
            self.appearance = self.states[self.current_state].clone();
        }
    }
}
