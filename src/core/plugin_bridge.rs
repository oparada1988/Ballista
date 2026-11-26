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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub norm_x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub norm_y: Option<f64>,
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

struct NativeProcessHandle {
    stdin: ChildStdin,
    child: Child,
    binary_path: PathBuf,
}

#[derive(Clone)]
pub struct PluginBridge {
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    child: Arc<Mutex<Option<Child>>>,
    native_plugins: Arc<Mutex<HashMap<String, NativeProcessHandle>>>,
    slot_owners: Arc<Mutex<HashMap<String, String>>>,
    event_callback: Arc<Mutex<Option<BridgeEventCallback>>>,
    pending_events: Arc<Mutex<Vec<BridgeEvent>>>,
}

impl PluginBridge {
    pub fn new() -> Self {
        Self {
            stdin: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
            native_plugins: Arc::new(Mutex::new(HashMap::new())),
            slot_owners: Arc::new(Mutex::new(HashMap::new())),
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

    fn write_cmd_to_stdin(stdin: &mut ChildStdin, cmd: &BridgeCommand) {
        if let Ok(json_str) = serde_json::to_string(cmd) {
            let _ = stdin.write_all(json_str.as_bytes());
            let _ = stdin.write_all(b"\n");
            let _ = stdin.flush();
        }
    }

    pub fn send_command(&self, cmd: BridgeCommand) {
        if let Ok(mut lock) = self.stdin.lock() {
            if let Some(ref mut stdin) = *lock {
                Self::write_cmd_to_stdin(stdin, &cmd);
            }
        }
    }

    fn send_to_plugin_or_python(&self, plugin_id: &str, cmd: BridgeCommand) {
        if let Ok(mut natives) = self.native_plugins.lock() {
            if let Some(handle) = natives.get_mut(plugin_id) {
                Self::write_cmd_to_stdin(&mut handle.stdin, &cmd);
                return;
            }
        }
        self.send_command(cmd);
    }

    fn send_to_slot_owner_or_python(&self, slot: &str, mut cmd: BridgeCommand) {
        let resolved = self.slot_owners.lock().ok().and_then(|m| {
            if let Some(pid) = m.get(slot) {
                return Some((slot.to_string(), pid.clone()));
            }
            let suffix = format!("::{slot}");
            m.iter()
                .find(|(k, _)| k.ends_with(&suffix))
                .map(|(k, pid)| (k.clone(), pid.clone()))
        });
        if let Some((canonical_slot, pid)) = resolved {
            if cmd.slot.as_deref() == Some(slot) && canonical_slot != slot {
                cmd.slot = Some(canonical_slot);
            }
            self.send_to_plugin_or_python(&pid, cmd);
        } else {
            self.send_command(cmd);
        }
    }

    fn broadcast_to_all_natives(&self, cmd: &BridgeCommand) {
        if let Ok(mut natives) = self.native_plugins.lock() {
            for handle in natives.values_mut() {
                Self::write_cmd_to_stdin(&mut handle.stdin, cmd);
            }
        }
    }

    /// Spawns (or restarts if updated) an out-of-process Native Ballista Rust plugin binary
    /// speaking Ballista's newline-delimited JSONL IPC protocol.
    pub fn init_native_plugin(
        &self,
        plugin_id: &str,
        plugin_path: &Path,
        binary_path: &Path,
    ) -> Result<(), String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(binary_path) {
                let mode = meta.permissions().mode();
                if (mode & 0o111) == 0 {
                    let _ = std::fs::set_permissions(
                        binary_path,
                        std::fs::Permissions::from_mode(mode | 0o755),
                    );
                }
            }
        }

        // If already running from the exact same binary and alive, just re-send init_plugin
        if let Ok(mut natives) = self.native_plugins.lock() {
            if let Some(existing) = natives.get_mut(plugin_id) {
                let alive = existing.child.try_wait().ok().flatten().is_none();
                if alive && existing.binary_path == binary_path {
                    Self::write_cmd_to_stdin(
                        &mut existing.stdin,
                        &BridgeCommand {
                            cmd: "init_plugin".into(),
                            plugin_id: Some(plugin_id.into()),
                            plugin_path: Some(plugin_path.to_string_lossy().to_string()),
                            ..Default::default()
                        },
                    );
                    return Ok(());
                }
                let _ = existing.child.kill();
                let _ = existing.child.wait();
                natives.remove(plugin_id);
            }
        }

        let data_dir = glib::user_data_dir()
            .join("ballista")
            .join("plugin_data")
            .join(plugin_id);
        let cache_dir = glib::user_cache_dir()
            .join("ballista")
            .join("plugins")
            .join(plugin_id);
        let _ = std::fs::create_dir_all(&data_dir);
        let _ = std::fs::create_dir_all(&cache_dir);

        tracing::info!(
            "Starting Native Ballista plugin '{}' ({})",
            plugin_id,
            binary_path.display()
        );

        let mut child = Command::new(binary_path)
            .current_dir(plugin_path)
            .env("BALLISTA_PLUGIN_ID", plugin_id)
            .env("BALLISTA_PLUGIN_DIR", plugin_path)
            .env("BALLISTA_DATA_DIR", &data_dir)
            .env("BALLISTA_CACHE_DIR", &cache_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                format!(
                    "Failed to spawn native plugin '{}' ({}): {e}",
                    plugin_id,
                    binary_path.display()
                )
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("Failed to capture stdout for native plugin '{plugin_id}'"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("Failed to capture stdin for native plugin '{plugin_id}'"))?;

        Self::write_cmd_to_stdin(
            &mut stdin,
            &BridgeCommand {
                cmd: "init_plugin".into(),
                plugin_id: Some(plugin_id.into()),
                plugin_path: Some(plugin_path.to_string_lossy().to_string()),
                ..Default::default()
            },
        );

        if let Ok(mut natives) = self.native_plugins.lock() {
            natives.insert(
                plugin_id.to_string(),
                NativeProcessHandle {
                    stdin,
                    child,
                    binary_path: binary_path.to_path_buf(),
                },
            );
        }

        let cb_lock = self.event_callback.clone();
        let pending_lock = self.pending_events.clone();
        let pid_log = plugin_id.to_string();

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
                        tracing::warn!("Native plugin '{}' stdout read error: {e}", pid_log);
                        break;
                    }
                }
            }
            tracing::info!("Native plugin '{}' stdout reader terminated", pid_log);
        });

        Ok(())
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
        if let Ok(mut owners) = self.slot_owners.lock() {
            owners.retain(|_, owner| owner != plugin_id);
        }
        if let Ok(mut natives) = self.native_plugins.lock() {
            if let Some(mut handle) = natives.remove(plugin_id) {
                Self::write_cmd_to_stdin(
                    &mut handle.stdin,
                    &BridgeCommand {
                        cmd: "shutdown".into(),
                        plugin_id: Some(plugin_id.into()),
                        ..Default::default()
                    },
                );
                let _ = handle.child.kill();
                let _ = handle.child.wait();
                return;
            }
        }
        self.send_command(BridgeCommand {
            cmd: "uninstall_plugin".into(),
            plugin_id: Some(plugin_id.into()),
            ..Default::default()
        });
    }

    pub fn create_action(&self, slot: &str, plugin_id: &str, action_id: &str, settings: HashMap<String, serde_json::Value>) {
        if let Ok(mut owners) = self.slot_owners.lock() {
            owners.insert(slot.to_string(), plugin_id.to_string());
        }
        self.send_to_plugin_or_python(
            plugin_id,
            BridgeCommand {
                cmd: "create_action".into(),
                plugin_id: Some(plugin_id.into()),
                slot: Some(slot.into()),
                action_id: Some(action_id.into()),
                settings: Some(settings),
                ..Default::default()
            },
        );
    }

    pub fn destroy_action(&self, slot: &str) {
        let cmd = BridgeCommand {
            cmd: "destroy_action".into(),
            slot: Some(slot.into()),
            ..Default::default()
        };
        let owner = self.slot_owners.lock().ok().and_then(|mut m| m.remove(slot));
        if let Some(pid) = owner {
            self.send_to_plugin_or_python(&pid, cmd);
        } else {
            self.send_command(cmd);
        }
    }

    pub fn clear_all_actions(&self) {
        if let Ok(mut owners) = self.slot_owners.lock() {
            owners.clear();
        }
        let cmd = BridgeCommand {
            cmd: "clear_all_actions".into(),
            ..Default::default()
        };
        self.broadcast_to_all_natives(&cmd);
        self.send_command(cmd);
    }

    pub fn clear_device_actions(&self, device_key: &str) {
        let prefix = format!("{device_key}::");
        if let Ok(mut owners) = self.slot_owners.lock() {
            owners.retain(|slot, _| !slot.starts_with(&prefix));
        }
        let cmd = BridgeCommand {
            cmd: "clear_device_actions".into(),
            slot: Some(device_key.into()),
            ..Default::default()
        };
        self.broadcast_to_all_natives(&cmd);
        self.send_command(cmd);
    }

    pub fn update_action_settings(&self, slot: &str, settings: HashMap<String, serde_json::Value>) {
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "update_action_settings".into(),
                slot: Some(slot.into()),
                settings: Some(settings),
                ..Default::default()
            },
        );
    }

    pub fn get_action_config_rows(&self, slot: &str) {
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "get_action_config_rows".into(),
                slot: Some(slot.into()),
                ..Default::default()
            },
        );
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
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "set_action_config_row".into(),
                slot: Some(slot.into()),
                index: Some(index),
                selected,
                active,
                text,
                color,
                value,
                ..Default::default()
            },
        );
    }

    pub fn trigger_action_config_button(&self, slot: &str, index: usize) {
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "trigger_action_config_button".into(),
                slot: Some(slot.into()),
                index: Some(index),
                trigger_button: Some(true),
                ..Default::default()
            },
        );
    }

    pub fn get_plugin_settings_rows(&self, plugin_id: &str) {
        self.send_to_plugin_or_python(
            plugin_id,
            BridgeCommand {
                cmd: "get_plugin_settings_rows".into(),
                plugin_id: Some(plugin_id.into()),
                ..Default::default()
            },
        );
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
        self.send_to_plugin_or_python(
            plugin_id,
            BridgeCommand {
                cmd: "set_plugin_settings_row".into(),
                plugin_id: Some(plugin_id.into()),
                index: Some(index),
                selected,
                active,
                text,
                color,
                value,
                ..Default::default()
            },
        );
    }

    pub fn trigger_plugin_settings_button(&self, plugin_id: &str, index: usize) {
        self.send_to_plugin_or_python(
            plugin_id,
            BridgeCommand {
                cmd: "trigger_plugin_settings_button".into(),
                plugin_id: Some(plugin_id.into()),
                index: Some(index),
                trigger_button: Some(true),
                ..Default::default()
            },
        );
    }

    pub fn send_key_event(&self, slot: &str, is_down: bool) {
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "key_event".into(),
                slot: Some(slot.into()),
                event: Some(if is_down { "down".into() } else { "up".into() }),
                ..Default::default()
            },
        );
    }

    pub fn send_dial_event(&self, slot: &str, event_type: &str, delta: i32) {
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "dial_event".into(),
                slot: Some(slot.into()),
                event: Some(event_type.into()),
                delta: Some(delta),
                ..Default::default()
            },
        );
    }

    pub fn send_dial_tap(&self, slot: &str, norm_x: f64, norm_y: f64) {
        self.send_to_slot_owner_or_python(
            slot,
            BridgeCommand {
                cmd: "dial_event".into(),
                slot: Some(slot.into()),
                event: Some("tap".into()),
                delta: Some(0),
                norm_x: Some(norm_x.clamp(0.0, 1.0)),
                norm_y: Some(norm_y.clamp(0.0, 1.0)),
                ..Default::default()
            },
        );
    }

    pub fn shutdown(&self) {
        let cmd = BridgeCommand {
            cmd: "shutdown".into(),
            ..Default::default()
        };
        if let Ok(mut natives) = self.native_plugins.lock() {
            for (_, mut handle) in natives.drain() {
                Self::write_cmd_to_stdin(&mut handle.stdin, &cmd);
                let _ = handle.child.kill();
                let _ = handle.child.wait();
            }
        }
        self.send_command(cmd);
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
