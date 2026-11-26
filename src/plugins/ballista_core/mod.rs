pub mod clock;
pub mod deck_control;
pub mod folder;
pub mod hotkey;
pub mod launcher;
pub mod media;
pub mod multi_action;
pub mod run_command;
pub mod system_metrics;
pub mod timer_dial;

use crate::native_sdk::{
    ActionDescriptor, BallistaAction, BallistaPlugin, TargetCaps, BALLISTA_CORE_PLUGIN_ID,
    BALLISTA_CORE_PLUGIN_NAME,
};

/// Ballista's official native Rust built-in plugin suite (`com.ballista.core`).
/// Serves as both the first-party exclusive action catalog ("Ballista") and the reference
/// implementation of Ballista's Native Rust Plugin SDK (`src/native_sdk/`).
pub struct BallistaCorePlugin;

impl BallistaCorePlugin {
    pub fn new() -> Self {
        Self
    }
}

impl BallistaPlugin for BallistaCorePlugin {
    fn id(&self) -> &'static str {
        BALLISTA_CORE_PLUGIN_ID
    }

    fn name(&self) -> &'static str {
        BALLISTA_CORE_PLUGIN_NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn actions(&self) -> Vec<ActionDescriptor> {
        vec![
            ActionDescriptor {
                id: "ballista.system_metrics".into(),
                name: "System Metrics".into(),
                description: "Live CPU, RAM, temperature, or disk usage as a percentage, sparkline graph, or gauge.".into(),
                icon_name: "utilities-system-monitor-symbolic".into(),
                caps: TargetCaps::KEYS_AND_DIALS,
                default_title: String::new(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.run_command".into(),
                name: "Run Command".into(),
                description: "Execute a shell or terminal command with optional live stdout feedback.".into(),
                icon_name: "utilities-terminal-symbolic".into(),
                caps: TargetCaps::KEYS_ONLY,
                default_title: "Command".into(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.clock".into(),
                name: "Clock".into(),
                description: "Crisp native Digital or Analog clock with 12/24h, date, and timezone support.".into(),
                icon_name: "preferences-system-time-symbolic".into(),
                caps: TargetCaps::KEYS_AND_DIALS,
                default_title: String::new(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.media".into(),
                name: "Media".into(),
                description: "Native MPRIS2 media control (Play/Pause, Next, Previous, Stop, and Track Info with album art).".into(),
                icon_name: "audio-x-generic-symbolic".into(),
                caps: TargetCaps::KEYS_AND_DIALS,
                default_title: String::new(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.folder".into(),
                name: "Folder".into(),
                description: "Group actions into nested folders with an automatic Back key.".into(),
                icon_name: "folder-symbolic".into(),
                caps: TargetCaps::KEYS_ONLY,
                default_title: "Folder".into(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.multi_action".into(),
                name: "Multi-Action".into(),
                description: "Run an ordered sequence of actions and delays with one press (supports 2-state toggle).".into(),
                icon_name: "view-list-symbolic".into(),
                caps: TargetCaps::KEYS_ONLY,
                default_title: "Multi-Action".into(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.hotkey".into(),
                name: "Hotkey & Text".into(),
                description: "Send keyboard shortcuts or type text on keys, or bind 3 rotary jog shortcuts (CCW / Press / CW) on dials.".into(),
                icon_name: "input-keyboard-symbolic".into(),
                caps: TargetCaps::KEYS_AND_DIALS,
                default_title: "Hotkey".into(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.launcher".into(),
                name: "Launcher".into(),
                description: "Launch an installed desktop application (visual icon picker), website URL, or file/folder.".into(),
                icon_name: "view-app-grid-symbolic".into(),
                caps: TargetCaps::KEYS_ONLY,
                default_title: "Launcher".into(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.deck_control".into(),
                name: "Deck Control".into(),
                description: "Switch pages, change profiles, adjust hardware brightness, or sleep/wake your deck.".into(),
                icon_name: "display-brightness-symbolic".into(),
                caps: TargetCaps::KEYS_AND_DIALS,
                default_title: String::new(),
                show_in_sidebar: true,
            },
            ActionDescriptor {
                id: "ballista.timer_dial".into(),
                name: "Timer & Pomodoro".into(),
                description: "Dial-exclusive countdown timer & Pomodoro widget: rotate to set duration, press to start/pause, tap to reset.".into(),
                icon_name: "alarm-symbolic".into(),
                caps: TargetCaps::DIALS_ONLY,
                default_title: String::new(),
                show_in_sidebar: true,
            },
            // Internal folder navigation Back action (auto-placed on Key 0 inside folders)
            ActionDescriptor {
                id: "ballista.folder_back".into(),
                name: "Back".into(),
                description: "Return to parent page or folder.".into(),
                icon_name: "go-previous-symbolic".into(),
                caps: TargetCaps::KEYS_ONLY,
                default_title: String::new(),
                show_in_sidebar: false,
            },
        ]
    }

    fn create_action(&self, action_id: &str) -> Option<Box<dyn BallistaAction>> {
        match action_id {
            "ballista.system_metrics" => Some(Box::new(system_metrics::SystemMetricsAction::new())),
            "ballista.run_command" => Some(Box::new(run_command::RunCommandAction::new())),
            "ballista.clock" => Some(Box::new(clock::ClockAction::new())),
            "ballista.media" => Some(Box::new(media::MediaAction::new())),
            "ballista.folder" => Some(Box::new(folder::FolderAction::new())),
            "ballista.folder_back" => Some(Box::new(folder::FolderBackAction::new())),
            "ballista.multi_action" => Some(Box::new(multi_action::MultiActionRunner::new())),
            "ballista.hotkey" => Some(Box::new(hotkey::HotkeyAction::new())),
            "ballista.launcher" => Some(Box::new(launcher::LauncherAction::new())),
            "ballista.deck_control" => Some(Box::new(deck_control::DeckControlAction::new())),
            "ballista.timer_dial" => Some(Box::new(timer_dial::TimerDialAction::new())),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DeviceKind, Profile};
    use crate::native_sdk::{ActionContext, ConfigContext, NativePluginHost, SlotSurface};
    use std::collections::HashMap;

    #[test]
    fn ballista_core_registers_all_10_visible_actions_with_strict_surface_caps() {
        let plugin = BallistaCorePlugin::new();
        let descs = plugin.actions();
        let visible: Vec<&ActionDescriptor> = descs.iter().filter(|d| d.show_in_sidebar).collect();
        assert_eq!(visible.len(), 10);

        let cap_of = |id: &str| {
            descs
                .iter()
                .find(|d| d.id == id)
                .unwrap_or_else(|| panic!("Missing action {id}"))
                .caps
        };

        // Dual-Surface Showcase Actions (Keys + Dials)
        assert_eq!(cap_of("ballista.media"), TargetCaps::KEYS_AND_DIALS);
        assert_eq!(cap_of("ballista.system_metrics"), TargetCaps::KEYS_AND_DIALS);
        assert_eq!(cap_of("ballista.clock"), TargetCaps::KEYS_AND_DIALS);
        assert_eq!(cap_of("ballista.deck_control"), TargetCaps::KEYS_AND_DIALS);
        assert_eq!(cap_of("ballista.hotkey"), TargetCaps::KEYS_AND_DIALS);

        // Strict Key-Only Actions
        assert_eq!(cap_of("ballista.folder"), TargetCaps::KEYS_ONLY);
        assert_eq!(cap_of("ballista.launcher"), TargetCaps::KEYS_ONLY);
        assert_eq!(cap_of("ballista.run_command"), TargetCaps::KEYS_ONLY);
        assert_eq!(cap_of("ballista.multi_action"), TargetCaps::KEYS_ONLY);

        // Strict Dial-Only Action
        assert_eq!(cap_of("ballista.timer_dial"), TargetCaps::DIALS_ONLY);
        assert!(!cap_of("ballista.timer_dial").supports(SlotSurface::Key));
        assert!(cap_of("ballista.timer_dial").supports(SlotSurface::Dial));

        // Hidden internal Back action is also registered and instantiable
        assert!(descs
            .iter()
            .any(|d| d.id == "ballista.folder_back" && !d.show_in_sidebar));
        for d in &descs {
            assert!(
                plugin.create_action(&d.id).is_some(),
                "Action {} failed to instantiate",
                d.id
            );
        }
    }

    #[test]
    fn all_native_actions_render_cairo_and_build_config_rows_on_both_surfaces() {
        let host = NativePluginHost::new();
        host.register_plugin(Box::new(BallistaCorePlugin::new()));
        let plugin = BallistaCorePlugin::new();

        let key_surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 120, 120).unwrap();
        let key_cr = cairo::Context::new(&key_surface).unwrap();
        let dial_surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 100).unwrap();
        let dial_cr = cairo::Context::new(&dial_surface).unwrap();

        let empty_settings: HashMap<String, serde_json::Value> = HashMap::new();
        let profile_names = vec!["Default".to_string()];

        let key_cfg = ConfigContext {
            slot: "key_0",
            surface: SlotSurface::Key,
            settings: &empty_settings,
            available_profiles: &profile_names,
            active_profile_name: "Default",
            current_page_idx: 0,
            total_pages: 3,
            current_brightness: 80,
        };
        let dial_cfg = ConfigContext {
            slot: "dial_0",
            surface: SlotSurface::Dial,
            settings: &empty_settings,
            available_profiles: &profile_names,
            active_profile_name: "Default",
            current_page_idx: 0,
            total_pages: 3,
            current_brightness: 80,
        };

        for d in plugin.actions() {
            let act = plugin.create_action(&d.id).unwrap();
            if d.caps.keys {
                let _rows = act.config_rows(&key_cfg);
                let _drew = act.render_visual_for_surface(&key_cr, 120.0, 120.0, SlotSurface::Key);
            }
            if d.caps.dials {
                let _rows = act.config_rows(&dial_cfg);
                let drew =
                    act.render_visual_for_surface(&dial_cr, 200.0, 100.0, SlotSurface::Dial);
                assert!(drew, "Dial-capable action {} must paint a 200x100 card", d.id);
            }
        }
    }

    #[test]
    fn timer_dial_rotates_starts_ticks_and_resets_cleanly() {
        let mut timer = timer_dial::TimerDialAction::new();
        let mut settings = HashMap::new();
        let profiles = vec!["Default".to_string()];

        {
            let mut ctx = ActionContext::new("dial_0", &mut settings, &profiles, "Default", 0, 1, 80);
            timer.on_load(&mut ctx);
            assert!(timer.tick_interval().is_none()); // idle initially

            // Rotate +2 minutes (default 5m -> 7m = 420s)
            timer.on_dial_rotate(&mut ctx, 2);
            assert_eq!(ctx.get_f64("target_secs", 0.0) as u32, 420);

            // Press to start -> 1Hz tick activates
            timer.on_dial_press(&mut ctx);
            assert!(timer.tick_interval().is_some());

            // Tick once
            timer.on_tick(&mut ctx);

            // Tap left third of touchscreen -> resets back to idle
            timer.on_touch_tap(&mut ctx, 0.12, 0.5);
            assert!(timer.tick_interval().is_none());
        }
    }

    #[test]
    fn nested_folders_auto_populate_back_key_and_preserve_backwards_compat() {
        let mut prof = Profile::new_for_device("Test", DeviceKind::StreamDeckPlus);
        assert!(prof.folder_stack.is_empty());

        prof.enter_folder("folder_1", "Streaming");
        assert_eq!(prof.folder_stack.len(), 1);
        let page = prof.current_page().expect("folder page must exist");
        let back = page.keys.get(&0).expect("Key 0 must auto-populate Back");
        assert_eq!(back.plugin_id, BALLISTA_CORE_PLUGIN_ID);
        assert_eq!(back.action_id, "ballista.folder_back");

        // Nested subfolder
        prof.enter_folder("folder_2", "OBS");
        assert_eq!(prof.folder_stack.len(), 2);
        assert!(prof.exit_folder());
        assert_eq!(prof.folder_stack.len(), 1);
        assert!(prof.exit_folder());
        assert!(prof.folder_stack.is_empty());
        assert!(!prof.exit_folder());

        // Serde round-trip persists folders cleanly while skipping transient folder_stack
        let json = serde_json::to_string(&prof).unwrap();
        let restored: Profile = serde_json::from_str(&json).unwrap();
        assert!(restored.folders.contains_key("folder_1"));
        assert!(restored.folders.contains_key("folder_2"));
        assert!(restored.folder_stack.is_empty());
    }
}
