#![allow(deprecated)]

use crate::core::PluginManager;
use crate::models::WindowState;
use adw::prelude::*;
use adw::{ActionRow, Carousel, CarouselIndicatorDots, Clamp, HeaderBar, ToolbarView, Window};
use gtk::{
    Box, Button, FileDialog, GestureClick, Grid, Image, Label, Orientation, Picture,
    ScrolledWindow, SearchEntry, Stack, StackTransitionType,
};
use std::cell::RefCell;
#[allow(unused_imports)]
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginRuntime {
    Ballista,
    StreamController,
}

#[derive(Clone, Debug)]
pub struct PluginActionPreview {
    pub name: String,
    pub description: String,
    pub surfaces: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub author: String,
    pub license: String,
    pub version: String,
    pub runtime: PluginRuntime,
    pub icon: String,
    pub thumbnail: Option<String>,
    pub is_file_icon: bool,
    pub fallback_grad: String,
    pub short_description: String,
    pub description: String,
    pub tags: Vec<String>,
    pub github_url: Option<String>,
    pub download_url: Option<String>,
    pub screenshots: Vec<String>,
    pub surfaces: Vec<String>,
    pub compatible_devices: Vec<String>,
    pub actions: Vec<PluginActionPreview>,
    pub rating: Option<f64>,
    pub rating_count: u32,
    pub local_path: Option<String>,
}

struct MarketplaceCatalog {
    plugins: Vec<PluginInfo>,
    spotlight_ids: Vec<String>,
}

fn resolve_spotlight_plugins<'a>(
    plugins: &'a [PluginInfo],
    spotlight_ids: &[String],
) -> Vec<&'a PluginInfo> {
    let mut resolved: Vec<&'a PluginInfo> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for sid in spotlight_ids {
        if let Some(plugin) = plugins.iter().find(|p| p.id == *sid) {
            if seen.insert(plugin.id.as_str()) {
                resolved.push(plugin);
            }
        }
    }

    if !resolved.is_empty() {
        resolved
    } else {
        plugins.iter().take(6).collect()
    }
}

fn is_newer_version(candidate: &str, installed: &str) -> bool {
    let parse = |s: &str| -> Vec<u32> {
        s.trim()
            .trim_start_matches('v')
            .split(|c: char| c == '.' || c == '-')
            .filter_map(|part| part.parse::<u32>().ok())
            .collect()
    };
    let a = parse(candidate);
    let b = parse(installed);
    if a.is_empty() || b.is_empty() {
        return candidate.trim_start_matches('v') > installed.trim_start_matches('v');
    }
    let len = a.len().max(b.len());
    for i in 0..len {
        let av = *a.get(i).unwrap_or(&0);
        let bv = *b.get(i).unwrap_or(&0);
        if av != bv {
            return av > bv;
        }
    }
    false
}

