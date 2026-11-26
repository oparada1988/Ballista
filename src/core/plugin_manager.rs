use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::models::KeyAppearance;

#[derive(Debug, Clone)]
pub struct InstalledPluginSummary {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub thumbnail_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionSettingType {
    Combo {
        title: String,
        subtitle: String,
        options: Vec<String>,
        default_idx: u32,
    },
    Switch {
        title: String,
        subtitle: String,
        default_active: bool,
    },
    Text {
        title: String,
        subtitle: String,
        default_text: String,
    },
    Spin {
        title: String,
        subtitle: String,
        min: f64,
        max: f64,
        step: f64,
        default_val: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfigRow {
    pub key: String,
    pub setting_type: ActionSettingType,
}

impl ActionConfigRow {
    pub fn title(&self) -> &str {
        match &self.setting_type {
            ActionSettingType::Combo { title, .. } => title,
            ActionSettingType::Switch { title, .. } => title,
            ActionSettingType::Text { title, .. } => title,
            ActionSettingType::Spin { title, .. } => title,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginActionInfo {
    pub id: String,
    pub name: String,
    pub icon_name_or_file: String,
    pub config_rows: Vec<ActionConfigRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPlugin {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub path: PathBuf,
    pub actions: Vec<PluginActionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub desktop_id: String,
    pub name: String,
    pub exec: String,
    pub icon: String,
    #[serde(default)]
    pub desktop_path: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Clone)]
pub struct PluginManager {
    pub plugins_dir: PathBuf,
    pub plugins: HashMap<String, DiscoveredPlugin>,
    pub installed_apps: Vec<InstalledApp>,
    pub bridge: super::plugin_bridge::PluginBridge,
    pub live_config_rows: Arc<Mutex<HashMap<String, Vec<super::plugin_bridge::BridgeConfigRow>>>>,
    pub live_plugin_settings_rows: Arc<Mutex<HashMap<String, Vec<super::plugin_bridge::BridgeConfigRow>>>>,
    pub plugin_has_settings: Arc<Mutex<HashMap<String, bool>>>,
}

impl PluginManager {
    pub fn new() -> Self {
        let plugins_dir = glib::user_data_dir().join("ballista").join("plugins");
        let _ = fs::create_dir_all(&plugins_dir);

        let bridge = super::plugin_bridge::PluginBridge::new();
        if let Err(e) = bridge.start() {
            tracing::warn!("Failed to start plugin bridge: {e}");
        }

        let mut manager = Self {
            plugins_dir,
            plugins: HashMap::new(),
            installed_apps: Vec::new(),
            bridge,
            live_config_rows: Arc::new(Mutex::new(HashMap::new())),
            live_plugin_settings_rows: Arc::new(Mutex::new(HashMap::new())),
            plugin_has_settings: Arc::new(Mutex::new(HashMap::new())),
        };

        // Defer desktop app scanning off startup critical path for instant login boot
        manager.rescan_plugins();
        manager
    }

    /// Scans freedesktop `.desktop` entries off the UI/startup thread
    pub fn collect_installed_apps() -> Vec<InstalledApp> {
        let mut tmp = Self {
            plugins_dir: PathBuf::new(),
            plugins: HashMap::new(),
            installed_apps: Vec::new(),
            bridge: super::plugin_bridge::PluginBridge::new(),
            live_config_rows: Arc::new(Mutex::new(HashMap::new())),
            live_plugin_settings_rows: Arc::new(Mutex::new(HashMap::new())),
            plugin_has_settings: Arc::new(Mutex::new(HashMap::new())),
        };
        tmp.scan_installed_apps();
        tmp.installed_apps
    }

    pub fn set_live_config_rows(&self, slot: &str, rows: Vec<super::plugin_bridge::BridgeConfigRow>) {
        if let Ok(mut lock) = self.live_config_rows.lock() {
            lock.insert(slot.to_string(), rows);
        }
    }

    pub fn get_live_config_rows(&self, slot: &str) -> Option<Vec<super::plugin_bridge::BridgeConfigRow>> {
        if let Ok(lock) = self.live_config_rows.lock() {
            lock.get(slot).cloned()
        } else {
            None
        }
    }

    pub fn clear_live_config_rows(&self, slot: &str) {
        if let Ok(mut lock) = self.live_config_rows.lock() {
            lock.remove(slot);
        }
    }

    pub fn clear_all_live_config_rows(&self) {
        if let Ok(mut lock) = self.live_config_rows.lock() {
            lock.clear();
        }
    }

    pub fn clear_device_live_config_rows(&self, device_key: &str) {
        let prefix = format!("{device_key}::");
        if let Ok(mut lock) = self.live_config_rows.lock() {
            lock.retain(|k, _| !k.starts_with(&prefix));
        }
    }

    pub fn set_live_plugin_settings_rows(&self, plugin_id: &str, rows: Vec<super::plugin_bridge::BridgeConfigRow>) {
        if let Ok(mut lock) = self.live_plugin_settings_rows.lock() {
            lock.insert(plugin_id.to_string(), rows);
        }
    }

    pub fn get_live_plugin_settings_rows(&self, plugin_id: &str) -> Option<Vec<super::plugin_bridge::BridgeConfigRow>> {
        if let Ok(lock) = self.live_plugin_settings_rows.lock() {
            lock.get(plugin_id).cloned()
        } else {
            None
        }
    }

    pub fn apply_bridge_plugin_metadata(
        &mut self,
        plugin_id: &str,
        plugin_name: &str,
        has_settings: bool,
        actions_meta: &[super::plugin_bridge::BridgeActionMeta],
    ) {
        if let Ok(mut lock) = self.plugin_has_settings.lock() {
            lock.insert(plugin_id.to_string(), has_settings);
        }
        if let Some(plugin) = self.plugins.get_mut(plugin_id) {
            if !plugin_name.is_empty() && plugin_name != plugin_id {
                plugin.name = plugin_name.to_string();
            }
            if !actions_meta.is_empty() {
                let mut updated_actions = Vec::new();
                for am in actions_meta {
                    let action_stem = am.action_id.split("::").last().unwrap_or(&am.action_id);
                    let icon = if !am.icon_path.is_empty() && Path::new(&am.icon_path).exists() {
                        am.icon_path.clone()
                    } else if !am.icon_name.is_empty() && am.icon_name != "insert-image-symbolic" {
                        am.icon_name.clone()
                    } else {
                        find_action_icon(&plugin.path, action_stem)
                    };
                    updated_actions.push(PluginActionInfo {
                        id: am.action_id.clone(),
                        name: am.action_name.clone(),
                        icon_name_or_file: icon,
                        config_rows: Vec::new(),
                    });
                }
                plugin.actions = updated_actions;
            }
        }
    }

    /// Returns all Freedesktop `.desktop` application directories in precedence order:
    /// User overrides (including AppImages), User Flatpaks, System Flatpaks, Snaps,
    /// custom `XDG_DATA_DIRS` entries, `/usr/local`, and `/usr`.
    pub fn desktop_application_dirs() -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut push_unique = |p: PathBuf| {
            if !dirs.contains(&p) {
                dirs.push(p);
            }
        };

        let home = std::env::var("HOME").ok().map(PathBuf::from);
        // 1. User applications (AppImages via Gear Lever / AppImageLauncher, custom .desktop files, user overrides)
        push_unique(glib::user_data_dir().join("applications"));
        if let Some(ref h) = home {
            push_unique(h.join(".local/share/applications"));
            // 2. User Flatpak exports
            push_unique(h.join(".local/share/flatpak/exports/share/applications"));
        }
        // 3. System Flatpak exports
        push_unique(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
        // 4. Snap desktop applications
        push_unique(PathBuf::from("/var/lib/snapd/desktop/applications"));
        // 5. Any additional XDG_DATA_DIRS (NixOS, Guix, Distrobox, custom prefixes)
        if let Ok(xdg_dirs) = std::env::var("XDG_DATA_DIRS") {
            for entry in xdg_dirs.split(':').filter(|s| !s.trim().is_empty()) {
                push_unique(PathBuf::from(entry).join("applications"));
            }
        }
        // 6. Standard local and system directories
        push_unique(PathBuf::from("/usr/local/share/applications"));
        push_unique(PathBuf::from("/usr/share/applications"));
        dirs
    }

    pub fn scan_installed_apps(&mut self) {
        let mut apps: Vec<InstalledApp> = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let user_apps_dir = glib::user_data_dir().join("applications");

        fn collect_desktop_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
            if depth > 3 {
                return;
            }
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        collect_desktop_files(&path, depth + 1, out);
                    } else if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                        out.push(path);
                    }
                }
            }
        }

        for dir in Self::desktop_application_dirs() {
            if !dir.exists() {
                continue;
            }
            let mut files = Vec::new();
            collect_desktop_files(&dir, 0, &mut files);
            for path in files {
                let Some(desktop_id) = path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()) else {
                    continue;
                };
                if seen_ids.contains(&desktop_id) {
                    continue;
                }
                let Ok(content) = fs::read_to_string(&path) else {
                    continue;
                };

                // Strictly parse keys inside `[Desktop Entry]` only (never bleed into `[Desktop Action ...]`)
                let mut in_main_entry = false;
                let mut name: Option<String> = None;
                let mut exec: Option<String> = None;
                let mut icon: Option<String> = None;
                let mut entry_type: Option<String> = None;
                let mut no_display = false;
                let mut hidden = false;

                for raw_line in content.lines() {
                    let line = raw_line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if line.starts_with('[') && line.ends_with(']') {
                        if line == "[Desktop Entry]" {
                            in_main_entry = true;
                        } else if in_main_entry {
                            break;
                        }
                        continue;
                    }
                    if !in_main_entry {
                        continue;
                    }
                    if let Some(val) = line.strip_prefix("Name=") {
                        if name.is_none() && !val.trim().is_empty() {
                            name = Some(val.trim().to_string());
                        }
                    } else if let Some(val) = line.strip_prefix("Exec=") {
                        if exec.is_none() && !val.trim().is_empty() {
                            exec = Some(val.trim().to_string());
                        }
                    } else if let Some(val) = line.strip_prefix("Icon=") {
                        if icon.is_none() && !val.trim().is_empty() {
                            icon = Some(val.trim().to_string());
                        }
                    } else if let Some(val) = line.strip_prefix("Type=") {
                        entry_type = Some(val.trim().to_string());
                    } else if line.eq_ignore_ascii_case("NoDisplay=true") {
                        no_display = true;
                    } else if line.eq_ignore_ascii_case("Hidden=true") {
                        hidden = true;
                    }
                }

                if no_display || hidden {
                    continue;
                }
                if let Some(ref t) = entry_type {
                    if t != "Application" {
                        continue;
                    }
                }

                if let (Some(n), Some(e)) = (name, exec) {
                    let path_str = path.to_string_lossy().to_string();
                    let lower_path = path_str.to_lowercase();
                    let lower_exec = e.to_lowercase();
                    let source = if lower_path.contains("/flatpak/")
                        || lower_exec.contains("flatpak run")
                        || lower_exec.contains("/flatpak ")
                    {
                        "Flatpak"
                    } else if lower_path.contains("/snapd/")
                        || lower_path.contains("/snap/")
                        || lower_exec.contains("/snap/")
                        || lower_exec.starts_with("snap run ")
                    {
                        "Snap"
                    } else if lower_exec.contains(".appimage")
                        || lower_exec.contains("appimagelauncher")
                        || lower_path.contains("appimage")
                    {
                        "AppImage"
                    } else if path.starts_with(&user_apps_dir) {
                        "User"
                    } else {
                        "System"
                    };

                    seen_ids.insert(desktop_id.clone());
                    apps.push(InstalledApp {
                        desktop_id,
                        name: n,
                        exec: e,
                        icon: icon.unwrap_or_else(|| "application-x-executable".into()),
                        desktop_path: path_str,
                        source: source.to_string(),
                    });
                }
            }
        }

