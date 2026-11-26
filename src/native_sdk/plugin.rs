#![allow(dead_code)]

use crate::core::plugin_bridge::BridgeConfigRow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub const BALLISTA_CORE_PLUGIN_ID: &str = "com.ballista.core";
pub const BALLISTA_CORE_PLUGIN_NAME: &str = "Ballista";

/// Identifies which hardware surface an action slot lives on, allowing a single
/// Ballista-native action to adapt its inspector UI, Cairo rendering, and gesture bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SlotSurface {
    Key,
    Dial,
    Touchscreen,
}

impl SlotSurface {
    pub fn from_slot(slot: &str) -> Self {
        if slot.starts_with("dial_") {
            Self::Dial
        } else if slot == "touchscreen" {
            Self::Touchscreen
        } else {
            Self::Key
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetCaps {
    pub keys: bool,
    pub dials: bool,
    pub touchscreen: bool,
}

impl Default for TargetCaps {
    fn default() -> Self {
        Self::KEYS_AND_DIALS
    }
}

impl TargetCaps {
    pub const KEYS_ONLY: Self = Self {
        keys: true,
        dials: false,
        touchscreen: false,
    };
    pub const DIALS_ONLY: Self = Self {
        keys: false,
        dials: true,
        touchscreen: false,
    };
    pub const KEYS_AND_DIALS: Self = Self {
        keys: true,
        dials: true,
        touchscreen: false,
    };
    pub const TOUCHSCREEN_ONLY: Self = Self {
        keys: false,
        dials: false,
        touchscreen: true,
    };
    pub const ALL: Self = Self {
        keys: true,
        dials: true,
        touchscreen: true,
    };

    pub fn supports(&self, surface: SlotSurface) -> bool {
        match surface {
            SlotSurface::Key => self.keys,
            SlotSurface::Dial => self.dials,
            SlotSurface::Touchscreen => self.touchscreen,
        }
    }

    pub fn badge_label(&self) -> &'static str {
        match (self.keys, self.dials, self.touchscreen) {
            (true, true, _) => "KEY · DIAL",
            (true, false, false) => "KEY",
            (false, true, false) => "DIAL",
            (false, false, true) => "TOUCH",
            _ => "ALL",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActionDescriptor {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_name: String,
    pub caps: TargetCaps,
    pub default_title: String,
    /// If true, the action appears in the sidebar catalog; internal actions (like folder Back) set false.
    pub show_in_sidebar: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPos {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageNav {
    Next,
    Prev,
    Index(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrightnessOp {
    Set(u8),
    Delta(i16),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MultiActionStep {
    Delay { ms: u64 },
    RunCommand { command: String },
    Hotkey { keys: String },
    TypeText { text: String, press_enter: bool },
    LaunchApp { desktop_id: String, app_name: String },
    OpenUrl { url: String },
    Media { action: String },
    SwitchPage { target: String },
    Brightness { percent: u8 },
    TriggerKey { key_index: usize },
}

impl MultiActionStep {
    pub fn summary(&self) -> String {
        match self {
            Self::Delay { ms } => format!("Delay {} ms", ms),
            Self::RunCommand { command } => {
                let c = if command.is_empty() { "(empty)" } else { command.as_str() };
                format!("Run: {}", c)
            }
            Self::Hotkey { keys } => {
                let k = if keys.is_empty() { "(unset)" } else { keys.as_str() };
                format!("Hotkey: {}", k)
            }
            Self::TypeText { text, .. } => {
                let t = if text.is_empty() { "(empty)" } else { text.as_str() };
                format!("Type: \"{}\"", t)
            }
            Self::LaunchApp { app_name, desktop_id } => {
                let label = if !app_name.is_empty() { app_name.as_str() } else { desktop_id.as_str() };
                format!("Launch: {}", if label.is_empty() { "(choose app)" } else { label })
            }
            Self::OpenUrl { url } => {
                format!("Open URL: {}", if url.is_empty() { "(empty)" } else { url.as_str() })
            }
            Self::Media { action } => format!("Media: {}", action),
            Self::SwitchPage { target } => format!("Switch Page: {}", target),
            Self::Brightness { percent } => format!("Brightness: {}%", percent),
            Self::TriggerKey { key_index } => format!("Trigger Key {}", key_index + 1),
        }
    }
}

#[derive(Debug, Clone)]
pub enum HostCommand {
    SetLabel {
        position: LabelPos,
        text: String,
        font_size: Option<u32>,
        color: Option<String>,
    },
    SetMedia {
        path: Option<String>,
        icon_name: Option<String>,
    },
    SetTitle(String),
    SetBackgroundColor(Option<String>),
    EnterFolder {
        folder_id: String,
        folder_name: String,
    },
    ExitFolder,
    SwitchPage(PageNav),
    SwitchProfile(String),
    SetBrightness(BrightnessOp),
    ToggleSleep,
    ExecuteMultiAction {
        steps: Vec<MultiActionStep>,
    },
    OpenAppPicker,
    OpenFilePicker {
        directories_only: bool,
    },
    RefreshConfigRows,
    DirtyVisual,
    PersistSettings,
}

#[derive(Debug, Clone, Default)]
pub struct ConfigChange {
    pub selected: Option<usize>,
    pub active: Option<bool>,
    pub text: Option<String>,
    pub value: Option<f64>,
    pub color: Option<String>,
}

pub struct ConfigContext<'a> {
    pub slot: &'a str,
    pub surface: SlotSurface,
    pub settings: &'a HashMap<String, Value>,
    pub available_profiles: &'a [String],
    pub active_profile_name: &'a str,
    pub current_page_idx: usize,
    pub total_pages: usize,
    pub current_brightness: u8,
}

impl<'a> ConfigContext<'a> {
    pub fn get_str(&self, key: &str, default: &str) -> String {
        self.settings
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.settings
            .get(key)
            .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
            .unwrap_or(default)
    }

    pub fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.settings
            .get(key)
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(default)
    }

    pub fn get_usize(&self, key: &str, default: usize) -> usize {
        self.settings
            .get(key)
            .and_then(|v| {
                v.as_u64()
                    .map(|n| n as usize)
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(default)
    }
}

pub struct ActionContext<'a> {
    pub slot: &'a str,
    pub surface: SlotSurface,
    pub settings: &'a mut HashMap<String, Value>,
    pub available_profiles: &'a [String],
    pub active_profile_name: &'a str,
    pub current_page_idx: usize,
    pub total_pages: usize,
    pub current_brightness: u8,
    pub commands: Vec<HostCommand>,
}

impl<'a> ActionContext<'a> {
    pub fn new(
        slot: &'a str,
        settings: &'a mut HashMap<String, Value>,
        available_profiles: &'a [String],
        active_profile_name: &'a str,
        current_page_idx: usize,
        total_pages: usize,
        current_brightness: u8,
    ) -> Self {
        Self {
            slot,
            surface: SlotSurface::from_slot(slot),
            settings,
            available_profiles,
            active_profile_name,
            current_page_idx,
            total_pages: total_pages.max(1),
            current_brightness,
            commands: Vec::new(),
        }
    }

    pub fn get_str(&self, key: &str, default: &str) -> String {
        self.settings
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| default.to_string())
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.settings
            .get(key)
            .and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))
            .unwrap_or(default)
    }

    pub fn get_f64(&self, key: &str, default: f64) -> f64 {
        self.settings
            .get(key)
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(default)
    }

    pub fn get_usize(&self, key: &str, default: usize) -> usize {
        self.settings
            .get(key)
            .and_then(|v| {
                v.as_u64()
                    .map(|n| n as usize)
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(default)
    }

    pub fn set_setting(&mut self, key: impl Into<String>, value: Value) {
        self.settings.insert(key.into(), value);
        self.push(HostCommand::PersistSettings);
    }

    pub fn push(&mut self, cmd: HostCommand) {
        self.commands.push(cmd);
    }

    pub fn mark_dirty(&mut self) {
        self.commands.push(HostCommand::DirtyVisual);
    }
}

/// Helper builder for declarative inspector configuration rows shared with Ballista's PropertyInspector.
pub fn make_row(index: usize, row_type: &str, title: &str) -> BridgeConfigRow {
    BridgeConfigRow {
        index,
        row_type: row_type.to_string(),
        title: title.to_string(),
        subtitle: String::new(),
        visible: true,
        sensitive: true,
        editable: true,
        options: Vec::new(),
        selected: 0,
        active: false,
        text: String::new(),
        value: 0.0,
        min: 0.0,
        max: 100.0,
        step: 1.0,
        digits: 0,
        color: "#9146ff".to_string(),
        button_icon: String::new(),
        button_label: String::new(),
        suggested: false,
    }
}

/// Core trait implemented by every Ballista-native Rust action.
pub trait BallistaAction: Send {
    fn on_load(&mut self, _ctx: &mut ActionContext) {}
    fn on_key_down(&mut self, _ctx: &mut ActionContext) {}
    fn on_key_up(&mut self, _ctx: &mut ActionContext) {}
    fn on_dial_rotate(&mut self, _ctx: &mut ActionContext, _delta: i32) {}
    fn on_dial_press(&mut self, _ctx: &mut ActionContext) {}
    fn on_dial_release(&mut self, _ctx: &mut ActionContext) {}

    /// Invoked when the user taps directly on a Stream Deck + / + XL touchscreen dial segment.
    /// `norm_x` and `norm_y` are normalized `0.0..=1.0` coordinates inside the 200x100 segment.
    /// Defaults to `on_dial_press` for actions that do not define custom touch hit-zones.
    fn on_touch_tap(&mut self, ctx: &mut ActionContext, _norm_x: f64, _norm_y: f64) {
        self.on_dial_press(ctx);
    }

    fn tick_interval(&self) -> Option<Duration> {
        None
    }
    fn on_tick(&mut self, _ctx: &mut ActionContext) {}

    fn config_rows(&self, _ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        Vec::new()
    }
    fn on_config_changed(
        &mut self,
        _ctx: &mut ActionContext,
        _row_idx: usize,
        _change: ConfigChange,
    ) {
    }
    fn on_button_clicked(&mut self, _ctx: &mut ActionContext, _row_idx: usize) {}

    /// Direct vector/pixel renderer onto Cairo. Returns `true` if the action painted custom artwork
    /// (allowing crisp live graphs, analog clocks, album art cards on both UI and hardware without temp PNGs).
    fn render_visual(&self, _cr: &cairo::Context, _w: f64, _h: f64) -> bool {
        false
    }

    /// Surface-aware Cairo renderer (`Key`, `Dial`, or `Touchscreen`).
    /// Dual-surface actions can override this to paint distinct 1:1 key vs 2:1 dial-card layouts.
    fn render_visual_for_surface(
        &self,
        cr: &cairo::Context,
        w: f64,
        h: f64,
        _surface: SlotSurface,
    ) -> bool {
        self.render_visual(cr, w, h)
    }
}

/// Core trait implemented by every Ballista-native Rust plugin package.
pub trait BallistaPlugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn version(&self) -> &'static str;
    fn actions(&self) -> Vec<ActionDescriptor>;
    fn create_action(&self, action_id: &str) -> Option<Box<dyn BallistaAction>>;
}
