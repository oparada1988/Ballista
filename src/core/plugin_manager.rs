use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginActionInfo {
    pub id: String,
    pub name: String,
    pub icon_name_or_file: String,
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
        if let Ok(entries) = fs::read_dir(&self.plugins_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let manifest_file = p.join("manifest.json");
                    if manifest_file.exists() {
                        if let Ok(data) = fs::read_to_string(&manifest_file) {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                                let id = json.get("id").and_then(|v| v.as_str()).unwrap_or("unknown_plugin").to_string();
                                let name = json.get("name").and_then(|v| v.as_str()).unwrap_or("Unnamed Plugin").to_string();
                                let author = json.get("author").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                                let version = json.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string();
                                let description = json.get("description")
                                    .or_else(|| json.get("short-description"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                // Discovered actions
                                let mut actions = Vec::new();
                                if id.contains("AppLauncher") {
                                    let action_launch_icon = p.join("assets").join("action_launch.png");
                                    let icon_str = if action_launch_icon.exists() {
                                        action_launch_icon.to_string_lossy().to_string()
                                    } else {
                                        "system-run-symbolic".into()
                                    };
                                    actions.push(PluginActionInfo {
                                        id: format!("{id}::LaunchAppAction"),
                                        name: "Launch App".into(),
                                        icon_name_or_file: icon_str,
                                    });
                                } else {
                                    let action_icon = p.join("assets").join("action.png");
                                    let icon_str = if action_icon.exists() {
                                        action_icon.to_string_lossy().to_string()
                                    } else {
                                        "system-run-symbolic".into()
                                    };
                                    actions.push(PluginActionInfo {
                                        id: format!("{id}::DefaultAction"),
                                        name: "Execute Action".into(),
                                        icon_name_or_file: icon_str,
                                    });
                                }

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
}