fn format_version_tag(ver: &str) -> String {
    let trimmed = ver.trim();
    if trimmed.is_empty() {
        "v0.1.0".to_string()
    } else if trimmed.starts_with('v') {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}

fn format_device_summary(devices: &[String]) -> String {
    if devices.is_empty() {
        return String::new();
    }
    let short_names: Vec<&str> = devices
        .iter()
        .map(|d| match d.as_str() {
            "StreamDeckPlus" => "+",
            "StreamDeckPlusXL" => "+ XL",
            "StreamDeckXL" => "XL",
            "StreamDeckMK2" => "MK.2",
            "StreamDeckMini" => "Mini",
            "StreamDeckNeo" => "Neo",
            "StreamDeckOriginal" => "Original",
            "StreamDeckPedal" => "Pedal",
            "StreamDeckStudio" => "Studio",
            other => other,
        })
        .collect();
    format!("• Stream Deck {}", short_names.join(", "))
}

fn market_plugin_cache_dir(plugin_id: &str) -> PathBuf {
    let dir = glib::user_cache_dir()
        .join("ballista")
        .join("market_images")
        .join(plugin_id);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn load_marketplace_catalog() -> MarketplaceCatalog {
    let cache_dir = glib::user_cache_dir().join("ballista");
    let _ = std::fs::create_dir_all(&cache_dir);
    let cache_file = cache_dir.join("market_catalog.json");
    let img_cache_dir = cache_dir.join("market_images");
    let _ = std::fs::create_dir_all(&img_cache_dir);

    // Fetch live catalog from Ballista-Market atomically with 3s timeout (preserves cache when offline)
    let url = "https://raw.githubusercontent.com/oparada1988/Ballista-Market/main/catalog.json";
    let tmp_cache = cache_dir.join("market_catalog.json.tmp");
    let fetch_ok = std::process::Command::new("curl")
        .arg("-fsSL")
        .arg("-m")
        .arg("3")
        .arg("-o")
        .arg(&tmp_cache)
        .arg(url)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if fetch_ok && tmp_cache.exists() {
        let _ = std::fs::rename(&tmp_cache, &cache_file);
    } else {
        let _ = std::fs::remove_file(&tmp_cache);
    }

    if let Ok(data) = std::fs::read_to_string(&cache_file) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
            if let Some(arr) = val.get("plugins").and_then(|v| v.as_array()) {
                let spotlight_ids: Vec<String> = val
                    .get("spotlight")
                    .and_then(|v| v.as_array())
                    .map(|sarr| {
                        sarr.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .filter(|s| !s.trim().is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                let mut list = Vec::new();
                for p in arr {
                    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let author = p.get("author").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                    let license = p.get("license").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let version = p.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string();
                    let runtime = match p
                        .get("runtime")
                        .or_else(|| p.get("plugin_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("streamcontroller")
                        .to_lowercase()
                        .as_str()
                    {
                        "ballista" | "native" => PluginRuntime::Ballista,
                        _ => PluginRuntime::StreamController,
                    };
                    let category = p.get("category").and_then(|v| v.as_str()).unwrap_or("Utility").to_string();
                    let description = p.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let short_description = p
                        .get("short_description")
                        .or_else(|| p.get("short-description"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            description
                                .split('\n')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_string()
                        });
                    let tags: Vec<String> = p
                        .get("tags")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let github_url = p
                        .get("github_url")
                        .or_else(|| p.get("repository_url"))
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| s.to_string());
                    let download_url = p.get("download_url").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let screenshots: Vec<String> = p
                        .get("screenshots")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .filter(|s| !s.trim().is_empty())
                                .take(6)
                                .collect()
                        })
                        .unwrap_or_default();
                    let surfaces: Vec<String> = p
                        .get("surfaces")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_lowercase()))
                                .collect()
                        })
                        .unwrap_or_else(|| vec!["keys".to_string(), "dials".to_string()]);
                    let compatible_devices: Vec<String> = p
                        .get("compatible_devices")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    let actions: Vec<PluginActionPreview> = p
                        .get("actions")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|a| {
                                    let a_name = a.get("name")?.as_str()?.to_string();
                                    let a_desc = a
                                        .get("description")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let a_surfs = a
                                        .get("surfaces")
                                        .and_then(|v| v.as_array())
                                        .map(|sa| {
                                            sa.iter()
                                                .filter_map(|x| x.as_str().map(|s| s.to_lowercase()))
                                                .collect()
                                        })
                                        .unwrap_or_else(|| vec!["keys".to_string()]);
                                    Some(PluginActionPreview {
                                        name: a_name,
                                        description: a_desc,
                                        surfaces: a_surfs,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let rating = p.get("rating").and_then(|v| v.as_f64());
                    let rating_count = p.get("rating_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    let local_path: Option<String> = None;

                    let per_plugin_cache = market_plugin_cache_dir(&id);

                    // Cache and resolve thumbnail (prefer per-plugin dir, fall back to legacy flat cache)
                    let thumb_url = p.get("thumbnail").and_then(|v| v.as_str());
                    let thumbnail = if let Some(t_url) = thumb_url {
                        if t_url.starts_with("http") {
                            let thumb_file = per_plugin_cache.join("thumb.png");
                            let legacy_thumb = img_cache_dir.join(format!("{id}_thumb.png"));
                            if !thumb_file.exists() && legacy_thumb.exists() {
                                let _ = std::fs::copy(&legacy_thumb, &thumb_file);
                            }
                            if !thumb_file.exists() {
                                let _ = std::process::Command::new("curl")
                                    .arg("-s")
                                    .arg("-m")
                                    .arg("5")
                                    .arg("-o")
                                    .arg(&thumb_file)
                                    .arg(t_url)
                                    .status();
                            }
                            if thumb_file.exists() {
                                Some(thumb_file.to_string_lossy().to_string())
                            } else {
                                None
                            }
                        } else {
                            Some(t_url.to_string())
                        }
                    } else {
                        None
                    };

                    // Cache and resolve icon
                    let icon_url = p.get("icon").and_then(|v| v.as_str()).unwrap_or("system-run-symbolic");
                    let (icon, is_file_icon) = if icon_url.starts_with("http") {
                        let icon_file = per_plugin_cache.join("icon.png");
                        let legacy_icon = img_cache_dir.join(format!("{id}_icon.png"));
                        if !icon_file.exists() && legacy_icon.exists() {
                            let _ = std::fs::copy(&legacy_icon, &icon_file);
                        }
                        if !icon_file.exists() {
                            let _ = std::process::Command::new("curl")
                                .arg("-s")
                                .arg("-m")
                                .arg("5")
                                .arg("-o")
                                .arg(&icon_file)
                                .arg(icon_url)
                                .status();
                        }
                        if icon_file.exists() {
                            (icon_file.to_string_lossy().to_string(), true)
                        } else {
                            ("system-run-symbolic".to_string(), false)
                        }
                    } else if icon_url.ends_with(".png") || icon_url.ends_with(".svg") {
                        (icon_url.to_string(), true)
                    } else {
                        (icon_url.to_string(), false)
                    };

                    let fallback_grad = match category.as_str() {
                        "Streaming" => "thumb-grad-streaming".to_string(),
                        "System" => "thumb-grad-system".to_string(),
                        "Audio" => "thumb-grad-audio".to_string(),
                        "Developer" => "thumb-grad-developer".to_string(),
                        "Productivity" => "thumb-grad-productivity".to_string(),
                        _ => "thumb-grad-utility".to_string(),
                    };

                    list.push(PluginInfo {
                        id,
                        name,
                        category,
                        author,
                        license,
                        version,
                        runtime,
                        icon,
                        thumbnail,
                        is_file_icon,
                        fallback_grad,
                        short_description,
                        description,
                        tags,
                        github_url,
                        download_url,
                        screenshots,
                        surfaces,
                        compatible_devices,
                        actions,
                        rating,
                        rating_count,
                        local_path,
                    });
                }
                return MarketplaceCatalog {
                    plugins: list,
                    spotlight_ids,
                };
            }
        }
    }

    // Bundled offline fallback (100% portable using XDG cache dirs and remote GitHub URLs)
    MarketplaceCatalog {
        plugins: vec![
        PluginInfo {
            id: "com_oparada_AppLauncher".into(),
            name: "App Launcher".into(),
            category: "Utility".into(),
            author: "oparada1988".into(),
            license: "GPL-3.0-or-later".into(),
            version: "0.1.0".into(),
            runtime: PluginRuntime::StreamController,
            icon: "action_launch.png".into(),
            thumbnail: {
                let p = img_cache_dir.join("com_oparada_AppLauncher_thumb.png");
                p.exists().then(|| p.to_string_lossy().to_string())
            },
            is_file_icon: true,
            fallback_grad: "thumb-grad-utility".into(),
            short_description: "Launch any installed Linux desktop app, Flatpak, or Snap with automatic icon setup.".into(),
            description: "Easily launch any installed host desktop application from your Stream Deck keys with automatic freedesktop icon and label setup.".into(),
            tags: vec!["Utility".into(), "System".into(), "Launcher".into()],
            github_url: Some("https://github.com/oparada1988/AppLauncher".into()),
            download_url: Some("https://github.com/oparada1988/AppLauncher/archive/refs/heads/main.zip".into()),
            screenshots: Vec::new(),
            surfaces: vec!["keys".into()],
            compatible_devices: vec!["StreamDeckPlus".into(), "StreamDeckXL".into(), "StreamDeckMK2".into(), "StreamDeckMini".into()],
            actions: vec![PluginActionPreview {
                name: "Launch App".into(),
                description: "Pick any system, Flatpak, or Snap app with automatic icon setup".into(),
                surfaces: vec!["keys".into()],
            }],
            rating: None,
            rating_count: 0,
            local_path: None,
        },
        PluginInfo {
            id: "com_oparada_DiscordSC".into(),
            name: "DiscordSC".into(),
            category: "Streaming".into(),
            author: "oparada1988".into(),
            license: "GPL-3.0-or-later".into(),
            version: "1.2.4".into(),
            runtime: PluginRuntime::StreamController,
            icon: "user-available-symbolic".into(),
            thumbnail: {
                let p = img_cache_dir.join("com_oparada_DiscordSC_thumb.png");
                p.exists().then(|| p.to_string_lossy().to_string())
            },
            is_file_icon: false,
            fallback_grad: "thumb-grad-streaming".into(),
            short_description: "Full Discord voice, soundboard, channel, call mixer & notification control over local IPC.".into(),
            description: "Control your Discord client's mute, deafen, notifications, server status, call mixer, and input modes directly from your Stream Deck.".into(),
            tags: vec!["Streaming".into(), "Communication".into(), "Discord".into()],
            github_url: Some("https://github.com/oparada1988/DiscordSC".into()),
            download_url: Some("https://github.com/oparada1988/DiscordSC/archive/refs/heads/main.zip".into()),
            screenshots: Vec::new(),
            surfaces: vec!["keys".into(), "dials".into(), "touchscreen".into()],
            compatible_devices: vec!["StreamDeckPlus".into(), "StreamDeckXL".into(), "StreamDeckMK2".into(), "StreamDeckMini".into()],
            actions: vec![
                PluginActionPreview { name: "Mute Toggle".into(), description: "Toggle Discord microphone mute with live state".into(), surfaces: vec!["keys".into()] },
                PluginActionPreview { name: "Deafen Toggle".into(), description: "Instantly deafen and mute active Discord voice".into(), surfaces: vec!["keys".into()] },
                PluginActionPreview { name: "Call Mixer".into(), description: "Adjust per-participant volume & mute on dials and LCD".into(), surfaces: vec!["dials".into(), "touchscreen".into()] },
                PluginActionPreview { name: "Soundboard".into(), description: "Play server soundboard clips in active voice".into(), surfaces: vec!["keys".into()] },
            ],
            rating: None,
            rating_count: 0,
            local_path: None,
        },
        PluginInfo {
            id: "com_oparada_GoveeStreamController".into(),
            name: "Govee Controller".into(),
            category: "Utility".into(),
            author: "oparada1988".into(),
            license: "GPL-3.0-or-later".into(),
            version: "1.0.1".into(),
            runtime: PluginRuntime::StreamController,
            icon: "lightbulb-symbolic".into(),
            thumbnail: {
                let p = img_cache_dir.join("com_oparada_GoveeStreamController_thumb.png");
                p.exists().then(|| p.to_string_lossy().to_string())
            },
            is_file_icon: false,
            fallback_grad: "thumb-grad-utility".into(),
            short_description: "Control Govee smart lights, LED strips, scenes, and home appliances.".into(),
            description: "Control Govee smart lights, LED strips, and home appliances directly from your Stream Deck.".into(),
            tags: vec!["Smart Home".into(), "Lighting".into(), "Utility".into()],
            github_url: Some("https://github.com/oparada1988/Govee-Streamcontroller".into()),
            download_url: Some("https://github.com/oparada1988/Govee-Streamcontroller/archive/refs/heads/main.zip".into()),
            screenshots: Vec::new(),
            surfaces: vec!["keys".into(), "dials".into()],
            compatible_devices: vec!["StreamDeckPlus".into(), "StreamDeckXL".into(), "StreamDeckMK2".into(), "StreamDeckMini".into()],
            actions: vec![
                PluginActionPreview { name: "Power Toggle".into(), description: "Turn Govee lights and smart appliances on or off".into(), surfaces: vec!["keys".into()] },
                PluginActionPreview { name: "Brightness Control".into(), description: "Set or step light brightness percentages".into(), surfaces: vec!["keys".into(), "dials".into()] },
                PluginActionPreview { name: "Color Picker".into(), description: "Apply custom RGB color presets".into(), surfaces: vec!["keys".into()] },
                PluginActionPreview { name: "Scene Trigger".into(), description: "Activate built-in Govee lighting scenes".into(), surfaces: vec!["keys".into()] },
            ],
            rating: None,
            rating_count: 0,
            local_path: None,
        },
        ],
        spotlight_ids: Vec::new(),
    }
}

pub fn create_store_window(
    parent: &impl IsA<gtk::Window>,
    plugin_manager: Rc<RefCell<PluginManager>>,
    on_plugin_installed: Rc<dyn Fn()>,
    on_open_plugin_settings: Rc<dyn Fn(String, String)>,
) -> Window {
    let _saved_state = WindowState::load();

    let window = Window::builder()
        .title("Plugin Store")
        .transient_for(parent)
        .modal(true)
        .default_width(940)
        .default_height(840)
        .resizable(false)
        .build();

    window.set_size_request(940, 840);
    window.add_css_class("ballista-modal");
    window.add_css_class("ballista-midnight-window");

    // Persist store window dimensions on close
    window.connect_close_request(|win| {
        let mut state = WindowState::load();
        let w = win.width().max(760);
        let h = win.height().max(560);
        state.store_width = w;
        state.store_height = h;
        state.save();
        glib::Propagation::Proceed
    });

    let toolbar_view = ToolbarView::new();

    // ===================== Header Bar =====================
    let header_bar = HeaderBar::new();
    header_bar.add_css_class("ballista-headerbar");

    let install_local_popover = gtk::Popover::new();
    let install_menu_box = Box::new(Orientation::Vertical, 4);
    install_menu_box.set_margin_top(6);
    install_menu_box.set_margin_bottom(6);
    install_menu_box.set_margin_start(6);
    install_menu_box.set_margin_end(6);

    let install_zip_btn = Button::new();
    install_zip_btn.add_css_class("flat");
    let zip_btn_box = Box::new(Orientation::Horizontal, 8);
    zip_btn_box.append(&Image::from_icon_name("package-x-generic-symbolic"));
    zip_btn_box.append(&Label::new(Some("Install from ZIP (.zip)...")));
    install_zip_btn.set_child(Some(&zip_btn_box));

    let install_folder_btn = Button::new();
    install_folder_btn.add_css_class("flat");
    let folder_btn_box = Box::new(Orientation::Horizontal, 8);
    folder_btn_box.append(&Image::from_icon_name("folder-open-symbolic"));
    folder_btn_box.append(&Label::new(Some("Install from Folder...")));
    install_folder_btn.set_child(Some(&folder_btn_box));

    install_menu_box.append(&install_zip_btn);
    install_menu_box.append(&install_folder_btn);
    install_local_popover.set_child(Some(&install_menu_box));

    let install_local_btn = gtk::MenuButton::builder()
        .label("Install Local")
        .popover(&install_local_popover)
        .tooltip_text("Install a plugin from a .zip archive or extracted folder")
        .build();
    install_local_btn.add_css_class("flat");
    header_bar.pack_start(&install_local_btn);

    let back_btn = Button::from_icon_name("go-previous-symbolic");
    back_btn.add_css_class("flat");
    back_btn.set_tooltip_text(Some("Back to Store"));
    back_btn.set_visible(false);
    header_bar.pack_start(&back_btn);

    // "My Plugins" header button with purple badge
    let my_plugins_btn = Button::with_label("My Plugins");
    my_plugins_btn.add_css_class("store-updates-btn");
    my_plugins_btn.set_tooltip_text(Some("Manage installed plugins and check for updates"));
    header_bar.pack_end(&my_plugins_btn);

    toolbar_view.add_top_bar(&header_bar);

    // Navigation Stack between Main Store view, Plugin Detail view, Installed Plugins view, and Category view
    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::SlideLeftRight);
    let last_page: Rc<RefCell<&'static str>> = Rc::new(RefCell::new("main"));

    // =========================================================================
    // Page 1: Main Store Browse View
    // =========================================================================
    let main_content_box = Box::new(Orientation::Vertical, 16);
    main_content_box.set_margin_top(16);
    main_content_box.set_margin_bottom(16);
    main_content_box.set_margin_start(20);
    main_content_box.set_margin_end(20);

    // 1. Compact Search Bar (Centered, restrained width)
    let search_entry = SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search plugins..."));
    search_entry.add_css_class("store-search-entry");
    search_entry.set_size_request(340, -1);
    search_entry.set_halign(gtk::Align::Center);
    search_entry.set_hexpand(false);
    main_content_box.append(&search_entry);

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let scroll_content = Box::new(Orientation::Vertical, 20);
    scroll_content.set_margin_end(4);

    // Spotlight Header
    let spotlight_header = Box::new(Orientation::Horizontal, 8);
    let spotlight_title = Label::new(Some("Spotlight"));
    spotlight_title.add_css_class("store-section-title");
    spotlight_title.set_hexpand(true);
    spotlight_title.set_xalign(0.0);

    let prev_btn = Button::from_icon_name("go-previous-symbolic");
    prev_btn.add_css_class("flat");
    prev_btn.add_css_class("circular");
    prev_btn.set_tooltip_text(Some("Previous Spotlight slide"));

    let next_btn = Button::from_icon_name("go-next-symbolic");
    next_btn.add_css_class("flat");
    next_btn.add_css_class("circular");
    next_btn.set_tooltip_text(Some("Next Spotlight slide"));

    spotlight_header.append(&spotlight_title);
    spotlight_header.append(&prev_btn);
    spotlight_header.append(&next_btn);
    scroll_content.append(&spotlight_header);

    // Live Marketplace Catalog queried directly from Ballista-Market
    let marketplace_catalog = load_marketplace_catalog();
    let plugins_catalog = marketplace_catalog.plugins;
    let spotlight_ids = marketplace_catalog.spotlight_ids;

    // =========================================================================
    // Page 2: Plugin Detail Subview (Two-Column Hero + 16:9 Gallery + Actions)
    // =========================================================================
    let detail_page_box = Box::new(Orientation::Vertical, 0);

    let detail_scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let detail_scroll_content = Box::new(Orientation::Vertical, 20);
    detail_scroll_content.set_margin_top(22);
    detail_scroll_content.set_margin_bottom(22);
    detail_scroll_content.set_margin_start(26);
    detail_scroll_content.set_margin_end(26);

    // --- Two-Column Hero ---
    let detail_hero_box = Box::new(Orientation::Horizontal, 24);
    detail_hero_box.set_valign(gtk::Align::Start);

    // Left Metadata Column (156px)
    let detail_left_col = Box::new(Orientation::Vertical, 10);
    detail_left_col.set_size_request(156, -1);
    detail_left_col.set_valign(gtk::Align::Start);

    let detail_icon_frame = Box::new(Orientation::Vertical, 0);
    detail_icon_frame.add_css_class("store-detail-icon-frame");
    detail_icon_frame.set_size_request(148, 148);
    detail_icon_frame.set_halign(gtk::Align::Center);
    detail_icon_frame.set_valign(gtk::Align::Center);
    detail_icon_frame.set_overflow(gtk::Overflow::Hidden);

    let detail_icon_pic = Picture::new();
    detail_icon_pic.set_can_shrink(true);
    detail_icon_pic.set_content_fit(gtk::ContentFit::Contain);
    detail_icon_pic.set_size_request(128, 128);
    detail_icon_pic.set_margin_top(10);
    detail_icon_pic.set_margin_bottom(10);
    detail_icon_pic.set_margin_start(10);
    detail_icon_pic.set_margin_end(10);
    detail_icon_pic.set_visible(false);

    let detail_icon_sym = Image::from_icon_name("system-run-symbolic");
    detail_icon_sym.set_pixel_size(76);
    detail_icon_sym.set_halign(gtk::Align::Center);
    detail_icon_sym.set_valign(gtk::Align::Center);
    detail_icon_sym.set_vexpand(true);

    detail_icon_frame.append(&detail_icon_pic);
    detail_icon_frame.append(&detail_icon_sym);
    detail_left_col.append(&detail_icon_frame);

    // Runtime Type Badge (Ballista Native vs StreamController)
    let detail_runtime_wrap = Box::new(Orientation::Vertical, 4);
    let detail_runtime_badge = Box::new(Orientation::Horizontal, 6);
    detail_runtime_badge.add_css_class("store-runtime-badge");
    detail_runtime_badge.set_halign(gtk::Align::Fill);
    let detail_runtime_inner = Box::new(Orientation::Horizontal, 6);
    detail_runtime_inner.set_halign(gtk::Align::Center);
    detail_runtime_inner.set_hexpand(true);
    let detail_runtime_ico = Image::from_icon_name("emblem-system-symbolic");
    detail_runtime_ico.set_pixel_size(13);
    let detail_runtime_lbl = Label::new(Some("StreamController"));
    detail_runtime_inner.append(&detail_runtime_ico);
    detail_runtime_inner.append(&detail_runtime_lbl);
    detail_runtime_badge.append(&detail_runtime_inner);

    let detail_runtime_sub = Label::new(Some("Runs via Built-in Bridge"));
    detail_runtime_sub.add_css_class("store-runtime-sub");
    detail_runtime_sub.set_halign(gtk::Align::Center);

    detail_runtime_wrap.append(&detail_runtime_badge);
    detail_runtime_wrap.append(&detail_runtime_sub);
    detail_left_col.append(&detail_runtime_wrap);

    let meta_div = Box::new(Orientation::Horizontal, 0);
    meta_div.add_css_class("store-meta-divider");
    detail_left_col.append(&meta_div);

    // Structured Metadata Stack (Author, License, Version, Source & Issues)
    let detail_meta_list = Box::new(Orientation::Vertical, 7);

    let make_meta_row = |icon_name: &str| -> (Box, Label) {
        let row = Box::new(Orientation::Horizontal, 8);
        let ico = Image::from_icon_name(icon_name);
        ico.set_pixel_size(14);
        ico.add_css_class("store-meta-icon");
        let lbl = Label::new(None);
        lbl.add_css_class("store-meta-row");
        lbl.set_xalign(0.0);
        lbl.set_ellipsize(pango::EllipsizeMode::End);
        row.append(&ico);
        row.append(&lbl);
        (row, lbl)
    };

    let (meta_author_row, detail_author_lbl) = make_meta_row("avatar-default-symbolic");
    let (meta_license_row, detail_license_lbl) = make_meta_row("text-x-generic-symbolic");
    let (meta_version_row, detail_version_lbl) = make_meta_row("software-update-available-symbolic");
    let detail_version_arrow = Label::new(None);
    detail_version_arrow.add_css_class("store-meta-update-arrow");
    detail_version_arrow.set_visible(false);
    meta_version_row.append(&detail_version_arrow);

    let detail_source_btn = Button::new();
    detail_source_btn.add_css_class("store-source-link");
    detail_source_btn.set_halign(gtk::Align::Start);
    detail_source_btn.set_cursor_from_name(Some("pointer"));
    let src_btn_box = Box::new(Orientation::Horizontal, 8);
    let src_ico = Image::from_icon_name("web-browser-symbolic");
    src_ico.set_pixel_size(14);
    let src_lbl = Label::new(Some("Source & Issues"));
    src_btn_box.append(&src_ico);
    src_btn_box.append(&src_lbl);
    detail_source_btn.set_child(Some(&src_btn_box));

    detail_meta_list.append(&meta_author_row);
    detail_meta_list.append(&meta_license_row);
    detail_meta_list.append(&meta_version_row);
    detail_meta_list.append(&detail_source_btn);
    detail_left_col.append(&detail_meta_list);

    detail_hero_box.append(&detail_left_col);

    // Right Content Column (Title + CTA + Rating + Description Card)
    let detail_right_col = Box::new(Orientation::Vertical, 14);
    detail_right_col.set_hexpand(true);
    detail_right_col.set_valign(gtk::Align::Start);

    let detail_top_row = Box::new(Orientation::Horizontal, 16);
    detail_top_row.set_valign(gtk::Align::Start);

    let detail_title_stack = Box::new(Orientation::Vertical, 5);
    detail_title_stack.set_hexpand(true);

    let detail_name_lbl = Label::new(Some("Plugin Name"));
    detail_name_lbl.add_css_class("store-detail-title");
    detail_name_lbl.set_xalign(0.0);
    detail_name_lbl.set_wrap(true);

    let detail_tagline_lbl = Label::new(None);
    detail_tagline_lbl.add_css_class("store-detail-tagline");
    detail_tagline_lbl.set_xalign(0.0);
    detail_tagline_lbl.set_wrap(true);

    let detail_caps_row = Box::new(Orientation::Horizontal, 6);
    detail_caps_row.set_margin_top(2);

    detail_title_stack.append(&detail_name_lbl);
    detail_title_stack.append(&detail_tagline_lbl);
    detail_title_stack.append(&detail_caps_row);
    detail_top_row.append(&detail_title_stack);

    // Top-Right CTA Buttons + Rating Stack
    let detail_cta_col = Box::new(Orientation::Vertical, 8);
    detail_cta_col.set_halign(gtk::Align::End);
    detail_cta_col.set_valign(gtk::Align::Start);

    let detail_cta_row = Box::new(Orientation::Horizontal, 7);
    detail_cta_row.set_halign(gtk::Align::End);

    let detail_action_btn = Button::with_label("Install");
    detail_action_btn.add_css_class("btn-install");

    let detail_installed_pill = Label::new(Some("✓ Installed"));
    detail_installed_pill.add_css_class("store-installed-pill");
    detail_installed_pill.set_visible(false);

    let detail_settings_btn = Button::from_icon_name("preferences-system-symbolic");
    detail_settings_btn.add_css_class("btn-gear-detail");
    detail_settings_btn.set_tooltip_text(Some("Plugin Settings"));
    detail_settings_btn.set_visible(false);

    let detail_uninstall_btn = Button::with_label("Uninstall");
    detail_uninstall_btn.add_css_class("btn-uninstall-soft");
    detail_uninstall_btn.set_visible(false);

    detail_cta_row.append(&detail_action_btn);
    detail_cta_row.append(&detail_installed_pill);
    detail_cta_row.append(&detail_settings_btn);
    detail_cta_row.append(&detail_uninstall_btn);

    let detail_pbar = gtk::ProgressBar::new();
    detail_pbar.add_css_class("store-pbar");
    detail_pbar.set_size_request(140, 4);
    detail_pbar.set_halign(gtk::Align::End);
    detail_pbar.set_visible(false);

    let detail_rating_row = Box::new(Orientation::Horizontal, 5);
    detail_rating_row.set_halign(gtk::Align::End);
    detail_rating_row.set_visible(false);

    detail_cta_col.append(&detail_cta_row);
    detail_cta_col.append(&detail_pbar);
    detail_cta_col.append(&detail_rating_row);
    detail_top_row.append(&detail_cta_col);

    detail_right_col.append(&detail_top_row);

    // Description Card + Highlight Tags
    let desc_card = Box::new(Orientation::Vertical, 12);
    desc_card.add_css_class("store-detail-desc-card");
    let desc_lbl = Label::new(None);
    desc_lbl.set_wrap(true);
    desc_lbl.set_xalign(0.0);
    desc_lbl.set_selectable(true);
    let detail_tags_row = Box::new(Orientation::Horizontal, 7);
    desc_card.append(&desc_lbl);
    desc_card.append(&detail_tags_row);
    detail_right_col.append(&desc_card);

    detail_hero_box.append(&detail_right_col);
    detail_scroll_content.append(&detail_hero_box);

    // --- Screenshots Section (16:9 Gallery + Clickable Lightbox) ---
    let detail_shots_section = Box::new(Orientation::Vertical, 10);
    let shots_hdr = Box::new(Orientation::Horizontal, 8);
    let shots_title_wrap = Box::new(Orientation::Horizontal, 8);
    let shots_title = Label::new(Some("Screenshots"));
    shots_title.add_css_class("store-section-title");
    shots_title.set_xalign(0.0);
    let shots_count_pill = Label::new(Some("0"));
    shots_count_pill.add_css_class("store-count-pill");
    shots_count_pill.set_valign(gtk::Align::Center);
    shots_title_wrap.append(&shots_title);
    shots_title_wrap.append(&shots_count_pill);
    shots_title_wrap.set_hexpand(true);

    let shots_hint = Label::new(Some("Click any screenshot to inspect full size"));
    shots_hint.add_css_class("store-section-hint");
    shots_hint.set_valign(gtk::Align::Center);

    shots_hdr.append(&shots_title_wrap);
    shots_hdr.append(&shots_hint);
    detail_shots_section.append(&shots_hdr);

    let shots_box = Box::new(Orientation::Horizontal, 14);
    shots_box.set_hexpand(true);
    shots_box.set_vexpand(false);
    detail_shots_section.append(&shots_box);

    detail_scroll_content.append(&detail_shots_section);

    // --- Included Actions Section (2-Column Grid) ---
    let detail_actions_section = Box::new(Orientation::Vertical, 10);
    let actions_hdr = Box::new(Orientation::Horizontal, 8);
    let actions_title_wrap = Box::new(Orientation::Horizontal, 8);
    let actions_title = Label::new(Some("Included Actions"));
    actions_title.add_css_class("store-section-title");
    actions_title.set_xalign(0.0);
    let actions_count_pill = Label::new(Some("0"));
    actions_count_pill.add_css_class("store-count-pill");
    actions_count_pill.set_valign(gtk::Align::Center);
    actions_title_wrap.append(&actions_title);
    actions_title_wrap.append(&actions_count_pill);
    actions_title_wrap.set_hexpand(true);

    let actions_hint_lbl = Label::new(None);
    actions_hint_lbl.add_css_class("store-section-hint");
    actions_hint_lbl.set_valign(gtk::Align::Center);

    let actions_toggle_btn = Button::with_label("Show all");
    actions_toggle_btn.add_css_class("store-toggle-actions-btn");
    actions_toggle_btn.set_visible(false);

    actions_hdr.append(&actions_title_wrap);
    actions_hdr.append(&actions_hint_lbl);
    actions_hdr.append(&actions_toggle_btn);
    detail_actions_section.append(&actions_hdr);

    let detail_actions_grid = Grid::new();
    detail_actions_grid.set_column_spacing(10);
    detail_actions_grid.set_row_spacing(10);
    detail_actions_grid.set_column_homogeneous(true);
    detail_actions_grid.set_hexpand(true);
    detail_actions_section.append(&detail_actions_grid);

    detail_scroll_content.append(&detail_actions_section);

    detail_scrolled.set_child(Some(&detail_scroll_content));
    detail_page_box.append(&detail_scrolled);

    // =========================================================================
    // Page 3: "My Plugins" (Installed Plugins Management)
    // =========================================================================
    let installed_page_box = Box::new(Orientation::Vertical, 16);
    installed_page_box.set_margin_top(16);
    installed_page_box.set_margin_bottom(16);
    installed_page_box.set_margin_start(20);
    installed_page_box.set_margin_end(20);

    let installed_scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let installed_content = Box::new(Orientation::Vertical, 14);

    let installed_header_box = Box::new(Orientation::Vertical, 2);
    let inst_title = Label::new(Some("Installed Plugins"));
    inst_title.add_css_class("store-section-title");
    inst_title.set_xalign(0.0);

    let inst_sub = Label::new(Some("Manage active plugins, check for updates, or uninstall extensions"));
    inst_sub.add_css_class("store-section-subtitle");
    inst_sub.set_xalign(0.0);

    installed_header_box.append(&inst_title);
    installed_header_box.append(&inst_sub);
    installed_content.append(&installed_header_box);

    let installed_list_box = Box::new(Orientation::Vertical, 10);
    installed_content.append(&installed_list_box);

    installed_scrolled.set_child(Some(&installed_content));
    installed_page_box.append(&installed_scrolled);

    // Helper to refresh the Installed Plugins view
    let pm_inst_refresh = plugin_manager.clone();
    let on_inst_refresh_cb = on_plugin_installed.clone();
    let list_box_ref = installed_list_box.clone();

    // Helper to refresh header status ("My Plugins" vs "Updates Available" in amber)
    let my_plugins_btn_ref = my_plugins_btn.clone();
    let pm_hdr = plugin_manager.clone();
    let cat_hdr = plugins_catalog.clone();
    let refresh_header_status = Rc::new(move || {
        let pm_b = pm_hdr.borrow();
        let mut has_update = false;
        for c in &cat_hdr {
            if let Some(inst) = pm_b.plugins.get(&c.id) {
                if is_newer_version(&c.version, &inst.version) {
                    has_update = true;
                    break;
                }
            }
        }
        if has_update {
            my_plugins_btn_ref.set_label("Updates Available");
            my_plugins_btn_ref.remove_css_class("store-updates-btn");
            my_plugins_btn_ref.add_css_class("store-updates-amber-btn");
            my_plugins_btn_ref.set_tooltip_text(Some("Plugin updates are available. Click to review."));
        } else {
            my_plugins_btn_ref.set_label("My Plugins");
            my_plugins_btn_ref.remove_css_class("store-updates-amber-btn");
            my_plugins_btn_ref.add_css_class("store-updates-btn");
            my_plugins_btn_ref.set_tooltip_text(Some("Manage installed plugins"));
        }
    });

    // Run initial header check on open
    refresh_header_status();

    // Shared hook so installing/updating/uninstalling from ANY page syncs the Main Popular catalog cards
    let refresh_main_catalog_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let render_installed_list_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let render_installed_list = {
        let list_box_inner = list_box_ref.clone();
        let pm_inner = pm_inst_refresh.clone();
        let on_cb_inner = on_inst_refresh_cb.clone();
        let refresh_hdr = refresh_header_status.clone();
        let refresh_main_cat = refresh_main_catalog_cell.clone();
        let catalog_copy = plugins_catalog.clone();
        let self_cell = render_installed_list_cell.clone();
        let on_settings_inner = on_open_plugin_settings.clone();

        Rc::new(move || {
            while let Some(child) = list_box_inner.first_child() {
                list_box_inner.remove(&child);
            }

            let pm = pm_inner.borrow();
            if pm.plugins.is_empty() {
                let empty_lbl = Label::new(Some("No custom plugins installed yet. Explore the store to install plugins."));
                empty_lbl.add_css_class("dim-label");
                empty_lbl.set_margin_top(24);
                empty_lbl.set_xalign(0.5);
                list_box_inner.append(&empty_lbl);
            } else {
                for (p_id, p_obj) in pm.plugins.iter() {
                    let card = Box::new(Orientation::Vertical, 6);
                    card.add_css_class("card");
                    card.add_css_class("store-card");

                    let escaped_name = glib::markup_escape_text(&p_obj.name);
                    let escaped_desc = glib::markup_escape_text(&p_obj.description);
                    let row = ActionRow::builder()
                        .title(escaped_name.as_str())
                        .subtitle(escaped_desc.as_str())
                        .build();

                    // Resolve best icon from catalog cache or plugin assets directory
                    let cat_match = catalog_copy.iter().find(|c| c.id == *p_id);
                    let resolved_icon_file = cat_match
                        .filter(|c| c.is_file_icon && std::path::Path::new(&c.icon).exists())
                        .map(|c| std::path::PathBuf::from(&c.icon))
                        .or_else(|| {
                            for cand in ["icon.png", "action_launch.png", "govee.png", "store.png"] {
                                let p = p_obj.path.join("assets").join(cand);
                                if p.exists() {
                                    return Some(p);
                                }
                            }
                            None
                        });
                    if let Some(icon_path) = resolved_icon_file {
                        let img = Image::from_file(&icon_path);
                        img.set_pixel_size(36);
                        row.add_prefix(&img);
                    } else {
                        let img = Image::from_icon_name("system-run-symbolic");
                        img.set_pixel_size(36);
                        row.add_prefix(&img);
                    }

                    let btn_box = Box::new(Orientation::Horizontal, 8);
                    btn_box.set_valign(gtk::Align::Center);
                    btn_box.set_halign(gtk::Align::End);

                    // Check if catalog offers an update (downloads fresh remote archive from download_url)
                    if let Some(cat_item) = cat_match {
                        if is_newer_version(&cat_item.version, &p_obj.version) {
                            let update_btn = Button::with_label(&format!("Update to {}", format_version_tag(&cat_item.version)));
                            update_btn.add_css_class("store-updates-amber-btn");
                            update_btn.set_valign(gtk::Align::Center);
                            update_btn.set_halign(gtk::Align::End);

                            let btn_up_clone = update_btn.clone();
                            let pm_up = pm_inner.clone();
                            let on_up_cb = on_cb_inner.clone();
                            let refresh_hdr_click = refresh_hdr.clone();
                            let refresh_cat_click = refresh_main_cat.clone();
                            let self_cell_clone = self_cell.clone();
                            let d_url_up = cat_item.download_url.clone().unwrap_or_default();
                            let pid_up = p_id.clone();

                            update_btn.connect_clicked(move |_| {
                                btn_up_clone.set_label("Updating...");
                                btn_up_clone.set_sensitive(false);

                                let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
                                let d_url = d_url_up.clone();
                                let pid_dl = pid_up.clone();
                                let p_dir = pm_up.borrow().plugins_dir.clone();

                                std::thread::spawn(move || {
                                    let res = PluginManager::download_and_extract_plugin(&p_dir, &pid_dl, &d_url);
                                    let _ = tx.send_blocking(res);
                                });

                                let btn_done = btn_up_clone.clone();
                                let pm_done = pm_up.clone();
                                let on_done = on_up_cb.clone();
                                let ref_hdr = refresh_hdr_click.clone();
                                let ref_cat = refresh_cat_click.clone();
                                let self_done = self_cell_clone.clone();

                                glib::spawn_future_local(async move {
                                    if let Ok(res) = rx.recv().await {
                                        match res {
                                            Ok(()) => {
                                                pm_done.borrow_mut().rescan_plugins();
                                                on_done();
                                                ref_hdr();
                                                if let Some(ref rc) = *ref_cat.borrow() {
                                                    rc();
                                                }
                                                if let Some(ref r) = *self_done.borrow() {
                                                    r();
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!("Failed to update plugin: {e}");
                                                btn_done.set_label("Retry Update");
                                                btn_done.set_sensitive(true);
                                            }
                                        }
                                    }
                                });
                            });
                            btn_box.append(&update_btn);
                        }
                    }

                    let settings_btn = Button::from_icon_name("preferences-system-symbolic");
                    settings_btn.add_css_class("flat");
                    settings_btn.set_tooltip_text(Some(&format!("{} Settings", p_obj.name)));
                    settings_btn.set_valign(gtk::Align::Center);
                    let pid_set = p_id.clone();
                    let pname_set = p_obj.name.clone();
                    let on_set_cb = on_settings_inner.clone();
                    settings_btn.connect_clicked(move |_| {
                        on_set_cb(pid_set.clone(), pname_set.clone());
                    });
                    btn_box.append(&settings_btn);

                    let uninst_btn = Button::with_label("Uninstall");
                    uninst_btn.add_css_class("btn-uninstall");
                    uninst_btn.set_valign(gtk::Align::Center);
                    uninst_btn.set_halign(gtk::Align::End);

                    let pid_clone = p_id.clone();
                    let pm_uninst = pm_inner.clone();
                    let on_uninst_cb = on_cb_inner.clone();
                    let refresh_hdr_uninst = refresh_hdr.clone();
                    let refresh_cat_uninst = refresh_main_cat.clone();
                    let self_uninst = self_cell.clone();

                    uninst_btn.connect_clicked(move |btn| {
                        btn.set_sensitive(false);
                        let mut pm_mut = pm_uninst.borrow_mut();
                        let _ = pm_mut.uninstall_plugin(&pid_clone);
                        drop(pm_mut);

                        // Defer widget removal until after GTK finishes handling this click event
                        // (prevents Gtk-CRITICAL: gtk_list_box_row_grab_focus on destroyed row)
                        let on_cb = on_uninst_cb.clone();
                        let ref_hdr = refresh_hdr_uninst.clone();
                        let ref_cat = refresh_cat_uninst.clone();
                        let self_ref = self_uninst.clone();
                        glib::idle_add_local_once(move || {
                            on_cb();
                            ref_hdr();
                            if let Some(ref rc) = *ref_cat.borrow() {
                                rc();
                            }
                            if let Some(ref r) = *self_ref.borrow() {
                                r();
                            }
                        });
                    });

                    btn_box.append(&uninst_btn);
                    row.add_suffix(&btn_box);
                    card.append(&row);

                    let author_line = format!("by {} • {} • ID: {}", p_obj.author, p_obj.version, p_id);
                    let meta = Label::new(Some(&author_line));
                    meta.add_css_class("dim-label");
                    meta.set_xalign(0.0);
                    meta.set_margin_start(16);
                    meta.set_margin_bottom(10);
                    card.append(&meta);

                    list_box_inner.append(&card);
                }
            }
        })
    };
    *render_installed_list_cell.borrow_mut() = Some(render_installed_list.clone());

    // =========================================================================
    // Connect Navigation & Transitions
    // =========================================================================
    let stack_ref = stack.clone();
    let back_btn_ref = back_btn.clone();
    let install_local_btn_ref = install_local_btn.clone();
    let window_title_ref = window.clone();
    let pm_detail = plugin_manager.clone();
    let on_installed_detail = on_plugin_installed.clone();

    // Single persistent state + handlers for Detail Page (prevents stacked duplicate downloads!)
    let current_detail_plugin: Rc<RefCell<Option<PluginInfo>>> = Rc::new(RefCell::new(None));
    let detail_actions_expanded: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let detail_shots_gen: Rc<std::cell::Cell<u64>> = Rc::new(std::cell::Cell::new(0));
    let refresh_detail_cta_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let render_detail_actions_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    // Source & Issues link click handler
    {
        let cur_plugin_src = current_detail_plugin.clone();
        let win_src = window.clone();
        detail_source_btn.connect_clicked(move |_| {
            if let Some(ref plugin) = *cur_plugin_src.borrow() {
                if let Some(ref url) = plugin.github_url {
                    gtk::UriLauncher::new(url).launch(Some(&win_src), gio::Cancellable::NONE, |_| {});
                }
            }
        });
    }

    // Plugin Settings gear button on Detail Page
    {
        let cur_plugin_set = current_detail_plugin.clone();
        let on_settings_detail = on_open_plugin_settings.clone();
        detail_settings_btn.connect_clicked(move |_| {
            if let Some(ref plugin) = *cur_plugin_set.borrow() {
                on_settings_detail(plugin.id.clone(), plugin.name.clone());
            }
        });
    }

    // CTA state updater (Install / Update vX.Y.Z / Installed + Settings + Uninstall + Version arrow)
    let refresh_detail_cta: Rc<dyn Fn()> = {
        let cur_plugin = current_detail_plugin.clone();
        let pm_cta = pm_detail.clone();
        let action_btn = detail_action_btn.clone();
        let installed_pill = detail_installed_pill.clone();
        let settings_btn = detail_settings_btn.clone();
        let uninstall_btn = detail_uninstall_btn.clone();
        let pbar = detail_pbar.clone();
        let ver_lbl = detail_version_lbl.clone();
        let ver_arrow = detail_version_arrow.clone();

        Rc::new(move || {
            let Some(plugin) = cur_plugin.borrow().clone() else {
                return;
            };
            let pm_b = pm_cta.borrow();
            let installed_obj = pm_b.plugins.get(&plugin.id);
            let is_installed = installed_obj.is_some();
            let has_update = installed_obj
                .map(|inst| is_newer_version(&plugin.version, &inst.version))
                .unwrap_or(false);

            if let Some(inst) = installed_obj {
                ver_lbl.set_text(&format_version_tag(&inst.version));
                if has_update {
                    ver_arrow.set_text(&format!("→ {}", format_version_tag(&plugin.version)));
                    ver_arrow.set_visible(true);
                } else {
                    ver_arrow.set_visible(false);
                }
            } else {
                ver_lbl.set_text(&format_version_tag(&plugin.version));
                ver_arrow.set_visible(false);
            }

            let has_settings = pm_b
                .plugin_has_settings
                .lock()
                .ok()
                .and_then(|m| m.get(&plugin.id).copied())
                .unwrap_or(true);
            drop(pm_b);

            action_btn.remove_css_class("btn-install");
            action_btn.remove_css_class("btn-downloading");
            action_btn.remove_css_class("btn-installed");
            action_btn.remove_css_class("store-updates-amber-btn");
            pbar.set_visible(false);

            if is_installed && has_update {
                action_btn.set_label(&format!("Update {}", format_version_tag(&plugin.version)));
                action_btn.add_css_class("store-updates-amber-btn");
                action_btn.set_sensitive(true);
                action_btn.set_visible(true);
                installed_pill.set_visible(false);
                settings_btn.set_visible(has_settings);
                uninstall_btn.set_visible(true);
                uninstall_btn.set_sensitive(true);
            } else if is_installed {
                action_btn.set_visible(false);
                installed_pill.set_visible(true);
                settings_btn.set_visible(has_settings);
                uninstall_btn.set_visible(true);
                uninstall_btn.set_sensitive(true);
            } else {
                action_btn.set_label("Install");
                action_btn.add_css_class("btn-install");
                action_btn.set_sensitive(true);
                action_btn.set_visible(true);
                installed_pill.set_visible(false);
                settings_btn.set_visible(false);
                uninstall_btn.set_visible(false);
            }
        })
    };
    *refresh_detail_cta_cell.borrow_mut() = Some(refresh_detail_cta.clone());

    // Render Included Actions 2-column grid (catalog metadata with local fallback)
    let render_detail_actions: Rc<dyn Fn()> = {
        let cur_plugin = current_detail_plugin.clone();
        let expanded_ref = detail_actions_expanded.clone();
        let pm_act = pm_detail.clone();
        let section = detail_actions_section.clone();
        let count_pill = actions_count_pill.clone();
        let hint_lbl = actions_hint_lbl.clone();
        let toggle_btn = actions_toggle_btn.clone();
        let grid = detail_actions_grid.clone();

        Rc::new(move || {
            while let Some(child) = grid.first_child() {
                grid.remove(&child);
            }
            let Some(plugin) = cur_plugin.borrow().clone() else {
                section.set_visible(false);
                return;
            };

            let mut previews = plugin.actions.clone();
            if previews.is_empty() {
                if let Some(inst) = pm_act.borrow().plugins.get(&plugin.id) {
                    for a in &inst.actions {
                        previews.push(PluginActionPreview {
                            name: a.name.clone(),
                            description: format!("{} action", plugin.name),
                            surfaces: vec!["keys".to_string(), "dials".to_string()],
                        });
                    }
                }
            }

            if previews.is_empty() {
                section.set_visible(false);
                return;
            }
            section.set_visible(true);

            let total = previews.len();
            let is_expanded = *expanded_ref.borrow();
            let visible_limit = if is_expanded || total <= 4 { total } else { 4 };
            count_pill.set_text(&total.to_string());

            if total > 4 {
                if is_expanded {
                    hint_lbl.set_text(&format!("Showing all {total} actions"));
                    toggle_btn.set_label("Show less");
                } else {
                    hint_lbl.set_text(&format!("Showing 4 of {total}"));
                    toggle_btn.set_label(&format!("Show all {total}"));
                }
                toggle_btn.set_visible(true);
            } else {
                let runtime_hint = match plugin.runtime {
                    PluginRuntime::Ballista => "Native Ballista surface-aware actions",
                    PluginRuntime::StreamController => "StreamController compatibility actions",
                };
                hint_lbl.set_text(runtime_hint);
                toggle_btn.set_visible(false);
            }

            let installed_plugin_dir = pm_act
                .borrow()
                .plugins
                .get(&plugin.id)
                .map(|p| p.path.clone());
            let installed_actions_map: std::collections::HashMap<String, String> = pm_act
                .borrow()
                .plugins
                .get(&plugin.id)
                .map(|p| {
                    p.actions
                        .iter()
                        .map(|a| (a.name.to_lowercase(), a.icon_name_or_file.clone()))
                        .collect()
                })
                .unwrap_or_default();

            for (idx, act) in previews.iter().take(visible_limit).enumerate() {
                let card = Box::new(Orientation::Horizontal, 11);
                card.add_css_class("store-action-preview-card");

                let ico_box = Box::new(Orientation::Horizontal, 0);
                ico_box.add_css_class("store-action-ico-box");
                ico_box.set_valign(gtk::Align::Center);
                ico_box.set_size_request(34, 34);

                let local_icon = installed_actions_map
                    .get(&act.name.to_lowercase())
                    .filter(|p| std::path::Path::new(p).exists())
                    .cloned()
                    .or_else(|| {
                        installed_plugin_dir.as_ref().and_then(|dir| {
                            let slug = act.name.to_lowercase().replace(' ', "_");
                            for cand in [
                                format!("{slug}.png"),
                                format!("action-{}.png", act.name.to_lowercase().replace(' ', "-")),
                            ] {
                                let p = dir.join("assets").join(cand);
                                if p.exists() {
                                    return Some(p.to_string_lossy().to_string());
                                }
                            }
                            None
                        })
                    });

                let ico = if let Some(path) = local_icon {
                    let img = Image::from_file(path);
                    img.set_pixel_size(22);
                    img
                } else {
                    let sym = match plugin.category.as_str() {
                        "Streaming" => "audio-input-microphone-symbolic",
                        "Audio" => "audio-volume-high-symbolic",
                        "System" => "preferences-system-symbolic",
                        "Developer" => "utilities-terminal-symbolic",
                        "Productivity" => "x-office-document-symbolic",
                        _ => "applications-system-symbolic",
                    };
                    let img = Image::from_icon_name(sym);
                    img.set_pixel_size(18);
                    img
                };
                ico.set_halign(gtk::Align::Center);
                ico.set_valign(gtk::Align::Center);
                ico.set_hexpand(true);
                ico.set_vexpand(true);
                ico_box.append(&ico);
                card.append(&ico_box);

                let body = Box::new(Orientation::Vertical, 2);
                body.set_hexpand(true);
                body.set_valign(gtk::Align::Center);

                let top = Box::new(Orientation::Horizontal, 8);
                let name_l = Label::new(Some(&act.name));
                name_l.add_css_class("store-action-name");
                name_l.set_xalign(0.0);
                name_l.set_hexpand(true);
                name_l.set_ellipsize(pango::EllipsizeMode::End);
                top.append(&name_l);

                let has_key = act.surfaces.iter().any(|s| s == "keys" || s == "key");
                let has_dial = act.surfaces.iter().any(|s| s == "dials" || s == "dial");
                let has_touch = act.surfaces.iter().any(|s| s == "touchscreen" || s == "touch");
                let (badge_text, badge_css) = match (has_key, has_dial, has_touch) {
                    (true, true, _) => ("KEY · DIAL", "cap-badge-both"),
                    (false, true, true) => ("DIAL · TOUCH", "cap-badge-dial"),
                    (false, true, false) => ("DIAL", "cap-badge-dial"),
                    (false, false, true) => ("TOUCH", "cap-badge-both"),
                    _ => ("KEY", "cap-badge-key"),
                };
                let cap_lbl = Label::new(Some(badge_text));
                cap_lbl.add_css_class("cap-badge");
                cap_lbl.add_css_class(badge_css);
                cap_lbl.set_valign(gtk::Align::Center);
                top.append(&cap_lbl);
                body.append(&top);

                if !act.description.is_empty() {
                    let desc_l = Label::new(Some(&act.description));
                    desc_l.add_css_class("store-action-desc");
                    desc_l.set_xalign(0.0);
                    desc_l.set_ellipsize(pango::EllipsizeMode::End);
                    body.append(&desc_l);
                }

                card.append(&body);

                let col = (idx % 2) as i32;
                let row = (idx / 2) as i32;
                grid.attach(&card, col, row, 1, 1);
            }
        })
    };
    *render_detail_actions_cell.borrow_mut() = Some(render_detail_actions.clone());

    // Toggle expanded Included Actions list
    {
        let expanded_click = detail_actions_expanded.clone();
        let render_act = render_detail_actions.clone();
        actions_toggle_btn.connect_clicked(move |_| {
            let cur = *expanded_click.borrow();
            *expanded_click.borrow_mut() = !cur;
            render_act();
        });
    }

    // Install / Update button click handler on Detail Page
    {
        let cur_plugin_click = current_detail_plugin.clone();
        let btn_clone = detail_action_btn.clone();
        let pbar_clone = detail_pbar.clone();
        let pm_action = pm_detail.clone();
        let on_inst_action = on_installed_detail.clone();
        let ref_hdr_action = refresh_header_status.clone();
        let ref_cat_action = refresh_main_catalog_cell.clone();
        let ref_cta_action = refresh_detail_cta_cell.clone();
        let ref_act_action = render_detail_actions_cell.clone();

        detail_action_btn.connect_clicked(move |_| {
            let Some(plugin) = cur_plugin_click.borrow().clone() else {
                return;
            };
            let is_update = pm_action.borrow().plugins.contains_key(&plugin.id);
            btn_clone.set_label(if is_update { "Updating..." } else { "Downloading..." });
            btn_clone.remove_css_class("btn-install");
            btn_clone.remove_css_class("store-updates-amber-btn");
            btn_clone.add_css_class("btn-downloading");
            btn_clone.set_sensitive(false);
            pbar_clone.set_visible(true);
            pbar_clone.pulse();

            let pbar_pulse = pbar_clone.clone();
            let pulse_source = glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
                pbar_pulse.pulse();
                glib::ControlFlow::Continue
            });
            let pulse_cell = std::rc::Rc::new(std::cell::Cell::new(Some(pulse_source)));

            let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
            let d_url = plugin.download_url.clone().unwrap_or_default();
            let p_id = plugin.id.clone();
            let p_dir = pm_action.borrow().plugins_dir.clone();

            std::thread::spawn(move || {
                let res = PluginManager::download_and_extract_plugin(&p_dir, &p_id, &d_url);
                let _ = tx.send_blocking(res);
            });

            let pbar_done = pbar_clone.clone();
            let on_inst_done = on_inst_action.clone();
            let ref_hdr_done = ref_hdr_action.clone();
            let ref_cat_done = ref_cat_action.clone();
            let ref_cta_done = ref_cta_action.clone();
            let ref_act_done = ref_act_action.clone();
            let pm_done = pm_action.clone();
            let pulse_done = pulse_cell.clone();

            glib::spawn_future_local(async move {
                if let Ok(res) = rx.recv().await {
                    if let Some(src) = pulse_done.take() {
                        src.remove();
                    }
                    pbar_done.set_visible(false);
                    match res {
                        Ok(()) => {
                            pm_done.borrow_mut().rescan_plugins();
                            on_inst_done();
                            ref_hdr_done();
                            if let Some(ref rc) = *ref_cat_done.borrow() {
                                rc();
                            }
                            if let Some(ref cta) = *ref_cta_done.borrow() {
                                cta();
                            }
                            if let Some(ref act) = *ref_act_done.borrow() {
                                act();
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to install/update plugin: {e}");
                            if let Some(ref cta) = *ref_cta_done.borrow() {
                                cta();
                            }
                        }
                    }
                }
            });
        });
    }

    // Uninstall button click handler on Detail Page
    {
        let cur_plugin_uninst = current_detail_plugin.clone();
        let pm_uninst = pm_detail.clone();
        let on_inst_uninst = on_installed_detail.clone();
        let ref_hdr_uninst = refresh_header_status.clone();
        let ref_cat_uninst = refresh_main_catalog_cell.clone();
        let ref_cta_uninst = refresh_detail_cta_cell.clone();
        let ref_act_uninst = render_detail_actions_cell.clone();

        detail_uninstall_btn.connect_clicked(move |btn| {
            let Some(plugin) = cur_plugin_uninst.borrow().clone() else {
                return;
            };
            btn.set_sensitive(false);
            let _ = pm_uninst.borrow_mut().uninstall_plugin(&plugin.id);

            let on_cb = on_inst_uninst.clone();
            let ref_hdr = ref_hdr_uninst.clone();
            let ref_cat = ref_cat_uninst.clone();
            let ref_cta = ref_cta_uninst.clone();
            let ref_act = ref_act_uninst.clone();
            glib::idle_add_local_once(move || {
                on_cb();
                ref_hdr();
                if let Some(ref rc) = *ref_cat.borrow() {
                    rc();
                }
                if let Some(ref cta) = *ref_cta.borrow() {
                    cta();
                }
                if let Some(ref act) = *ref_act.borrow() {
                    act();
                }
            });
        });
    }

    let open_plugin_detail = Rc::new({
        let stack_inner = stack_ref.clone();
        let back_btn_inner = back_btn_ref.clone();
        let install_local_inner = install_local_btn_ref.clone();
        let win_title_inner = window_title_ref.clone();
        let name_lbl = detail_name_lbl.clone();
        let tagline_lbl = detail_tagline_lbl.clone();
        let caps_row = detail_caps_row.clone();
        let desc_lbl = desc_lbl.clone();
        let tags_row = detail_tags_row.clone();
        let icon_pic = detail_icon_pic.clone();
        let icon_sym = detail_icon_sym.clone();
        let runtime_badge = detail_runtime_badge.clone();
        let runtime_ico = detail_runtime_ico.clone();
        let runtime_lbl = detail_runtime_lbl.clone();
        let runtime_sub = detail_runtime_sub.clone();
        let author_lbl = detail_author_lbl.clone();
        let license_row = meta_license_row.clone();
        let license_lbl = detail_license_lbl.clone();
        let source_btn = detail_source_btn.clone();
        let rating_row = detail_rating_row.clone();
        let shots_section = detail_shots_section.clone();
        let shots_count = shots_count_pill.clone();
        let shots_container = shots_box.clone();
        let shots_gen = detail_shots_gen.clone();
        let cur_plugin_set = current_detail_plugin.clone();
        let expanded_reset = detail_actions_expanded.clone();
        let ref_cta = refresh_detail_cta.clone();
        let ref_actions = render_detail_actions.clone();
        let detail_vscroll = detail_scrolled.clone();

        move |plugin: PluginInfo| {
            *cur_plugin_set.borrow_mut() = Some(plugin.clone());
            *expanded_reset.borrow_mut() = false;
            let gen_id = shots_gen.get().wrapping_add(1);
            shots_gen.set(gen_id);

            win_title_inner.set_title(Some(&plugin.name));
            name_lbl.set_text(&plugin.name);

            if !plugin.short_description.is_empty() {
                tagline_lbl.set_text(&plugin.short_description);
                tagline_lbl.set_visible(true);
            } else {
                tagline_lbl.set_visible(false);
            }
            desc_lbl.set_text(&plugin.description);

            // Runtime Badge (Ballista Native vs StreamController)
            runtime_badge.remove_css_class("store-runtime-ballista");
            runtime_badge.remove_css_class("store-runtime-sc");
            match plugin.runtime {
                PluginRuntime::Ballista => {
                    runtime_badge.add_css_class("store-runtime-ballista");
                    runtime_ico.set_icon_name(Some("starred-symbolic"));
                    runtime_lbl.set_text("Ballista Native");
                    runtime_sub.set_text("Native Rust · Zero Overhead");
                    runtime_badge.set_tooltip_text(Some(
                        "Native Ballista plugin running directly in Rust & Cairo with zero bridge overhead",
                    ));
                }
                PluginRuntime::StreamController => {
                    runtime_badge.add_css_class("store-runtime-sc");
                    runtime_ico.set_icon_name(Some("insert-link-symbolic"));
                    runtime_lbl.set_text("StreamController");
                    runtime_sub.set_text("Runs via Built-in Bridge");
                    runtime_badge.set_tooltip_text(Some(
                        "StreamController plugin running seamlessly via Ballista's built-in compatibility bridge",
                    ));
                }
            }

            // Left-Column Metadata
            author_lbl.set_text(&plugin.author);
            if !plugin.license.trim().is_empty() {
                license_lbl.set_text(&format!("{} License", plugin.license.trim()));
                license_row.set_visible(true);
            } else {
                license_row.set_visible(false);
            }
            source_btn.set_visible(plugin.github_url.is_some());

            // Hardware Surface Pills + Compatible Devices
            while let Some(c) = caps_row.first_child() {
                caps_row.remove(&c);
            }
            for surf in &plugin.surfaces {
                let (txt, css) = match surf.as_str() {
                    "keys" | "key" => ("KEY", "cap-badge-key"),
                    "dials" | "dial" => ("DIAL", "cap-badge-dial"),
                    "touchscreen" | "touch" => ("TOUCHSCREEN", "cap-badge-both"),
                    _ => continue,
                };
                let pill = Label::new(Some(txt));
                pill.add_css_class("cap-badge");
                pill.add_css_class(css);
                caps_row.append(&pill);
            }
            let dev_note = format_device_summary(&plugin.compatible_devices);
            if !dev_note.is_empty() {
                let dev_lbl = Label::new(Some(&dev_note));
                dev_lbl.add_css_class("store-device-note");
                dev_lbl.set_ellipsize(pango::EllipsizeMode::End);
                caps_row.append(&dev_lbl);
            }
            caps_row.set_visible(caps_row.first_child().is_some());

            // Description Highlight Tags
            while let Some(c) = tags_row.first_child() {
                tags_row.remove(&c);
            }
            for tag in plugin.tags.iter().take(4) {
                let t = Label::new(Some(tag));
                t.add_css_class("store-desc-tag");
                tags_row.append(&t);
            }
            let cat_tag = Label::new(Some(&format!("Category: {}", plugin.category)));
            cat_tag.add_css_class("store-desc-tag");
            tags_row.append(&cat_tag);

            // Community Rating Row (cleanly hidden when unrated)
            while let Some(c) = rating_row.first_child() {
                rating_row.remove(&c);
            }
            if let Some(score) = plugin.rating {
                if plugin.rating_count > 0 {
                    let filled = score.round().clamp(0.0, 5.0) as usize;
                    let stars_box = Box::new(Orientation::Horizontal, 1);
                    for i in 0..5 {
                        let s = Label::new(Some(if i < filled { "★" } else { "☆" }));
                        s.add_css_class(if i < filled { "store-star-filled" } else { "store-star-empty" });
                        stars_box.append(&s);
                    }
                    let score_lbl = Label::new(Some(&format!("{score:.1}")));
                    score_lbl.add_css_class("store-rating-score");
                    let cnt_lbl = Label::new(Some(&format!("({})", plugin.rating_count)));
                    cnt_lbl.add_css_class("store-rating-count");
                    rating_row.append(&stars_box);
                    rating_row.append(&score_lbl);
                    rating_row.append(&cnt_lbl);
                    rating_row.set_visible(true);
                } else {
                    rating_row.set_visible(false);
                }
            } else {
                rating_row.set_visible(false);
            }

            // Load Hero Icon
            let resolved_icon = if plugin.is_file_icon {
                let direct = PathBuf::from(&plugin.icon);
                let local_asset = glib::user_data_dir()
                    .join("ballista")
                    .join("plugins")
                    .join(&plugin.id)
                    .join("assets")
                    .join(&plugin.icon);
                if direct.exists() {
                    Some(direct)
                } else if local_asset.exists() {
                    Some(local_asset)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(icon_path) = resolved_icon {
                let file = gtk::gio::File::for_path(&icon_path);
                icon_pic.set_file(Some(&file));
                icon_pic.set_visible(true);
                icon_sym.set_visible(false);
            } else {
                icon_pic.set_visible(false);
                let sym = if plugin.is_file_icon { "system-run-symbolic" } else { &plugin.icon };
                icon_sym.set_icon_name(Some(sym));
                icon_sym.set_visible(true);
            }

            // Build Screenshots Gallery (16:9 tiles, lazy async fetch, Lightbox click, zero fake placeholders)
            while let Some(c) = shots_container.first_child() {
                shots_container.remove(&c);
            }

            let mut shot_sources: Vec<(String, String, bool)> = Vec::new();
            for (idx, s_url) in plugin.screenshots.iter().enumerate() {
                shot_sources.push((s_url.clone(), format!("Screenshot {}", idx + 1), false));
            }
            if shot_sources.is_empty() {
                if let Some(ref thumb) = plugin.thumbnail {
                    if std::path::Path::new(thumb).exists() {
                        shot_sources.push((thumb.clone(), "Overview Banner".to_string(), true));
                    }
                }
            }

            if shot_sources.is_empty() {
                shots_section.set_visible(false);
            } else {
                shots_section.set_visible(true);
                shots_count.set_text(&shot_sources.len().to_string());
                shots_container.set_homogeneous(shot_sources.len() >= 2 && shot_sources.len() <= 3);

                let resolved_paths: Rc<RefCell<Vec<Option<PathBuf>>>> =
                    Rc::new(RefCell::new(vec![None; shot_sources.len()]));
                let captions_list: Rc<Vec<String>> =
                    Rc::new(shot_sources.iter().map(|(_, cap, _)| cap.clone()).collect());

                for (idx, (src, caption, is_banner)) in shot_sources.into_iter().enumerate() {
                    let tile = gtk::Overlay::new();
                    tile.add_css_class("store-screenshot-tile");
                    tile.set_overflow(gtk::Overflow::Hidden);
                    tile.set_size_request(282, 158);
                    tile.set_hexpand(true);
                    tile.set_vexpand(false);
                    tile.set_cursor_from_name(Some("pointer"));

                    let pic = Picture::new();
                    pic.set_can_shrink(true);
                    pic.set_content_fit(if is_banner {
                        gtk::ContentFit::Contain
                    } else {
                        gtk::ContentFit::Cover
                    });
                    pic.set_size_request(282, 158);
                    pic.set_hexpand(true);
                    pic.set_vexpand(true);
                    tile.set_child(Some(&pic));

                    let cap_lbl = Label::new(Some(&caption));
                    cap_lbl.add_css_class("store-shot-caption");
                    cap_lbl.set_halign(gtk::Align::Start);
                    cap_lbl.set_valign(gtk::Align::End);
                    tile.add_overlay(&cap_lbl);

                    let zoom_box = Box::new(Orientation::Horizontal, 0);
                    zoom_box.add_css_class("store-shot-zoom-badge");
                    zoom_box.set_halign(gtk::Align::End);
                    zoom_box.set_valign(gtk::Align::End);
                    let zoom_ico = Image::from_icon_name("view-fullscreen-symbolic");
                    zoom_ico.set_pixel_size(12);
                    zoom_ico.set_halign(gtk::Align::Center);
                    zoom_ico.set_valign(gtk::Align::Center);
                    zoom_ico.set_hexpand(true);
                    zoom_ico.set_vexpand(true);
                    zoom_box.append(&zoom_ico);
                    tile.add_overlay(&zoom_box);

                    // Click opens full-size Lightbox viewer
                    let click = GestureClick::new();
                    let win_lb = win_title_inner.clone();
                    let paths_lb = resolved_paths.clone();
                    let caps_lb = captions_list.clone();
                    let pname_lb = plugin.name.clone();
                    click.connect_pressed(move |_, _, _, _| {
                        let ready: Vec<(PathBuf, String)> = paths_lb
                            .borrow()
                            .iter()
                            .enumerate()
                            .filter_map(|(i, p)| p.clone().map(|pb| (pb, caps_lb[i].clone())))
                            .collect();
                        if !ready.is_empty() {
                            let start_idx = idx.min(ready.len().saturating_sub(1));
                            show_screenshot_lightbox(&win_lb, &pname_lb, ready, start_idx);
                        }
                    });
                    tile.add_controller(click);

                    if src.starts_with("http://") || src.starts_with("https://") {
                        let cache_path = market_plugin_cache_dir(&plugin.id).join(format!("shot_{idx}.png"));
                        if cache_path.exists() {
                            let file = gtk::gio::File::for_path(&cache_path);
                            pic.set_file(Some(&file));
                            resolved_paths.borrow_mut()[idx] = Some(cache_path);
                        } else {
                            let (tx, rx) = async_channel::bounded::<Option<PathBuf>>(1);
                            let dl_url = src.clone();
                            let dl_dest = cache_path.clone();
                            std::thread::spawn(move || {
                                let ok = std::process::Command::new("curl")
                                    .arg("-fsSL")
                                    .arg("-m")
                                    .arg("8")
                                    .arg("-o")
                                    .arg(&dl_dest)
                                    .arg(&dl_url)
                                    .status()
                                    .map(|s| s.success())
                                    .unwrap_or(false);
                                let _ = tx.send_blocking(if ok && dl_dest.exists() {
                                    Some(dl_dest)
                                } else {
                                    None
                                });
                            });
                            let pic_async = pic.clone();
                            let paths_async = resolved_paths.clone();
                            let gen_check = shots_gen.clone();
                            glib::spawn_future_local(async move {
                                if let Ok(Some(path)) = rx.recv().await {
                                    if gen_check.get() == gen_id {
                                        let file = gtk::gio::File::for_path(&path);
                                        pic_async.set_file(Some(&file));
                                        paths_async.borrow_mut()[idx] = Some(path);
                                    }
                                }
                            });
                        }
                    } else {
                        let local_p = PathBuf::from(&src);
                        if local_p.exists() {
                            let file = gtk::gio::File::for_path(&local_p);
                            pic.set_file(Some(&file));
                            resolved_paths.borrow_mut()[idx] = Some(local_p);
                        }
                    }

                    shots_container.append(&tile);
                }
            }

            // Refresh CTA Buttons, Version status & Included Actions Grid
            ref_cta();
            ref_actions();

            // Reset scroll to top on open
            detail_vscroll.vadjustment().set_value(0.0);

            install_local_inner.set_visible(false);
            back_btn_inner.set_visible(true);
            stack_inner.set_visible_child_name("detail");
        }
    });

    // =========================================================================
    // Page 4: Dedicated Category View (Matching User's Mockup)
    // =========================================================================
    let category_page_box = Box::new(Orientation::Vertical, 16);
    category_page_box.set_margin_top(16);
    category_page_box.set_margin_bottom(16);
    category_page_box.set_margin_start(20);
    category_page_box.set_margin_end(20);

    let cat_scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let cat_content_box = Box::new(Orientation::Vertical, 16);

    let cat_header_box = Box::new(Orientation::Vertical, 4);
    let cat_page_title = Label::new(Some("Category Plugins"));
    cat_page_title.add_css_class("title-1");
    cat_page_title.set_xalign(0.0);

    let cat_page_sub = Label::new(Some("Browse and install extensions in this category"));
    cat_page_sub.add_css_class("dim-label");
    cat_page_sub.set_xalign(0.0);

    cat_header_box.append(&cat_page_title);
    cat_header_box.append(&cat_page_sub);
    cat_content_box.append(&cat_header_box);

    let cat_grid_cards = Grid::new();
    cat_grid_cards.set_column_spacing(12);
    cat_grid_cards.set_row_spacing(12);
    cat_grid_cards.set_column_homogeneous(true);
    cat_grid_cards.set_hexpand(true);
    cat_content_box.append(&cat_grid_cards);

    cat_scrolled.set_child(Some(&cat_content_box));
    category_page_box.append(&cat_scrolled);

    let open_category_page = {
        let stack_cat = stack.clone();
        let back_btn_cat = back_btn.clone();
        let install_local_cat = install_local_btn.clone();
        let win_title_cat = window.clone();
        let page_title_cat = cat_page_title.clone();
        let page_sub_cat = cat_page_sub.clone();
        let grid_cat = cat_grid_cards.clone();
        let catalog_copy = plugins_catalog.clone();
        let pm_cat = plugin_manager.clone();
        let on_inst_cat = on_plugin_installed.clone();
        let on_ref_hdr_cat = refresh_header_status.clone();
        let open_detail_cat = open_plugin_detail.clone();
        let last_page_cat = last_page.clone();

        Rc::new(move |selected_category: &'static str| {
            *last_page_cat.borrow_mut() = "category";
            win_title_cat.set_title(Some(selected_category));
            page_title_cat.set_text(&format!("{} Plugins", selected_category));
            page_sub_cat.set_text(&format!("Browse and install extensions in the {} category", selected_category));

            // Clear previous grid items
            while let Some(child) = grid_cat.first_child() {
                grid_cat.remove(&child);
            }

            // Filter plugins matching this category
            let matching: Vec<&PluginInfo> = catalog_copy.iter()
                .filter(|item| item.category.to_lowercase().contains(&selected_category.to_lowercase()))
                .collect();

            if matching.is_empty() {
                let empty_lbl = Label::new(Some("No plugins found in this category."));
                empty_lbl.add_css_class("dim-label");
                empty_lbl.set_margin_top(20);
                grid_cat.attach(&empty_lbl, 0, 0, 2, 1);
            } else {
                for (idx, item) in matching.iter().enumerate() {
                    let is_installed = pm_cat.borrow().plugins.contains_key(&item.id);
                    let card = create_catalog_card(
                        item,
                        is_installed,
                        open_detail_cat.clone(),
                        pm_cat.clone(),
                        on_inst_cat.clone(),
                        on_ref_hdr_cat.clone(),
                    );
                    let col = (idx % 2) as i32;
                    let row = (idx / 2) as i32;
                    grid_cat.attach(&card, col, row, 1, 1);
                }
            }

            back_btn_cat.set_visible(true);
            install_local_cat.set_visible(false);
            stack_cat.set_visible_child_name("category");
        })
    };

    // Connect Back Button
    let stack_back = stack.clone();
    let back_btn_clone = back_btn.clone();
    let install_local_clone = install_local_btn.clone();
    let window_title_back = window.clone();
    let last_page_back = last_page.clone();
    back_btn.connect_clicked(move |_| {
        let cur = stack_back.visible_child_name();
        let last = *last_page_back.borrow();

        if cur.as_deref() == Some("detail") && last == "category" {
            stack_back.set_visible_child_name("category");
            *last_page_back.borrow_mut() = "main";
        } else {
            stack_back.set_visible_child_name("main");
            back_btn_clone.set_visible(false);
            install_local_clone.set_visible(true);
            window_title_back.set_title(Some("Plugin Store"));
            *last_page_back.borrow_mut() = "main";
        }
    });

    // Connect "My Plugins" button
    let stack_my_plugins = stack.clone();
    let back_btn_my_plugins = back_btn.clone();
    let install_local_my_plugins = install_local_btn.clone();
    let window_title_my_plugins = window.clone();
    let render_inst_clone = render_installed_list.clone();
    let last_page_my_plugins = last_page.clone();

    my_plugins_btn.connect_clicked(move |_| {
        *last_page_my_plugins.borrow_mut() = "main";
        render_inst_clone();
        stack_my_plugins.set_visible_child_name("installed");
        back_btn_my_plugins.set_visible(true);
        install_local_my_plugins.set_visible(false);
        window_title_my_plugins.set_title(Some("Installed Plugins"));
    });

    // =========================================================================
    // Build Spotlight Carousel (Bazaar Style - 100% Full Artwork Banners)
    // =========================================================================
    let carousel = Carousel::new();
    carousel.set_spacing(12);
    carousel.set_allow_mouse_drag(true);
    carousel.set_allow_scroll_wheel(true);
    carousel.set_hexpand(true);
    carousel.set_size_request(-1, 105);

    let spotlight_plugins = resolve_spotlight_plugins(&plugins_catalog, &spotlight_ids);
    let mut chunk = Vec::new();
    for (i, item) in spotlight_plugins.iter().enumerate() {
        let card = create_spotlight_card(item, open_plugin_detail.clone());
        chunk.push(card);

        if chunk.len() == 3 || i == spotlight_plugins.len() - 1 {
            let slide_box = Box::new(Orientation::Horizontal, 12);
            slide_box.set_hexpand(true);
            slide_box.set_homogeneous(true);
            slide_box.set_size_request(-1, 105);
            let card_count = chunk.len();
            for c in chunk.drain(..) {
                slide_box.append(&c);
            }
            // Keep 3 homogeneous columns on partial final slides so banners keep their 2.78:1 aspect ratio
            for _ in card_count..3 {
                let spacer = Box::new(Orientation::Vertical, 0);
                spacer.set_hexpand(true);
                spacer.set_can_target(false);
                slide_box.append(&spacer);
            }
            carousel.append(&slide_box);
        }
    }

    scroll_content.append(&carousel);

    let has_multiple_slides = carousel.n_pages() > 1;
    prev_btn.set_visible(has_multiple_slides);
    next_btn.set_visible(has_multiple_slides);

    // Carousel Navigation chevrons
    let car_clone1 = carousel.clone();
    prev_btn.connect_clicked(move |_| {
        let cur = car_clone1.position().round() as u32;
        if cur > 0 {
            let child = car_clone1.nth_page(cur - 1);
            car_clone1.scroll_to(&child, true);
        }
    });

    let car_clone2 = carousel.clone();
    next_btn.connect_clicked(move |_| {
        let cur = car_clone2.position().round() as u32;
        let total = car_clone2.n_pages();
        if cur + 1 < total {
            let child = car_clone2.nth_page(cur + 1);
            car_clone2.scroll_to(&child, true);
        }
    });

    // Automatic scrolling every 4.5 seconds
    let car_auto = carousel.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(4500), move || {
        let total = car_auto.n_pages();
        if total > 1 {
            let cur = car_auto.position().round() as u32;
            let next_page = (cur + 1) % total;
            let child = car_auto.nth_page(next_page);
            car_auto.scroll_to(&child, true);
        }
        glib::ControlFlow::Continue
    });

    let dots = CarouselIndicatorDots::new();
    dots.set_carousel(Some(&carousel));
    dots.set_halign(gtk::Align::Center);
    dots.set_margin_top(4);
    dots.set_margin_bottom(6);
    dots.set_visible(has_multiple_slides);
    scroll_content.append(&dots);

    // =========================================================================
    // Centered Content Box for Categories and Popular Plugins (Locked within Red Box, 760px)
    // =========================================================================
    let center_clamp = Clamp::builder()
        .maximum_size(760)
        .tightening_threshold(760)
        .build();

    let center_content_box = Box::new(Orientation::Vertical, 18);
    center_content_box.set_hexpand(true);

    // 3. 3-Column x 2-Row Category Grid
    let cat_title = Label::new(Some("Categories"));
    cat_title.add_css_class("store-section-title");
    cat_title.set_xalign(0.0);
    center_content_box.append(&cat_title);

    let cat_grid = Grid::new();
    cat_grid.set_column_spacing(10);
    cat_grid.set_row_spacing(10);
    cat_grid.set_column_homogeneous(true);
    cat_grid.set_hexpand(true);

    let categories_def = [
        ("Utility", "tools-symbolic", 0, 0),
        ("System", "preferences-system-symbolic", 1, 0),
        ("Audio", "audio-volume-high-symbolic", 2, 0),
        ("Streaming", "video-display-symbolic", 0, 1),
        ("Productivity", "x-office-document-symbolic", 1, 1),
        ("Developer", "utilities-terminal-symbolic", 2, 1),
    ];

    let mut cat_buttons = Vec::new();
    for (cat_name, icon_name, col, row) in categories_def {
        let btn = Button::new();
        btn.add_css_class("store-category-tile");

        let tile_box = Box::new(Orientation::Horizontal, 8);
        tile_box.set_halign(gtk::Align::Center);
        tile_box.set_valign(gtk::Align::Center);

        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(18);
        let label = Label::new(Some(cat_name));
        label.add_css_class("heading");

        tile_box.append(&icon);
        tile_box.append(&label);
        btn.set_child(Some(&tile_box));

        cat_grid.attach(&btn, col, row, 1, 1);
        cat_buttons.push((cat_name, btn));
    }
    center_content_box.append(&cat_grid);

    // =========================================================================
    // 4. Popular Plugins Catalog
    // =========================================================================
    let pop_header_box = Box::new(Orientation::Vertical, 2);
    let plugins_title = Label::new(Some("Popular Plugins"));
    plugins_title.add_css_class("store-section-title");
    plugins_title.set_xalign(0.0);

    let plugins_sub = Label::new(Some("Trending and community favorite Stream Deck plugins"));
    plugins_sub.add_css_class("store-section-subtitle");
    plugins_sub.set_xalign(0.0);

    pop_header_box.append(&plugins_title);
    pop_header_box.append(&plugins_sub);
    center_content_box.append(&pop_header_box);

    let list_box = Box::new(Orientation::Vertical, 10);
    let catalog_items_rc: Rc<RefCell<Vec<(Box, String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let active_cat = Rc::new(RefCell::new(Option::<String>::None));

    let filter_catalog = {
        let items = catalog_items_rc.clone();
        let search_ent = search_entry.clone();
        let active_cat_ref = active_cat.clone();
        Rc::new(move || {
            let query = search_ent.text().to_string().to_lowercase();
            let cur_cat = active_cat_ref.borrow().clone();

            for (card, title, cats) in items.borrow().iter() {
                let matches_cat = match &cur_cat {
                    Some(c) => cats.contains(c.as_str()),
                    None => true,
                };
                let matches_search = query.is_empty() || title.to_lowercase().contains(&query);
                card.set_visible(matches_cat && matches_search);
            }
        })
    };

    let rebuild_main_catalog: Rc<dyn Fn()> = {
        let lb = list_box.clone();
        let items_store = catalog_items_rc.clone();
        let cat_data = plugins_catalog.clone();
        let pm_cat_main = plugin_manager.clone();
        let open_det = open_plugin_detail.clone();
        let on_inst = on_plugin_installed.clone();
        let ref_hdr = refresh_header_status.clone();
        let filt = filter_catalog.clone();
        Rc::new(move || {
            while let Some(child) = lb.first_child() {
                lb.remove(&child);
            }
            let mut new_items = Vec::new();
            for item in cat_data.iter() {
                let is_installed = pm_cat_main.borrow().plugins.contains_key(&item.id);
                let card = create_catalog_card(
                    item,
                    is_installed,
                    open_det.clone(),
                    pm_cat_main.clone(),
                    on_inst.clone(),
                    ref_hdr.clone(),
                );
                lb.append(&card);
                new_items.push((card, item.name.clone(), item.category.clone()));
            }
            *items_store.borrow_mut() = new_items;
            filt();
        })
    };
    *refresh_main_catalog_cell.borrow_mut() = Some(rebuild_main_catalog.clone());
    rebuild_main_catalog();

    center_content_box.append(&list_box);
    center_clamp.set_child(Some(&center_content_box));
    scroll_content.append(&center_clamp);

    for (cat_name, btn) in cat_buttons {
        let open_cat = open_category_page.clone();
        btn.connect_clicked(move |_| {
            open_cat(cat_name);
        });
    }

    let filter_for_search = filter_catalog.clone();
    search_entry.connect_search_changed(move |_| {
        filter_for_search();
    });

    scrolled.set_child(Some(&scroll_content));
    main_content_box.append(&scrolled);

    // Add Main Page, Detail Page, Installed Plugins Page, and Category Page to Stack
    stack.add_named(&main_content_box, Some("main"));
    stack.add_named(&detail_page_box, Some("detail"));
    stack.add_named(&installed_page_box, Some("installed"));
    stack.add_named(&category_page_box, Some("category"));
    stack.set_visible_child_name("main");

    toolbar_view.set_content(Some(&stack));
    window.set_content(Some(&toolbar_view));

    // Handle "Install from ZIP (.zip)..." dialog
    {
        let pm_zip = plugin_manager.clone();
        let on_installed_zip = on_plugin_installed.clone();
        let ref_hdr_zip = refresh_header_status.clone();
        let ref_cat_zip = refresh_main_catalog_cell.clone();
        let window_zip = window.clone();
        let popover_zip = install_local_popover.clone();
        let menu_btn_zip = install_local_btn.clone();
        let my_plugins_zip = my_plugins_btn.clone();

        install_zip_btn.connect_clicked(move |_| {
            popover_zip.popdown();
            let dialog = FileDialog::builder()
                .title("Select Plugin Archive (.zip)")
                .build();

            let filter = gtk::FileFilter::new();
            filter.set_name(Some("Plugin ZIP Archives (*.zip)"));
            filter.add_mime_type("application/zip");
            filter.add_mime_type("application/x-zip-compressed");
            filter.add_pattern("*.zip");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            dialog.set_filters(Some(&filters));

            let pm_inner = pm_zip.clone();
            let cb_inner = on_installed_zip.clone();
            let ref_hdr_in = ref_hdr_zip.clone();
            let ref_cat_in = ref_cat_zip.clone();
            let win_in = window_zip.clone();
            let btn_in = menu_btn_zip.clone();
            let my_plugins_in = my_plugins_zip.clone();

            dialog.open(Some(&window_zip), Option::<&gio::Cancellable>::None, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        btn_in.set_sensitive(false);
                        btn_in.set_label("Installing...");

                        let p_dir = pm_inner.borrow().plugins_dir.clone();
                        let (tx, rx) = async_channel::bounded::<Result<crate::core::plugin_manager::InstalledPluginSummary, String>>(1);
                        std::thread::spawn(move || {
                            let res = PluginManager::extract_plugin_zip(&p_dir, &path);
                            let _ = tx.send_blocking(res);
                        });

                        let pm_done = pm_inner.clone();
                        let cb_done = cb_inner.clone();
                        let ref_hdr_done = ref_hdr_in.clone();
                        let ref_cat_done = ref_cat_in.clone();
                        let win_done = win_in.clone();
                        let btn_done = btn_in.clone();
                        let my_plugins_done = my_plugins_in.clone();

                        glib::spawn_future_local(async move {
                            if let Ok(res) = rx.recv().await {
                                btn_done.set_label("Install Local");
                                btn_done.set_sensitive(true);
                                match res {
                                    Ok(summary) => {
                                        pm_done.borrow_mut().rescan_plugins();
                                        cb_done();
                                        ref_hdr_done();
                                        if let Some(ref rc) = *ref_cat_done.borrow() {
                                            rc();
                                        }
                                        show_plugin_installed_dialog(&win_done, &summary, my_plugins_done.clone());
                                    }
                                    Err(err) => {
                                        tracing::error!("Failed to install plugin from ZIP: {err}");
                                        show_plugin_install_error_dialog(&win_done, "ZIP Installation Failed", &err);
                                    }
                                }
                            }
                        });
                    }
                }
            });
        });
    }

    // Handle "Install from Folder..." dialog
    {
        let pm_local = plugin_manager.clone();
        let on_installed_local = on_plugin_installed.clone();
        let ref_hdr_local = refresh_header_status.clone();
        let ref_cat_local = refresh_main_catalog_cell.clone();
        let window_ref = window.clone();
        let popover_folder = install_local_popover.clone();

        install_folder_btn.connect_clicked(move |_| {
            popover_folder.popdown();
            let dialog = FileDialog::builder()
                .title("Select Plugin Folder (containing manifest.json)")
                .build();

            let pm_inner = pm_local.clone();
            let cb_inner = on_installed_local.clone();
            let ref_hdr_in = ref_hdr_local.clone();
            let ref_cat_in = ref_cat_local.clone();
            let win_in = window_ref.clone();
            dialog.select_folder(Some(&window_ref), Option::<&gio::Cancellable>::None, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        match pm_inner.borrow_mut().install_from_dir(&path) {
                            Ok(_) => {
                                cb_inner();
                                ref_hdr_in();
                                if let Some(ref rc) = *ref_cat_in.borrow() {
                                    rc();
                                }
                            }
                            Err(err) => {
                                tracing::error!("Failed to install plugin from folder: {err}");
                                show_plugin_install_error_dialog(&win_in, "Folder Installation Failed", &err);
                            }
                        }
                    }
                }
            });
        });
    }

    window
}

