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
    pub parameters: HashMap<String, String>,
}

impl KeyAction {
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_name: impl Into<String>,
        action_id: impl Into<String>,
        action_name: impl Into<String>,
    ) -> Self {
        Self {
            uuid: glib::uuid_string_random().to_string(),
            plugin_id: plugin_id.into(),
            plugin_name: plugin_name.into(),
            action_id: action_id.into(),
            action_name: action_name.into(),
            appearance: KeyAppearance::default(),
            parameters: HashMap::new(),
        }
    }
}
