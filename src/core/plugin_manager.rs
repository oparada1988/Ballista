use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use crate::models::KeyAppearance;

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
}

#[derive(Clone)]
pub struct PluginManager {
    pub plugins_dir: PathBuf,
    pub plugins: HashMap<String, DiscoveredPlugin>,
    pub installed_apps: Vec<InstalledApp>,
}

impl PluginManager {
    pub fn new() -> Self {
        let plugins_dir = glib::user_data_dir().join("ballista").join("plugins");
        let _ = fs::create_dir_all(&plugins_dir);

        let mut manager = Self {
            plugins_dir,
            plugins: HashMap::new(),
            installed_apps: Vec::new(),
        };

        manager.scan_installed_apps();
        manager.rescan_plugins();
        manager
    }

    pub fn scan_installed_apps(&mut self) {
        let mut apps = Vec::new();
        let app_dirs = [
            PathBuf::from("/usr/share/applications"),
            glib::user_data_dir().join("applications"),
        ];

        for dir in app_dirs {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                        if let Ok(content) = fs::read_to_string(&path) {
                            let mut name = None;
                            let mut exec = None;
                            let mut icon = None;
                            let mut no_display = false;

                            for line in content.lines() {
                                let line = line.trim();
                                if line.starts_with("Name=") && name.is_none() {
                                    name = Some(line.strip_prefix("Name=").unwrap().to_string());
                                } else if line.starts_with("Exec=") && exec.is_none() {
                                    exec = Some(line.strip_prefix("Exec=").unwrap().to_string());
                                } else if line.starts_with("Icon=") && icon.is_none() {
                                    icon = Some(line.strip_prefix("Icon=").unwrap().to_string());
                                } else if line == "NoDisplay=true" {
                                    no_display = true;
                                }
                            }

                            if !no_display {
                                if let (Some(n), Some(e)) = (name, exec) {
                                    let desktop_id = path.file_name().unwrap().to_string_lossy().to_string();
                                    apps.push(InstalledApp {
                                        desktop_id,
                                        name: n,
                                        exec: e,
                                        icon: icon.unwrap_or_else(|| "application-x-executable".into()),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        apps.dedup_by(|a, b| a.name == b.name);
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
                                        id,
                                        name,
                                        author,
                                        version,
                                        description,
                                        path: p,
                                        actions,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Sideload / Install from a local directory (e.g. /home/oscar/Github Projects/AppLauncher)
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

        self.rescan_plugins();
        Ok(id.to_string())
    }

    /// Download & Install a plugin directly from a zip archive URL (e.g. GitHub release or main branch)
    pub fn install_from_url(&mut self, id: &str, download_url: &str) -> Result<String, String> {
        let dest = self.plugins_dir.join(id);
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::create_dir_all(&dest);

        let temp_zip = std::env::temp_dir().join(format!("{id}.zip"));
        let temp_extract = std::env::temp_dir().join(format!("{id}_extracted"));
        let _ = fs::remove_file(&temp_zip);
        let _ = fs::remove_dir_all(&temp_extract);

        let status = std::process::Command::new("curl")
            .arg("-L")
            .arg("-s")
            .arg("-o")
            .arg(&temp_zip)
            .arg(download_url)
            .status()
            .map_err(|e| format!("Failed to download: {e}"))?;

        if !status.success() {
            return Err("Failed to download plugin archive".into());
        }

        let unzip_status = std::process::Command::new("unzip")
            .arg("-q")
            .arg("-o")
            .arg(&temp_zip)
            .arg("-d")
            .arg(&temp_extract)
            .status()
            .map_err(|e| format!("Failed to unzip: {e}"))?;

        if !unzip_status.success() {
            return Err("Failed to extract plugin archive".into());
        }

        // GitHub zips contain an inner folder (e.g. AppLauncher-main)
        // Find folder containing manifest.json
        let mut source_folder = temp_extract.clone();
        if let Ok(entries) = fs::read_dir(&temp_extract) {
            for entry in entries.flatten() {
                if entry.path().is_dir() && entry.path().join("manifest.json").exists() {
                    source_folder = entry.path();
                    break;
                }
            }
        }

        Self::copy_dir_all(&source_folder, &dest).map_err(|e| format!("Failed to copy plugin: {e}"))?;
        let _ = fs::remove_file(&temp_zip);
        let _ = fs::remove_dir_all(&temp_extract);

        self.rescan_plugins();
        Ok(id.to_string())
    }

    pub fn uninstall_plugin(&mut self, plugin_id: &str) -> Result<(), String> {
        let dest = self.plugins_dir.join(plugin_id);
        if dest.exists() {
            fs::remove_dir_all(&dest).map_err(|e| format!("Failed to remove plugin directory: {e}"))?;
        }
        self.plugins.remove(plugin_id);
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
        let _ = std::process::Command::new("gtk-launch")
            .arg(desktop_id)
            .spawn()
            .or_else(|_| {
                std::process::Command::new("gio")
                    .arg("launch")
                    .arg(format!("/usr/share/applications/{desktop_id}"))
                    .spawn()
            })
            .map_err(|e| format!("Failed to spawn launcher: {e}"))?;

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