fn show_screenshot_lightbox(
    parent: &Window,
    plugin_name: &str,
    items: Vec<(PathBuf, String)>,
    start_idx: usize,
) {
    let dialog = adw::Dialog::builder()
        .content_width(820)
        .content_height(560)
        .can_close(true)
        .build();

    let root = Box::new(Orientation::Vertical, 12);
    root.add_css_class("store-lightbox-root");

    let hdr = Box::new(Orientation::Horizontal, 10);
    let title_lbl = Label::new(None);
    title_lbl.add_css_class("heading");
    title_lbl.set_xalign(0.0);
    title_lbl.set_hexpand(true);

    let counter_lbl = Label::new(None);
    counter_lbl.add_css_class("dim-label");

    let close_btn = Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.add_css_class("circular");
    let dlg_close = dialog.clone();
    close_btn.connect_clicked(move |_| {
        dlg_close.close();
    });

    hdr.append(&title_lbl);
    hdr.append(&counter_lbl);
    hdr.append(&close_btn);
    root.append(&hdr);

    let frame = Box::new(Orientation::Vertical, 0);
    frame.add_css_class("store-lightbox-frame");
    frame.set_vexpand(true);
    frame.set_hexpand(true);
    frame.set_overflow(gtk::Overflow::Hidden);

    let pic = Picture::new();
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk::ContentFit::Contain);
    pic.set_hexpand(true);
    pic.set_vexpand(true);
    pic.set_margin_top(12);
    pic.set_margin_bottom(12);
    pic.set_margin_start(12);
    pic.set_margin_end(12);
    frame.append(&pic);
    root.append(&frame);

    let nav_row = Box::new(Orientation::Horizontal, 10);
    nav_row.set_halign(gtk::Align::Center);
    let prev_btn = Button::from_icon_name("go-previous-symbolic");
    prev_btn.add_css_class("install-secondary-btn");
    let next_btn = Button::from_icon_name("go-next-symbolic");
    next_btn.add_css_class("install-secondary-btn");
    nav_row.append(&prev_btn);
    nav_row.append(&next_btn);
    nav_row.set_visible(items.len() > 1);
    root.append(&nav_row);

    let cur_idx = Rc::new(std::cell::Cell::new(start_idx));
    let items_rc = Rc::new(items);
    let pname = plugin_name.to_string();

    let render_slide: Rc<dyn Fn()> = {
        let idx_ref = cur_idx.clone();
        let items_ref = items_rc.clone();
        let pic_ref = pic.clone();
        let title_ref = title_lbl.clone();
        let cnt_ref = counter_lbl.clone();
        let prev_ref = prev_btn.clone();
        let next_ref = next_btn.clone();
        Rc::new(move || {
            let i = idx_ref.get();
            if let Some((path, cap)) = items_ref.get(i) {
                let file = gtk::gio::File::for_path(path);
                pic_ref.set_file(Some(&file));
                title_ref.set_text(&format!("{pname} — {cap}"));
                cnt_ref.set_text(&format!("{} / {}", i + 1, items_ref.len()));
                prev_ref.set_sensitive(i > 0);
                next_ref.set_sensitive(i + 1 < items_ref.len());
            }
        })
    };

    {
        let idx_p = cur_idx.clone();
        let ren_p = render_slide.clone();
        prev_btn.connect_clicked(move |_| {
            let i = idx_p.get();
            if i > 0 {
                idx_p.set(i - 1);
                ren_p();
            }
        });
    }
    {
        let idx_n = cur_idx.clone();
        let total = items_rc.len();
        let ren_n = render_slide.clone();
        next_btn.connect_clicked(move |_| {
            let i = idx_n.get();
            if i + 1 < total {
                idx_n.set(i + 1);
                ren_n();
            }
        });
    }

    render_slide();
    dialog.set_child(Some(&root));
    dialog.present(Some(parent));
}

