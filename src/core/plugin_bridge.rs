use std::process::{Command, Stdio, Child, ChildStdin};
use std::io::{BufReader, BufRead, Write};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct BridgeConfigRow {
    pub index: usize,
    #[serde(rename = "type")]
    pub row_type: String, // "ComboRow", "SwitchRow", "EntryRow", "PasswordRow", "SpinRow", "ScaleRow", "ColorRow", "ButtonRow", "ActionRow"
    pub title: String,
    #[serde(default)]
    pub subtitle: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_true")]
    pub sensitive: bool,
    #[serde(default = "default_true")]
    pub editable: bool,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub selected: usize,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    #[serde(default)]
    pub step: f64,
    #[serde(default)]
    pub digits: u32,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub button_icon: String,
    #[serde(default)]
    pub button_label: String,
    #[serde(default)]
    pub suggested: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BridgeActionMeta {
    pub action_id: String,
    pub action_name: String,
    #[serde(default)]
    pub icon_path: String,
    #[serde(default)]
    pub icon_name: String,
    #[serde(default = "default_true")]
    pub supports_key: bool,
    #[serde(default = "default_true")]
    pub supports_dial: bool,
    #[serde(default = "default_true")]
    pub supports_touchscreen: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct BridgeCommand {
    pub cmd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_button: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "event")]
pub enum BridgeEvent {
    #[serde(rename = "ready")]
    Ready { version: String },
    #[serde(rename = "plugin_initialized")]
    PluginInitialized {
        plugin_id: String,
        plugin_name: String,
        #[serde(default)]
        has_settings: bool,
        #[serde(default)]
        selector_icon_path: String,
        #[serde(default)]
        selector_icon_name: String,
        #[serde(default)]
        actions: Vec<BridgeActionMeta>,
    },
    #[serde(rename = "set_media")]
    SetMedia {
        slot: String,
        media_path: String,
        #[serde(default)]
        size: Option<f64>,
        #[serde(default)]
        valign: Option<f64>,
        #[serde(default)]
        halign: Option<f64>,
    },
    #[serde(rename = "set_label")]
    SetLabel {
        slot: String,
        text: String,
        position: Option<String>,
        #[serde(default)]
        color: Option<serde_json::Value>,
        #[serde(default)]
        font_size: Option<u32>,
        #[serde(default)]
        font_family: Option<String>,
    },
    #[serde(rename = "set_state")]
    SetState { slot: String, state: usize },
    #[serde(rename = "set_settings")]
    SetSettings { slot: String, settings: HashMap<String, serde_json::Value> },
    #[serde(rename = "set_background_color")]
    SetBackgroundColor { slot: String, color: serde_json::Value },
    #[serde(rename = "action_config_rows")]
    ActionConfigRows { slot: String, rows: Vec<BridgeConfigRow> },
    #[serde(rename = "plugin_settings_rows")]
    PluginSettingsRows { plugin_id: String, rows: Vec<BridgeConfigRow> },
    #[serde(rename = "show_error")]
    ShowError { slot: String, #[serde(default)] duration: i32 },
    #[serde(rename = "hide_error")]
    HideError { slot: String },
    #[serde(rename = "notification")]
    Notification {
        #[serde(default)]
        icon: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
    },
    #[serde(rename = "cmd_result")]
    CmdResult { status: String },
    #[serde(other)]
    Unknown,
}

pub type BridgeEventCallback = Arc<dyn Fn(BridgeEvent) + Send + Sync + 'static>;

#[derive(Clone)]
pub struct PluginBridge {
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    child: Arc<Mutex<Option<Child>>>,
    event_callback: Arc<Mutex<Option<BridgeEventCallback>>>,
    pending_events: Arc<Mutex<Vec<BridgeEvent>>>,
}

impl PluginBridge {
    pub fn new() -> Self {
        Self {
            stdin: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
            event_callback: Arc::new(Mutex::new(None)),
            pending_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn set_event_callback<F>(&self, cb: F)
    where
        F: Fn(BridgeEvent) + Send + Sync + 'static,
    {
        let cb_arc: BridgeEventCallback = Arc::new(cb);
        let drained = {
            let mut cb_guard = self.event_callback.lock().unwrap();
            *cb_guard = Some(cb_arc.clone());
            let mut pend = self.pending_events.lock().unwrap();
            std::mem::take(&mut *pend)
        };
        for ev in drained {
            cb_arc(ev);
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let runner_script = Self::locate_runner_script()?;
        let python_bin = Self::locate_python_executable();
        tracing::info!(
            "Starting Ballista StreamController Python Bridge: {} (python={})",
            runner_script.display(),
            python_bin.display()
        );

        let mut child = Command::new(&python_bin)
            .arg(&runner_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to spawn Python bridge with {}: {e}", python_bin.display()))?;

        let stdout = child.stdout.take().ok_or_else(|| "Failed to capture bridge stdout".to_string())?;
        let stdin = child.stdin.take().ok_or_else(|| "Failed to capture bridge stdin".to_string())?;

        *self.stdin.lock().unwrap() = Some(stdin);
        *self.child.lock().unwrap() = Some(child);

        let cb_lock = self.event_callback.clone();
        let pending_lock = self.pending_events.clone();

        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        let l_trimmed = l.trim();
                        if l_trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(event) = serde_json::from_str::<BridgeEvent>(l_trimmed) {
                            let cb_opt = cb_lock.lock().unwrap().clone();
                            if let Some(cb) = cb_opt {
                                cb(event);
                            } else {
                                pending_lock.lock().unwrap().push(event);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Python bridge stdout read error: {e}");
                        break;
                    }
                }
            }
            tracing::info!("Python bridge stdout reader thread terminated");
        });

        Ok(())
    }

    pub fn send_command(&self, cmd: BridgeCommand) {
        if let Ok(mut lock) = self.stdin.lock() {
            if let Some(ref mut stdin) = *lock {
                if let Ok(json_str) = serde_json::to_string(&cmd) {
                    let _ = stdin.write_all(json_str.as_bytes());
                    let _ = stdin.write_all(b"\n");
                    let _ = stdin.flush();
                }
            }
        }
    }

    pub fn init_plugin(&self, plugin_id: &str, plugin_path: &Path) {
        self.send_command(BridgeCommand {
            cmd: "init_plugin".into(),
            plugin_id: Some(plugin_id.into()),
            plugin_path: Some(plugin_path.to_string_lossy().to_string()),
            ..Default::default()
        });
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        self.send_command(BridgeCommand {
            cmd: "uninstall_plugin".into(),
            plugin_id: Some(plugin_id.into()),
            ..Default::default()
        });
    }

    pub fn create_action(&self, slot: &str, plugin_id: &str, action_id: &str, settings: HashMap<String, serde_json::Value>) {
        self.send_command(BridgeCommand {
            cmd: "create_action".into(),
            plugin_id: Some(plugin_id.into()),
            slot: Some(slot.into()),
            action_id: Some(action_id.into()),
            settings: Some(settings),
            ..Default::default()
        });
    }

    pub fn destroy_action(&self, slot: &str) {
        self.send_command(BridgeCommand {
            cmd: "destroy_action".into(),
            slot: Some(slot.into()),
            ..Default::default()
        });
    }

    pub fn clear_all_actions(&self) {
        self.send_command(BridgeCommand {
            cmd: "clear_all_actions".into(),
            ..Default::default()
        });
    }

    pub fn update_action_settings(&self, slot: &str, settings: HashMap<String, serde_json::Value>) {
        self.send_command(BridgeCommand {
            cmd: "update_action_settings".into(),
            slot: Some(slot.into()),
            settings: Some(settings),
            ..Default::default()
        });
    }

    pub fn get_action_config_rows(&self, slot: &str) {
        self.send_command(BridgeCommand {
            cmd: "get_action_config_rows".into(),
            slot: Some(slot.into()),
            ..Default::default()
        });
    }

    pub fn set_action_config_row(
        &self,
        slot: &str,
        index: usize,
        selected: Option<usize>,
        active: Option<bool>,
        text: Option<String>,
        color: Option<String>,
        value: Option<f64>,
    ) {
        self.send_command(BridgeCommand {
            cmd: "set_action_config_row".into(),
            slot: Some(slot.into()),
            index: Some(index),
            selected,
            active,
            text,
            color,
            value,
            ..Default::default()
        });
    }

    pub fn trigger_action_config_button(&self, slot: &str, index: usize) {
        self.send_command(BridgeCommand {
            cmd: "trigger_action_config_button".into(),
            slot: Some(slot.into()),
            index: Some(index),
            trigger_button: Some(true),
            ..Default::default()
        });
    }

    pub fn get_plugin_settings_rows(&self, plugin_id: &str) {
        self.send_command(BridgeCommand {
            cmd: "get_plugin_settings_rows".into(),
            plugin_id: Some(plugin_id.into()),
            ..Default::default()
        });
    }

    pub fn set_plugin_settings_row(
        &self,
        plugin_id: &str,
        index: usize,
        selected: Option<usize>,
        active: Option<bool>,
        text: Option<String>,
        color: Option<String>,
        value: Option<f64>,
    ) {
        self.send_command(BridgeCommand {
            cmd: "set_plugin_settings_row".into(),
            plugin_id: Some(plugin_id.into()),
            index: Some(index),
            selected,
            active,
            text,
            color,
            value,
            ..Default::default()
        });
    }

    pub fn trigger_plugin_settings_button(&self, plugin_id: &str, index: usize) {
        self.send_command(BridgeCommand {
            cmd: "trigger_plugin_settings_button".into(),
            plugin_id: Some(plugin_id.into()),
            index: Some(index),
            trigger_button: Some(true),
            ..Default::default()
        });
    }

    pub fn send_key_event(&self, slot: &str, is_down: bool) {
        self.send_command(BridgeCommand {
            cmd: "key_event".into(),
            slot: Some(slot.into()),
            event: Some(if is_down { "down".into() } else { "up".into() }),
            ..Default::default()
        });
    }

    pub fn send_dial_event(&self, slot: &str, event_type: &str, delta: i32) {
        self.send_command(BridgeCommand {
            cmd: "dial_event".into(),
            slot: Some(slot.into()),
            event: Some(event_type.into()),
            delta: Some(delta),
            ..Default::default()
        });
    }

    pub fn shutdown(&self) {
        self.send_command(BridgeCommand {
            cmd: "shutdown".into(),
            ..Default::default()
        });
    }

    pub fn locate_bridge_dir() -> Option<PathBuf> {
        Self::locate_runner_script().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))
    }

    /// Resolves the best Python 3 interpreter for running StreamController plugins:
    /// prefers Ballista's dedicated `--system-site-packages` venv if installed, falling back to `python3`.
    pub fn locate_python_executable() -> PathBuf {
        if let Ok(custom) = std::env::var("BALLISTA_PYTHON") {
            let p = PathBuf::from(custom);
            if p.exists() {
                return p;
            }
        }

        let mut candidates = vec![
            glib::user_data_dir().join("ballista").join("venv").join("bin").join("python3"),
            PathBuf::from("/usr/local/share/ballista/venv/bin/python3"),
            PathBuf::from("/usr/share/ballista/venv/bin/python3"),
        ];

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(parent.join("venv").join("bin").join("python3"));
                candidates.push(parent.join("..").join("share").join("ballista").join("venv").join("bin").join("python3"));
            }
        }

        for cand in candidates {
            if cand.exists() {
                return cand;
            }
        }

        PathBuf::from("python3")
    }

    fn locate_runner_script() -> Result<PathBuf, String> {
        if let Ok(custom_dir) = std::env::var("BALLISTA_BRIDGE_DIR") {
            let p = PathBuf::from(custom_dir).join("runner.py");
            if p.exists() {
                return Ok(p);
            }
        }

        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                // Installed binary next to bridge/ or ../share/ballista/bridge/
                candidates.push(parent.join("bridge").join("runner.py"));
                candidates.push(parent.join("..").join("share").join("ballista").join("bridge").join("runner.py"));
                // Dev build paths (target/release/ballista or target/debug/ballista -> ../../bridge/runner.py)
                candidates.push(parent.join("..").join("bridge").join("runner.py"));
                candidates.push(parent.join("..").join("..").join("bridge").join("runner.py"));
            }
        }

        candidates.push(PathBuf::from("bridge/runner.py"));
        candidates.push(glib::user_data_dir().join("ballista").join("bridge").join("runner.py"));
        candidates.push(PathBuf::from("/usr/local/share/ballista/bridge/runner.py"));
        candidates.push(PathBuf::from("/usr/share/ballista/bridge/runner.py"));

        for cand in candidates {
            if cand.exists() {
                return Ok(cand.canonicalize().unwrap_or(cand));
            }
        }

        Err("Could not locate Ballista bridge runner.py in known installation or source paths".into())
    }
}
