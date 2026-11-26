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
    #[serde(default)]
    pub top_label: Option<String>,
    #[serde(default)]
    pub center_label: Option<String>,
    #[serde(default)]
    pub bottom_label: Option<String>,
    #[serde(default)]
    pub custom_title_override: bool,
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
            top_label: None,
            center_label: None,
            bottom_label: None,
            custom_title_override: false,
        }
    }
}

impl KeyAppearance {
    pub fn has_plugin_labels(&self) -> bool {
        self.top_label.is_some() || self.center_label.is_some() || self.bottom_label.is_some()
    }

    pub fn combined_display_summary(&self) -> String {
        if !self.show_title {
            return String::new();
        }
        if self.has_plugin_labels() {
            let mut parts = Vec::new();
            if let Some(ref t) = self.top_label {
                if !t.is_empty() { parts.push(t.as_str()); }
            }
            if let Some(ref c) = self.center_label {
                if !c.is_empty() { parts.push(c.as_str()); }
            }
            if let Some(ref b) = self.bottom_label {
                if !b.is_empty() { parts.push(b.as_str()); }
            }
            return parts.join("\n");
        }
        if self.show_title {
            self.title.clone()
        } else {
            String::new()
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
    #[serde(default)]
    pub raw_settings: HashMap<String, serde_json::Value>,
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
            raw_settings: HashMap::new(),
        }
    }

    pub fn merged_settings_json(&self) -> HashMap<String, serde_json::Value> {
        let mut out: HashMap<String, serde_json::Value> = self
            .parameters
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        for (k, v) in &self.raw_settings {
            out.insert(k.clone(), v.clone());
        }
        out
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