fn show_plugin_installed_dialog(
    parent: &Window,
    summary: &crate::core::plugin_manager::InstalledPluginSummary,
    my_plugins_btn: Button,
) {
    let dialog = adw::Dialog::builder()
        .content_width(400)
        .can_close(true)
        .build();

    let root = Box::new(Orientation::Vertical, 14);
    root.add_css_class("install-result-root");
    root.set_halign(gtk::Align::Fill);

    // Top Hero: Plugin thumbnail with green verified checkmark OR green glow checkmark badge
    let hero_has_thumb = summary
        .thumbnail_path
        .as_deref()
        .filter(|p| std::path::Path::new(p).exists())
        .and_then(|p| crate::ui::canvas::create_rounded_texture(p, 168, 78, 12.0, true));

    if let Some(tex) = hero_has_thumb {
        let ov = gtk::Overlay::new();
        ov.set_halign(gtk::Align::Center);
        let pic = Picture::new();
        pic.set_paintable(Some(&tex));
        pic.set_size_request(168, 78);
        pic.set_content_fit(gtk::ContentFit::Contain);
        ov.set_child(Some(&pic));

        let badge = Box::new(Orientation::Horizontal, 0);
        badge.add_css_class("install-thumb-badge");
        badge.set_halign(gtk::Align::End);
        badge.set_valign(gtk::Align::End);
        let chk = Image::from_icon_name("object-select-symbolic");
        chk.set_pixel_size(14);
        chk.set_halign(gtk::Align::Center);
        chk.set_valign(gtk::Align::Center);
        chk.set_hexpand(true);
        chk.set_vexpand(true);
        badge.append(&chk);
        ov.add_overlay(&badge);
        root.append(&ov);
    } else {
        let badge = Box::new(Orientation::Horizontal, 0);
        badge.add_css_class("install-badge-success");
        badge.set_halign(gtk::Align::Center);
        let chk = Image::from_icon_name("object-select-symbolic");
        chk.set_pixel_size(26);
        chk.set_halign(gtk::Align::Center);
        chk.set_valign(gtk::Align::Center);
        chk.set_hexpand(true);
        chk.set_vexpand(true);
        badge.append(&chk);
        root.append(&badge);
    }

    // Title & Subtitle
    let header_box = Box::new(Orientation::Vertical, 4);
    let heading = Label::new(Some("Plugin Installed"));
    heading.add_css_class("title-2");
    heading.set_halign(gtk::Align::Center);

    let subtitle = Label::new(Some("Actions are ready to drag & drop from the sidebar."));
    subtitle.add_css_class("dim-label");
    subtitle.set_halign(gtk::Align::Center);
    subtitle.set_wrap(true);
    subtitle.set_justify(gtk::Justification::Center);

    header_box.append(&heading);
    header_box.append(&subtitle);
    root.append(&header_box);

    // Midnight Summary Card
    let card = Box::new(Orientation::Vertical, 6);
    card.add_css_class("install-plugin-card");

    let top_row = Box::new(Orientation::Horizontal, 8);
    let name_lbl = Label::new(Some(&summary.name));
    name_lbl.add_css_class("heading");
    name_lbl.set_hexpand(true);
    name_lbl.set_xalign(0.0);
    name_lbl.set_wrap(true);

    let ver_str = if summary.version.starts_with('v') {
        summary.version.clone()
    } else {
        format!("v{}", summary.version)
    };
    let ver_pill = Label::new(Some(&ver_str));
    ver_pill.add_css_class("install-version-pill");
    ver_pill.set_valign(gtk::Align::Center);

    top_row.append(&name_lbl);
    top_row.append(&ver_pill);
    card.append(&top_row);

    let meta_text = if !summary.author.is_empty() {
        format!("by {} • {}", summary.author, summary.id)
    } else {
        summary.id.clone()
    };
    let meta_lbl = Label::new(Some(&meta_text));
    meta_lbl.add_css_class("dim-label");
    meta_lbl.set_xalign(0.0);
    meta_lbl.set_ellipsize(pango::EllipsizeMode::End);
    card.append(&meta_lbl);

    root.append(&card);

    // Bottom Action Buttons
    let btn_row = Box::new(Orientation::Horizontal, 10);
    btn_row.set_homogeneous(true);
    btn_row.set_margin_top(4);

    let view_btn = Button::with_label("My Plugins");
    view_btn.add_css_class("install-secondary-btn");

    let done_btn = Button::with_label("Done");
    done_btn.add_css_class("install-primary-btn");

    let dlg_view = dialog.clone();
    view_btn.connect_clicked(move |_| {
        dlg_view.close();
        my_plugins_btn.emit_clicked();
    });

    let dlg_done = dialog.clone();
    done_btn.connect_clicked(move |_| {
        dlg_done.close();
    });

    btn_row.append(&view_btn);
    btn_row.append(&done_btn);
    root.append(&btn_row);

    dialog.set_child(Some(&root));
    dialog.present(Some(parent));
}