        apps.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.source.cmp(&b.source))
        });
        self.installed_apps = apps;
    }

    pub fn rescan_plugins(&mut self) {
        self.plugins.clear();

        let plugin_search_dirs = [
            self.plugins_dir.clone(),
        ];

        for search_dir in &plugin_search_dirs {
            if let Ok(entries) = fs::read_dir(search_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let manifest_file = p.join("manifest.json");
                        if manifest_file.exists() {
                            if let Ok(data) = fs::read_to_string(&manifest_file) {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                                    let id = json.get("id").and_then(|v| v.as_str()).unwrap_or("unknown_plugin").to_string();
                                    
                                    // Don't overwrite if already scanned
                                    if self.plugins.contains_key(&id) {
                                        continue;
                                    }

                                    let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed Plugin").to_string();
                                    let author = json.get("author").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                                    let version = json.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string();
                                    let description = json.get("description")
                                        .or_else(|| json.get("short-description"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    // Dynamic Discovery of Actions from actions/ directory
                                    let mut actions = Vec::new();
                                    let actions_dir = p.join("actions");

                                    if actions_dir.exists() && actions_dir.is_dir() {
                                        if let Ok(act_entries) = fs::read_dir(&actions_dir) {
                                            for act_entry in act_entries.flatten() {
                                                let act_p = act_entry.path();
                                                let file_name = act_p.file_name().unwrap_or_default().to_string_lossy().to_string();
                                                
                                                if file_name.starts_with('_') || file_name == "mixins" {
                                                    continue;
                                                }

                                                let action_stem = if act_p.is_dir() {
                                                    file_name
                                                } else if act_p.extension().and_then(|e| e.to_str()) == Some("py") {
                                                    act_p.file_stem().unwrap_or_default().to_string_lossy().to_string()
                                                } else {
                                                    continue;
                                                };

                                                let action_id = format!("{id}::{action_stem}");
                                                let display_name = format_action_name(&action_stem);

                                                // Find candidate action icon in assets/
                                                let icon_str = find_action_icon(&p, &action_stem);

                                                // Parse interactive config rows from the action Python file
                                                let action_py = if act_p.is_dir() {
                                                    act_p.join(format!("{action_stem}.py"))
                                                } else {
                                                    act_p.clone()
                                                };
                                                let config_rows = parse_action_config_rows(&action_py);

                                                actions.push(PluginActionInfo {
                                                    id: action_id,
                                                    name: display_name,
                                                    icon_name_or_file: icon_str,
                                                    config_rows,
                                                });
                                            }
                                        }
                                    }

                                    // Fallback if no sub-actions were parsed
                                    if actions.is_empty() {
                                        if id.contains("AppLauncher") {
                                            let icon_str = find_action_icon(&p, "action_launch");
                                            actions.push(PluginActionInfo {
                                                id: format!("{id}::LaunchAppAction"),
                                                name: "Launch App".into(),
                                                icon_name_or_file: icon_str,
                                                config_rows: Vec::new(),
                                            });
                                        } else {
                                            let icon_str = find_action_icon(&p, "action");
                                            actions.push(PluginActionInfo {
                                                id: format!("{id}::DefaultAction"),
                                                name: "Execute Action".into(),
                                                icon_name_or_file: icon_str,
                                                config_rows: Vec::new(),
                                            });
                                        }
                                    }

                                    actions.sort_by(|a, b| a.name.cmp(&b.name));

                                    self.plugins.insert(id.clone(), DiscoveredPlugin {
                                        id: id.clone(),
                                        name,
                                        author,
                                        version,
                                        description,
                                        path: p.clone(),
                                        actions,
                                    });

                                    // Initialize plugin in Python compatibility bridge
                                    self.bridge.init_plugin(&id, &p);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Sideload / Install from a local directory containing manifest.json
    pub fn install_from_dir(&mut self, source_dir: &Path) -> Result<String, String> {
        let manifest = source_dir.join("manifest.json");
        if !manifest.exists() {
            return Err("Selected folder does not contain a valid manifest.json".into());
        }

        let data = fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
        let json: serde_json::Value = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        let id = json.get("id").and_then(|v| v.as_str()).unwrap_or("custom_plugin");

        let dest = self.plugins_dir.join(id);
        let _ = fs::remove_dir_all(&dest);

        // Copy directory recursively
        Self::copy_dir_all(source_dir, &dest).map_err(|e| format!("Failed to copy plugin: {e}"))?;
        Self::run_plugin_install_hook(&dest);

        self.rescan_plugins();
        Ok(id.to_string())
    }

    /// Extract and install a plugin from a local `.zip` archive without holding PluginManager locks
    /// (supports root `manifest.json` as well as GitHub repo wrapper directories like `Plugin-main/manifest.json`)
    pub fn extract_plugin_zip(plugins_dir: &Path, zip_path: &Path) -> Result<InstalledPluginSummary, String> {
        if !zip_path.is_file() {
            return Err(format!("Selected file does not exist: {}", zip_path.display()));
        }

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let temp_extract = std::env::temp_dir().join(format!("ballista_local_zip_{stamp}"));
        let _ = fs::remove_dir_all(&temp_extract);
        fs::create_dir_all(&temp_extract)
            .map_err(|e| format!("Failed to create temporary extraction folder: {e}"))?;

        let unzip_status = std::process::Command::new("unzip")
            .arg("-q")
            .arg("-o")
            .arg(zip_path)
            .arg("-d")
            .arg(&temp_extract)
            .status()
            .map_err(|e| {
                let _ = fs::remove_dir_all(&temp_extract);
                format!("Failed to invoke unzip: {e}")
            })?;

        if !unzip_status.success() {
            let _ = fs::remove_dir_all(&temp_extract);
            return Err(format!("Failed to extract ZIP archive '{}'", zip_path.display()));
        }

        let mut source_folder: Option<PathBuf> = None;
        if temp_extract.join("manifest.json").exists() {
            source_folder = Some(temp_extract.clone());
        } else if let Ok(entries) = fs::read_dir(&temp_extract) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_dir() || entry.file_name() == "__MACOSX" {
                    continue;
                }
                if p.join("manifest.json").exists() {
                    source_folder = Some(p);
                    break;
                }
                if let Ok(sub_entries) = fs::read_dir(&p) {
                    for sub in sub_entries.flatten() {
                        let sp = sub.path();
                        if sp.is_dir() && sp.join("manifest.json").exists() {
                            source_folder = Some(sp);
                            break;
                        }
                    }
                }
                if source_folder.is_some() {
                    break;
                }
            }
        }

        let Some(source_folder) = source_folder else {
            let _ = fs::remove_dir_all(&temp_extract);
            return Err("Selected ZIP archive does not contain a valid manifest.json".into());
        };

        let manifest_data = fs::read_to_string(source_folder.join("manifest.json")).map_err(|e| {
            let _ = fs::remove_dir_all(&temp_extract);
            format!("Failed to read manifest.json: {e}")
        })?;
        let json: serde_json::Value = serde_json::from_str(&manifest_data).map_err(|e| {
            let _ = fs::remove_dir_all(&temp_extract);
            format!("Invalid manifest.json: {e}")
        })?;
        let fallback_id = source_folder
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("custom_plugin");
        let id = json
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_id)
            .trim()
            .to_string();
        let name = json
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&id)
            .trim()
            .to_string();
        let version = json
            .get("version")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("1.0.0")
            .trim()
            .to_string();
        let author = json
            .get("author")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let thumb_rel = json
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string());

        let dest = plugins_dir.join(&id);
        let _ = fs::remove_dir_all(&dest);
        fs::create_dir_all(&dest).map_err(|e| {
            let _ = fs::remove_dir_all(&temp_extract);
            format!("Failed to prepare plugin directory: {e}")
        })?;

        let copy_res = Self::copy_dir_all(&source_folder, &dest);
        let _ = fs::remove_dir_all(&temp_extract);
        copy_res.map_err(|e| format!("Failed to copy extracted plugin '{id}': {e}"))?;

        Self::run_plugin_install_hook(&dest);
        let thumbnail_path = thumb_rel
            .map(|rel| dest.join(rel))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().to_string());

        tracing::info!("Installed plugin '{}' ({}) from ZIP into {}", name, id, dest.display());
        Ok(InstalledPluginSummary {
            id,
            name,
            version,
            author,
            thumbnail_path,
        })
    }

    /// Sideload / Install a plugin from a local `.zip` archive
    #[allow(dead_code)]
    pub fn install_from_zip(&mut self, zip_path: &Path) -> Result<String, String> {
        let summary = Self::extract_plugin_zip(&self.plugins_dir, zip_path)?;
        self.rescan_plugins();
        Ok(summary.id)
    }

    /// Download & Extract a plugin directly from a remote zip archive URL without touching UI or plugin state
    pub fn download_and_extract_plugin(plugins_dir: &Path, id: &str, download_url: &str) -> Result<(), String> {
        let url = download_url.trim();
        if url.is_empty() || (!url.starts_with("https://") && !url.starts_with("http://")) {
            return Err(format!("Invalid remote download URL for plugin '{id}': '{url}'"));
        }

        tracing::info!("Downloading plugin '{}' directly from remote URL: {}", id, url);

        let temp_zip = std::env::temp_dir().join(format!("ballista_{id}.zip"));
        let temp_extract = std::env::temp_dir().join(format!("ballista_{id}_extracted"));
        let _ = fs::remove_file(&temp_zip);
        let _ = fs::remove_dir_all(&temp_extract);

        let status = std::process::Command::new("curl")
            .arg("-fSL")
            .arg("--connect-timeout")
            .arg("10")
            .arg("--max-time")
            .arg("60")
            .arg("-o")
            .arg(&temp_zip)
            .arg(url)
            .status()
            .map_err(|e| format!("Failed to invoke curl for '{id}': {e}"))?;

        if !status.success() {
            let _ = fs::remove_file(&temp_zip);
            return Err(format!("Remote download failed for '{id}' from {url} (exit: {status})"));
        }

        let zip_bytes = fs::metadata(&temp_zip).map(|m| m.len()).unwrap_or(0);
        if zip_bytes == 0 {
            let _ = fs::remove_file(&temp_zip);
            return Err(format!("Downloaded archive for '{id}' from {url} is empty"));
        }
        tracing::info!("Downloaded {} bytes for plugin '{}' from {}", zip_bytes, id, url);

        let unzip_status = std::process::Command::new("unzip")
            .arg("-q")
            .arg("-o")
            .arg(&temp_zip)
            .arg("-d")
            .arg(&temp_extract)
            .status()
            .map_err(|e| format!("Failed to unzip archive for '{id}': {e}"))?;

        if !unzip_status.success() {
            let _ = fs::remove_file(&temp_zip);
            let _ = fs::remove_dir_all(&temp_extract);
            return Err(format!("Failed to extract plugin archive for '{id}'"));
        }

        // GitHub zips contain an inner root folder (e.g. AppLauncher-main)
        // Find folder containing manifest.json
        let mut source_folder = temp_extract.clone();
        if !source_folder.join("manifest.json").exists() {
            if let Ok(entries) = fs::read_dir(&temp_extract) {
                for entry in entries.flatten() {
                    if entry.path().is_dir() && entry.path().join("manifest.json").exists() {
                        source_folder = entry.path();
                        break;
                    }
                }
            }
        }

        if !source_folder.join("manifest.json").exists() {
            let _ = fs::remove_file(&temp_zip);
            let _ = fs::remove_dir_all(&temp_extract);
            return Err(format!("Extracted archive for '{id}' does not contain manifest.json"));
        }

        let dest = plugins_dir.join(id);
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::create_dir_all(&dest);

        Self::copy_dir_all(&source_folder, &dest).map_err(|e| format!("Failed to copy plugin '{id}': {e}"))?;
        let _ = fs::remove_file(&temp_zip);
        let _ = fs::remove_dir_all(&temp_extract);

        // Execute StreamController plugin __install__.py hook if present (e.g. creates backend/.venv)
        Self::run_plugin_install_hook(&dest);

        tracing::info!("Installed plugin '{}' cleanly into {}", id, dest.display());
        Ok(())
    }

    fn run_plugin_install_hook(plugin_dir: &Path) {
        let install_py = plugin_dir.join("__install__.py");
        if !install_py.exists() {
            return;
        }
        tracing::info!("Running StreamController plugin install hook: {}", install_py.display());
        let python_bin = super::plugin_bridge::PluginBridge::locate_python_executable();
        let mut cmd = std::process::Command::new(&python_bin);
        cmd.arg(&install_py).current_dir(plugin_dir);
        if let Some(bridge_dir) = super::plugin_bridge::PluginBridge::locate_bridge_dir() {
            let existing = std::env::var("PYTHONPATH").unwrap_or_default();
            let combined = if existing.is_empty() {
                bridge_dir.to_string_lossy().to_string()
            } else {
                format!("{}:{}", bridge_dir.display(), existing)
            };
            cmd.env("PYTHONPATH", combined);
        }
        if let Err(e) = cmd.status() {
            tracing::warn!("Plugin install hook failed for {}: {e}", plugin_dir.display());
        }
    }

    /// Download & Install a plugin directly from a zip archive URL (e.g. GitHub release or main branch)
    pub fn install_from_url(&mut self, id: &str, download_url: &str) -> Result<String, String> {
        Self::download_and_extract_plugin(&self.plugins_dir, id, download_url)?;
        self.rescan_plugins();
        Ok(id.to_string())
    }

    pub fn uninstall_plugin(&mut self, plugin_id: &str) -> Result<(), String> {
        self.bridge.uninstall_plugin(plugin_id);
        let dest = self.plugins_dir.join(plugin_id);
        if dest.exists() {
            fs::remove_dir_all(&dest).map_err(|e| format!("Failed to remove plugin directory: {e}"))?;
        }
        self.plugins.remove(plugin_id);
        if let Ok(mut lock) = self.live_plugin_settings_rows.lock() {
            lock.remove(plugin_id);
        }
        if let Ok(mut lock) = self.plugin_has_settings.lock() {
            lock.remove(plugin_id);
        }
        self.rescan_plugins();
        Ok(())
    }

    fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if ty.is_dir() {
                if entry.file_name() != ".git" && entry.file_name() != "__pycache__" {
                    Self::copy_dir_all(&src_path, &dst_path)?;
                }
            } else {
                fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    pub fn launch_app_desktop(&self, desktop_id: &str) -> Result<(), String> {
        crate::plugins::ballista_core::launcher::launch_desktop_app(desktop_id, "");
        Ok(())
    }

    pub fn resolve_action_states(&self, plugin_id: &str, action_stem: &str) -> Vec<KeyAppearance> {
        let display_title = format_action_name(action_stem);
        
        let plugin = match self.plugins.get(plugin_id) {
            Some(p) => p,
            None => {
                let mut app = KeyAppearance::default();
                app.title = display_title;
                app.show_title = true;
                return vec![app];
            }
        };

        let assets = plugin.path.join("assets");
        if !assets.exists() {
            let mut app = KeyAppearance::default();
            app.title = display_title;
            app.show_title = true;
            return vec![app];
        }

        let clean = action_stem.trim_end_matches("Action").trim_end_matches(".py");
        let snake = to_snake_case(clean);
        let kebab = to_kebab_case(clean);
        let raw_lower = clean.to_lowercase();

        // 1. Check specialized toggle pairs (e.g. DiscordSC InputMode)
        if snake == "input_mode" {
            let s0 = assets.join("input_voice_activity.png");
            let s1 = assets.join("input_push_to_talk.png");
            if s0.exists() && s1.exists() {
                return vec![
                    create_state_appearance(&display_title, Some(s0.to_string_lossy().to_string())),
                    create_state_appearance(&display_title, Some(s1.to_string_lossy().to_string())),
                ];
            }
        }

        // 2. Check binary pairs with "un-" prefix (e.g. undeafen.png / deafen.png, unmute.png / mute.png)
        for cand in &[&kebab, &snake, &raw_lower] {
            let s0 = assets.join(format!("un{cand}.png"));
            let s1 = assets.join(format!("{cand}.png"));
            if s0.exists() && s1.exists() {
                return vec![
                    create_state_appearance(&display_title, Some(s0.to_string_lossy().to_string())),
                    create_state_appearance(&display_title, Some(s1.to_string_lossy().to_string())),
                ];
            }
        }

        // 3. Check suffix-based state pairs (_inactive / _active, _off / _on, _talking)
        for cand in &[&kebab, &snake, &raw_lower] {
            let s0 = assets.join(format!("{cand}.png"));
            if s0.exists() {
                for active_suffix in &["_talking.png", "_active.png", "-active.png", "_on.png", "-on.png"] {
                    let s1 = assets.join(format!("{cand}{active_suffix}"));
                    if s1.exists() {
                        return vec![
                            create_state_appearance(&display_title, Some(s0.to_string_lossy().to_string())),
                            create_state_appearance(&display_title, Some(s1.to_string_lossy().to_string())),
                        ];
                    }
                }
            }

            let s0_off = assets.join(format!("{cand}_off.png"));
            let s1_on = assets.join(format!("{cand}_on.png"));
            if s0_off.exists() && s1_on.exists() {
                return vec![
                    create_state_appearance(&display_title, Some(s0_off.to_string_lossy().to_string())),
                    create_state_appearance(&display_title, Some(s1_on.to_string_lossy().to_string())),
                ];
            }
        }

        // 4. Check action source file for set_media(...) references (e.g. PowerToggleAction -> govee.png)
        let action_py = plugin.path.join("actions").join(action_stem).join(format!("{action_stem}.py"));
        let action_py_alt = plugin.path.join("actions").join(format!("{action_stem}.py"));
        let py_file = if action_py.exists() {
            Some(action_py)
        } else if action_py_alt.exists() {
            Some(action_py_alt)
        } else {
            None
        };

        if let Some(py) = py_file {
            if let Ok(content) = fs::read_to_string(&py) {
                if let Some(pos) = content.find("set_media") {
                    let slice = &content[pos..content.len().min(pos + 300)];
                    for ext in &[".png", ".svg", ".jpg", ".jpeg"] {
                        if let Some(end) = slice.find(ext) {
                            let start = slice[..end].rfind(|c: char| c == '/' || c == '"' || c == '\'').map_or(0, |idx| idx + 1);
                            let media_fname = &slice[start..end + ext.len()];
                            let cand = assets.join(media_fname);
                            if cand.exists() {
                                return vec![create_state_appearance(&display_title, Some(cand.to_string_lossy().to_string()))];
                            }
                        }
                    }
                }
            }
        }

        // 5. Check standalone button artwork (excluding sidebar action-* and store banners)
        for cand in &[&snake, &kebab, &raw_lower] {
            let s = assets.join(format!("{cand}.png"));
            if s.exists() {
                return vec![create_state_appearance(&display_title, Some(s.to_string_lossy().to_string()))];
            }
        }

        // 6. Fallback: check find_action_icon
        let icon_path = find_action_icon(&plugin.path, action_stem);
        if std::path::Path::new(&icon_path).exists() {
            vec![create_state_appearance(&display_title, Some(icon_path))]
        } else {
            let mut app = KeyAppearance::default();
            app.title = display_title;
            app.show_title = true;
            vec![app]
        }
    }

    pub fn get_action_config_rows(&self, plugin_id: &str, action_id: &str) -> Vec<ActionConfigRow> {
        if let Some(plugin) = self.plugins.get(plugin_id) {
            for act in &plugin.actions {
                if act.id == action_id || act.id.ends_with(action_id) || action_id.ends_with(&act.id) {
                    return act.config_rows.clone();
                }
            }
        }
        for plugin in self.plugins.values() {
            for act in &plugin.actions {
                if act.id == action_id || act.id.ends_with(action_id) || action_id.ends_with(&act.id) {
                    return act.config_rows.clone();
                }
            }
        }
        Vec::new()
    }
}

fn create_state_appearance(title: &str, icon_path: Option<String>) -> KeyAppearance {
    let mut app = KeyAppearance::default();
    app.title = title.to_string();
    app.show_title = icon_path.is_none();
    app.icon_path = icon_path;
    app.icon_scale = 100;
    app.bg_color = None;
    app
}

fn format_action_name(raw: &str) -> String {
    let clean = raw.trim_end_matches("Action").trim_end_matches(".py");
    if clean.is_empty() {
        return raw.to_string();
    }
    let mut result = String::new();
    for (i, ch) in clean.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push(' ');
        }
        result.push(ch);
    }
    result
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            for lc in ch.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            for lc in ch.to_lowercase() {
                result.push(lc);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn find_action_icon(plugin_dir: &Path, action_stem: &str) -> String {
    let assets = plugin_dir.join("assets");
    if !assets.exists() {
        return "system-run-symbolic".into();
    }

    // 1. Priority: Extract author-declared ActionHolder icon directly from main.py if present
    let main_py = plugin_dir.join("main.py");
    if main_py.exists() {
        if let Ok(content) = fs::read_to_string(&main_py) {
            if let Some(pos) = content.find(action_stem) {
                let slice = &content[pos..content.len().min(pos + 600)];
                for line in slice.lines().take(15) {
                    if line.contains("icon=") || line.contains("assets") {
                        for ext in &[".png", ".svg", ".jpg", ".jpeg"] {
                            if let Some(end) = line.find(ext) {
                                let start = line[..end].rfind(|c: char| c == '/' || c == '"' || c == '\'').map_or(0, |idx| idx + 1);
                                let icon_filename = &line[start..end + ext.len()];
                                if !icon_filename.is_empty() 
                                    && !icon_filename.contains("thumbnail") 
                                    && !icon_filename.contains("banner") 
                                {
                                    let cand = assets.join(icon_filename);
                                    if cand.exists() {
                                        return cand.to_string_lossy().to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let clean = action_stem.trim_end_matches("Action").trim_end_matches(".py");
    let snake = to_snake_case(clean);
    let kebab = to_kebab_case(clean);
    let raw_stem_lower = action_stem.to_lowercase();

    // 2. Author patterns in order of precedence
    let mut candidate_filenames = Vec::new();

    // Pattern 1: action-<kebab>.png (DiscordSC standard: e.g. action-call-mixer.png, action-mute.png)
    candidate_filenames.push(format!("action-{kebab}.png"));
    candidate_filenames.push(format!("action-{kebab}.svg"));

    // Pattern 2: action_<snake>.png
    candidate_filenames.push(format!("action_{snake}.png"));
    candidate_filenames.push(format!("action_{snake}.svg"));

    // Pattern 3: <snake>_action.png (Govee standard: e.g. brightness_temp_action.png, scene_action.png)
    candidate_filenames.push(format!("{snake}_action.png"));
    candidate_filenames.push(format!("{snake}_action.svg"));

    // Pattern 4: <snake>.png (OBS standard: e.g. scene.png, stream_active.png, stats.png)
    candidate_filenames.push(format!("{snake}.png"));
    candidate_filenames.push(format!("{snake}.svg"));

    // Pattern 5: <kebab>.png
    candidate_filenames.push(format!("{kebab}.png"));
    candidate_filenames.push(format!("{kebab}.svg"));

    // Pattern 6: Direct stem matches
    candidate_filenames.push(format!("{action_stem}.png"));
    candidate_filenames.push(format!("{raw_stem_lower}.png"));
    candidate_filenames.push(format!("{clean}.png"));
    candidate_filenames.push(format!("{}.png", clean.to_lowercase()));

    // Pattern 7: Fallback action icon
    candidate_filenames.push("action.png".into());
    candidate_filenames.push("action_launch.png".into());

    for fname in candidate_filenames {
        // Strict blacklist: Never use wide store banners or thumbnails as action icons
        if fname.contains("thumbnail") || fname.contains("banner") || fname.contains("preview") {
            continue;
        }
        let candidate_path = assets.join(&fname);
        if candidate_path.exists() {
            return candidate_path.to_string_lossy().to_string();
        }
    }

    // 3. Fallback to 1:1 square plugin icon if available (strictly excluding thumbnail/banner)
    let square_icons = ["icon.png", "plugin_icon.png", "logo.png"];
    for sq in square_icons {
        let p = assets.join(sq);
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }

    "system-run-symbolic".into()
}

pub fn extract_row_arg(chunk: &str, param: &str) -> Option<String> {
    let needle = format!("{param}=");
    let pos = chunk.find(&needle)?;
    let after = &chunk[pos + needle.len()..];
    let quote_idx = after.find(|c| c == '"' || c == '\'')?;
    let quote_char = after.as_bytes()[quote_idx] as char;
    let rest = &after[quote_idx + 1..];
    let end_quote = rest.find(quote_char)?;
    Some(rest[..end_quote].to_string())
}

pub fn parse_action_config_rows(action_py_path: &Path) -> Vec<ActionConfigRow> {
    if !action_py_path.exists() {
        return Vec::new();
    }
    let content = match fs::read_to_string(action_py_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let idx = match content.find("def get_config_rows(") {
        Some(i) => i,
        None => return Vec::new(),
    };

    let slice = &content[idx..content.len().min(idx + 3500)];
    let end_idx = slice[20..].find("\n    def ").map(|i| i + 20).unwrap_or(slice.len());
    let body = &slice[..end_idx];

    let mut rows = Vec::new();
    let mut pos = 0;
    while let Some(adw_pos) = body[pos..].find("Adw.") {
        let actual_pos = pos + adw_pos;
        let end_call = match body[actual_pos..].find(')') {
            Some(e) => actual_pos + e,
            None => break,
        };
        let chunk = &body[actual_pos..=end_call];
        pos = end_call + 1;

        let row_type = if chunk.contains("ComboRow") {
            "ComboRow"
        } else if chunk.contains("SwitchRow") {
            "SwitchRow"
        } else if chunk.contains("EntryRow") {
            "EntryRow"
        } else if chunk.contains("SpinRow") {
            "SpinRow"
        } else {
            continue;
        };

        let title = extract_row_arg(chunk, "title").unwrap_or_else(|| "Setting".into());
        let subtitle = extract_row_arg(chunk, "subtitle").unwrap_or_default();

        let clean_key = title
            .chars()
            .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect::<String>()
            .split('_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("_");

        match row_type {
            "ComboRow" => {
                let lt = title.to_lowercase();
                let options = if lt.contains("server") {
                    vec!["Select a Server...".into(), "Current Active Call".into(), "My Discord Server".into()]
                } else if lt.contains("channel") {
                    vec!["Select a Channel...".into(), "General Voice".into(), "Gaming Room".into(), "Lounge".into()]
                } else if lt.contains("volume step") {
                    vec!["1%".into(), "2%".into(), "5%".into(), "10%".into()]
                } else if lt.contains("sound") {
                    vec!["Select Sound...".into(), "Airhorn".into(), "Applause".into(), "Buzzer".into(), "Victory Fanfare".into()]
                } else if lt.contains("device") {
                    vec!["Select Govee Device...".into(), "Living Room Light".into(), "Desk Strip".into()]
                } else if lt.contains("mode") || lt.contains("action type") {
                    vec!["Toggle Power".into(), "Set Brightness".into(), "Set Color".into(), "Apply Scene".into()]
                } else if lt.contains("scene") {
                    vec!["Select Scene...".into(), "Sunrise".into(), "Sunset".into(), "Reading".into(), "Night".into()]
                } else {
                    vec!["Select Option...".into()]
                };

                rows.push(ActionConfigRow {
                    key: clean_key,
                    setting_type: ActionSettingType::Combo {
                        title,
                        subtitle,
                        options,
                        default_idx: 0,
                    },
                });
            }
            "SwitchRow" => {
                let default_active = chunk.contains("True") || body.contains(&format!("{}.set_active(True)", clean_key));
                rows.push(ActionConfigRow {
                    key: clean_key,
                    setting_type: ActionSettingType::Switch {
                        title,
                        subtitle,
                        default_active,
                    },
                });
            }
            "EntryRow" => {
                rows.push(ActionConfigRow {
                    key: clean_key,
                    setting_type: ActionSettingType::Text {
                        title,
                        subtitle,
                        default_text: String::new(),
                    },
                });
            }
            "SpinRow" => {
                rows.push(ActionConfigRow {
                    key: clean_key,
                    setting_type: ActionSettingType::Spin {
                        title,
                        subtitle,
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        default_val: 50.0,
                    },
                });
            }
            _ => {}
        }
    }

    rows
}