fn show_plugin_install_error_dialog(parent: &Window, title: &str, err: &str) {
    let dialog = adw::Dialog::builder()
        .content_width(390)
        .can_close(true)
        .build();

    let root = Box::new(Orientation::Vertical, 14);
    root.add_css_class("install-result-root");

    let badge = Box::new(Orientation::Horizontal, 0);
    badge.add_css_class("install-badge-error");
    badge.set_halign(gtk::Align::Center);
    let icon = Image::from_icon_name("dialog-error-symbolic");
    icon.set_pixel_size(26);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon.set_hexpand(true);
    icon.set_vexpand(true);
    badge.append(&icon);
    root.append(&badge);

    let heading = Label::new(Some(title));
    heading.add_css_class("title-2");
    heading.set_halign(gtk::Align::Center);
    root.append(&heading);

    let card = Box::new(Orientation::Vertical, 4);
    card.add_css_class("install-plugin-card");
    let err_lbl = Label::new(Some(err));
    err_lbl.set_wrap(true);
    err_lbl.set_xalign(0.0);
    card.append(&err_lbl);
    root.append(&card);

    let close_btn = Button::with_label("Close");
    close_btn.add_css_class("install-secondary-btn");
    close_btn.set_margin_top(4);
    let dlg_close = dialog.clone();
    close_btn.connect_clicked(move |_| {
        dlg_close.close();
    });
    root.append(&close_btn);

    dialog.set_child(Some(&root));
    dialog.present(Some(parent));
}

// =========================================================================
// Spotlight Card Builder: 100% Full Artwork Banner (NO Bottom Bar/Name)
// Covers the entire card edge-to-edge
// =========================================================================
fn create_spotlight_card(
    item: &PluginInfo,
    on_click_detail: Rc<dyn Fn(PluginInfo)>,
) -> Box {
    let card = Box::new(Orientation::Vertical, 0);
    card.add_css_class("store-spotlight-card");
    card.set_hexpand(true);
    card.set_vexpand(false);
    card.set_size_request(292, 105);
    card.set_cursor_from_name(Some("pointer"));

    let mut has_real_thumb = false;
    let thumb_pic = Picture::new();
    thumb_pic.set_can_shrink(true);
    thumb_pic.set_content_fit(gtk::ContentFit::Fill);
    thumb_pic.set_hexpand(true);
    thumb_pic.set_vexpand(true);
    thumb_pic.set_size_request(292, 105);
    thumb_pic.add_css_class("store-spotlight-thumbnail");

    if let Some(ref path) = item.thumbnail {
        if std::path::Path::new(path).exists() {
            let file = gtk::gio::File::for_path(path);
            thumb_pic.set_file(Some(&file));
            has_real_thumb = true;
        }
    }

    if has_real_thumb {
        card.append(&thumb_pic);
    } else {
        let fallback_box = Box::new(Orientation::Vertical, 0);
        fallback_box.add_css_class("thumb-banner-fallback");
        fallback_box.add_css_class(&item.fallback_grad);
        fallback_box.set_hexpand(true);
        fallback_box.set_vexpand(true);
        fallback_box.set_size_request(292, 105);

        let icon_preview = Image::from_icon_name(if item.is_file_icon { "system-run-symbolic" } else { &item.icon });
        icon_preview.set_pixel_size(42);
        icon_preview.set_halign(gtk::Align::Center);
        icon_preview.set_valign(gtk::Align::Center);
        icon_preview.set_vexpand(true);
        fallback_box.append(&icon_preview);

        card.append(&fallback_box);
    }

    // Make entire spotlight banner clickable to navigate to Detail Page
    let click_gesture = GestureClick::new();
    let item_clone = item.clone();
    let on_detail_clone = on_click_detail.clone();
    click_gesture.connect_pressed(move |_, _, _, _| {
        on_detail_clone(item_clone.clone());
    });
    card.add_controller(click_gesture);

    card
}

fn create_catalog_card(
    item: &PluginInfo,
    is_installed: bool,
    on_click_detail: Rc<dyn Fn(PluginInfo)>,
    plugin_manager: Rc<RefCell<PluginManager>>,
    on_plugin_installed: Rc<dyn Fn()>,
    on_refresh_header: Rc<dyn Fn()>,
) -> Box {
    let card = Box::new(Orientation::Vertical, 6);
    card.add_css_class("card");
    card.add_css_class("store-card");
    card.set_cursor_from_name(Some("pointer"));

    let escaped_name = glib::markup_escape_text(&item.name);
    let card_summary = if !item.short_description.is_empty() {
        &item.short_description
    } else {
        &item.description
    };
    let escaped_desc = glib::markup_escape_text(card_summary);
    let row = ActionRow::builder()
        .title(escaped_name.as_str())
        .subtitle(escaped_desc.as_str())
        .build();

    let img = if item.is_file_icon {
        let direct = std::path::PathBuf::from(&item.icon);
        let asset_path = glib::user_data_dir()
            .join("ballista")
            .join("plugins")
            .join(&item.id)
            .join("assets")
            .join(&item.icon);
        if direct.exists() {
            let img = Image::from_file(&direct);
            img.set_pixel_size(32);
            img
        } else if asset_path.exists() {
            let img = Image::from_file(&asset_path);
            img.set_pixel_size(32);
            img
        } else {
            let img = Image::from_icon_name("system-run-symbolic");
            img.set_pixel_size(32);
            img
        }
    } else {
        let img = Image::from_icon_name(&item.icon);
        img.set_pixel_size(32);
        img
    };
    row.add_prefix(&img);

    let action_box = Box::new(Orientation::Vertical, 4);
    action_box.set_valign(gtk::Align::Center);
    action_box.set_halign(gtk::Align::End);

    let btn = Button::with_label(if is_installed { "Installed" } else { "Install" });
    btn.set_valign(gtk::Align::Center);
    btn.set_halign(gtk::Align::End);

    let pbar = gtk::ProgressBar::new();
    pbar.add_css_class("store-pbar");
    pbar.set_size_request(82, 4);
    pbar.set_visible(false);

    action_box.append(&btn);
    action_box.append(&pbar);

    if is_installed {
        let pm_b = plugin_manager.borrow();
        let mut has_update = false;
        if let Some(inst) = pm_b.plugins.get(&item.id) {
            if is_newer_version(&item.version, &inst.version) {
                has_update = true;
            }
        }
        drop(pm_b);

        if has_update {
            btn.set_label("Update");
            btn.add_css_class("store-updates-amber-btn");
            btn.set_sensitive(true);

            let pm_update = plugin_manager.clone();
            let on_inst_action = on_plugin_installed.clone();
            let on_ref_hdr = on_refresh_header.clone();
            let btn_clone = btn.clone();
            let pbar_clone = pbar.clone();
            let d_url_up = item.download_url.clone().unwrap_or_default();
            let pid_up = item.id.clone();

            btn.connect_clicked(move |_| {
                btn_clone.set_label("Updating...");
                btn_clone.remove_css_class("store-updates-amber-btn");
                btn_clone.add_css_class("btn-downloading");
                btn_clone.set_sensitive(false);
                pbar_clone.set_visible(true);
                pbar_clone.pulse();

                let pbar_pulse = pbar_clone.clone();
                let pulse_source = glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
                    pbar_pulse.pulse();
                    glib::ControlFlow::Continue
                });
                let pulse_cell = std::rc::Rc::new(std::cell::Cell::new(Some(pulse_source)));

                let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
                let d_url = d_url_up.clone();
                let p_id = pid_up.clone();
                let p_dir = pm_update.borrow().plugins_dir.clone();

                std::thread::spawn(move || {
                    let res = PluginManager::download_and_extract_plugin(&p_dir, &p_id, &d_url);
                    let _ = tx.send_blocking(res);
                });

                let btn_done = btn_clone.clone();
                let pbar_done = pbar_clone.clone();
                let on_inst_done = on_inst_action.clone();
                let on_ref_done = on_ref_hdr.clone();
                let pm_done = pm_update.clone();
                let pulse_done = pulse_cell.clone();

                glib::spawn_future_local(async move {
                    if let Ok(res) = rx.recv().await {
                        if let Some(src) = pulse_done.take() {
                            src.remove();
                        }
                        pbar_done.set_visible(false);
                        match res {
                            Ok(()) => {
                                pm_done.borrow_mut().rescan_plugins();
                                btn_done.set_label("Installed");
                                btn_done.remove_css_class("btn-downloading");
                                btn_done.add_css_class("btn-installed");
                                btn_done.set_sensitive(false);
                                on_inst_done();
                                on_ref_done();
                            }
                            Err(e) => {
                                tracing::error!("Failed to update plugin: {e}");
                                btn_done.set_label("Update");
                                btn_done.remove_css_class("btn-downloading");
                                btn_done.add_css_class("store-updates-amber-btn");
                                btn_done.set_sensitive(true);
                            }
                        }
                    }
                });
            });
        } else {
            btn.add_css_class("btn-installed");
            btn.set_sensitive(false);
        }
    } else {
        btn.add_css_class("btn-install");
        let btn_clone = btn.clone();
        let pbar_clone = pbar.clone();
        let pm_inner = plugin_manager.clone();
        let on_inst_inner = on_plugin_installed.clone();
        let on_ref_hdr = on_refresh_header.clone();
        let download_url = item.download_url.clone();
        let plugin_id = item.id.clone();

        btn.connect_clicked(move |_| {
            btn_clone.set_label("Downloading...");
            btn_clone.remove_css_class("btn-install");
            btn_clone.add_css_class("btn-downloading");
            btn_clone.set_sensitive(false);
            pbar_clone.set_visible(true);
            pbar_clone.pulse();

            let pbar_pulse = pbar_clone.clone();
            let pulse_source = glib::timeout_add_local(std::time::Duration::from_millis(80), move || {
                pbar_pulse.pulse();
                glib::ControlFlow::Continue
            });
            let pulse_cell = std::rc::Rc::new(std::cell::Cell::new(Some(pulse_source)));

            let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
            let d_url = download_url.clone().unwrap_or_default();
            let p_id = plugin_id.clone();
            let p_dir = pm_inner.borrow().plugins_dir.clone();

            std::thread::spawn(move || {
                let res = PluginManager::download_and_extract_plugin(&p_dir, &p_id, &d_url);
                let _ = tx.send_blocking(res);
            });

            let btn_done = btn_clone.clone();
            let pbar_done = pbar_clone.clone();
            let on_inst_done = on_inst_inner.clone();
            let on_ref_done = on_ref_hdr.clone();
            let pm_done = pm_inner.clone();
            let pulse_done = pulse_cell.clone();

            glib::spawn_future_local(async move {
                if let Ok(res) = rx.recv().await {
                    if let Some(src) = pulse_done.take() {
                        src.remove();
                    }
                    pbar_done.set_visible(false);
                    match res {
                        Ok(()) => {
                            pm_done.borrow_mut().rescan_plugins();
                            btn_done.set_label("Installed");
                            btn_done.remove_css_class("btn-downloading");
                            btn_done.add_css_class("btn-installed");
                            btn_done.set_sensitive(false);
                            on_inst_done();
                            on_ref_done();
                        }
                        Err(e) => {
                            tracing::error!("Failed to install plugin: {e}");
                            btn_done.set_label("Install");
                            btn_done.remove_css_class("btn-downloading");
                            btn_done.add_css_class("btn-install");
                            btn_done.set_sensitive(true);
                        }
                    }
                }
            });
        });
    }
    row.add_suffix(&action_box);
    card.append(&row);

    let meta_row = Box::new(Orientation::Horizontal, 8);
    meta_row.set_margin_start(16);
    meta_row.set_margin_end(14);
    meta_row.set_margin_bottom(10);

    let (rt_txt, rt_css) = match item.runtime {
        PluginRuntime::Ballista => ("Ballista Native", "store-card-runtime-ballista"),
        PluginRuntime::StreamController => ("StreamController", "store-card-runtime-sc"),
    };
    let rt_badge = Label::new(Some(rt_txt));
    rt_badge.add_css_class(rt_css);
    rt_badge.set_valign(gtk::Align::Center);
    meta_row.append(&rt_badge);

    let author_line = format!(
        "by {} • {} • {}",
        item.author,
        format_version_tag(&item.version),
        item.category
    );
    let meta = Label::new(Some(&author_line));
    meta.add_css_class("dim-label");
    meta.set_xalign(0.0);
    meta.set_valign(gtk::Align::Center);
    meta_row.append(&meta);
    card.append(&meta_row);

    // Clicking anywhere on the card row opens the detail page
    let click_gesture = GestureClick::new();
    let item_clone = item.clone();
    let on_detail_clone = on_click_detail.clone();
    click_gesture.connect_pressed(move |_, _, _, _| {
        on_detail_clone(item_clone.clone());
    });
    row.add_controller(click_gesture);

    card
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_plugin(id: &str) -> PluginInfo {
        PluginInfo {
            id: id.to_string(),
            name: id.to_string(),
            category: "Utility".to_string(),
            author: "tester".to_string(),
            license: "MIT".to_string(),
            version: "1.0.0".to_string(),
            runtime: PluginRuntime::Ballista,
            icon: "system-run-symbolic".to_string(),
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-utility".to_string(),
            short_description: String::new(),
            description: String::new(),
            tags: Vec::new(),
            github_url: None,
            download_url: None,
            screenshots: Vec::new(),
            surfaces: Vec::new(),
            compatible_devices: Vec::new(),
            actions: Vec::new(),
            rating: None,
            rating_count: 0,
            local_path: None,
        }
    }

    #[test]
    fn resolve_spotlight_preserves_order_and_deduplicates() {
        let plugins = vec![dummy_plugin("a"), dummy_plugin("b"), dummy_plugin("c")];
        let ids = vec![
            "c".to_string(),
            "missing".to_string(),
            "a".to_string(),
            "c".to_string(),
        ];
        let resolved: Vec<&str> = resolve_spotlight_plugins(&plugins, &ids)
            .into_iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(resolved, vec!["c", "a"]);
    }

    #[test]
    fn resolve_spotlight_falls_back_when_empty() {
        let plugins: Vec<PluginInfo> = (0..8).map(|i| dummy_plugin(&format!("p{i}"))).collect();
        let resolved: Vec<&str> = resolve_spotlight_plugins(&plugins, &[])
            .into_iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(resolved, vec!["p0", "p1", "p2", "p3", "p4", "p5"]);
    }
}
