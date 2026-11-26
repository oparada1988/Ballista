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

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub category: String,
    pub author: String,
    pub version: String,
    pub icon: String,
    pub thumbnail: Option<String>,
    pub is_file_icon: bool,
    pub fallback_grad: String,
    pub description: String,
    pub download_url: Option<String>,
    pub local_path: Option<String>,
}

fn load_marketplace_catalog() -> Vec<PluginInfo> {
    let cache_dir = glib::user_cache_dir().join("ballista");
    let _ = std::fs::create_dir_all(&cache_dir);
    let cache_file = cache_dir.join("market_catalog.json");
    let img_cache_dir = cache_dir.join("market_images");
    let _ = std::fs::create_dir_all(&img_cache_dir);

    // Fetch live catalog from Ballista-Market with 3s timeout
    let url = "https://raw.githubusercontent.com/oparada1988/Ballista-Market/main/catalog.json";
    let _ = std::process::Command::new("curl")
        .arg("-s")
        .arg("-m")
        .arg("3")
        .arg("-o")
        .arg(&cache_file)
        .arg(url)
        .status();

    if let Ok(data) = std::fs::read_to_string(&cache_file) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
            if let Some(arr) = val.get("plugins").and_then(|v| v.as_array()) {
                let mut list = Vec::new();
                for p in arr {
                    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let author = p.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let version = p.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let category = p.get("category").and_then(|v| v.as_str()).unwrap_or("Utility").to_string();
                    let description = p.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let download_url = p.get("download_url").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let local_path: Option<String> = None;

                    // Cache and resolve thumbnail
                    let thumb_url = p.get("thumbnail").and_then(|v| v.as_str());
                    let thumbnail = if let Some(t_url) = thumb_url {
                        if t_url.starts_with("http") {
                            let thumb_file = img_cache_dir.join(format!("{id}_thumb.png"));
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
                        let icon_file = img_cache_dir.join(format!("{id}_icon.png"));
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
                    } else if icon_url.ends_with(".png") {
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
                        version,
                        icon,
                        thumbnail,
                        is_file_icon,
                        fallback_grad,
                        description,
                        download_url,
                        local_path,
                    });
                }
                return list;
            }
        }
    }

    // Bundled offline fallback (100% portable using XDG cache dirs and remote GitHub URLs)
    vec![
        PluginInfo {
            id: "com_oparada_AppLauncher".into(),
            name: "App Launcher".into(),
            category: "Utility".into(),
            author: "oparada1988".into(),
            version: "0.1.0".into(),
            icon: "action_launch.png".into(),
            thumbnail: {
                let p = img_cache_dir.join("com_oparada_AppLauncher_thumb.png");
                p.exists().then(|| p.to_string_lossy().to_string())
            },
            is_file_icon: true,
            fallback_grad: "thumb-grad-utility".into(),
            description: "Easily launch any installed host desktop application from your Stream Deck keys with automatic freedesktop icon and label setup.".into(),
            download_url: Some("https://github.com/oparada1988/AppLauncher/archive/refs/heads/main.zip".into()),
            local_path: None,
        },
        PluginInfo {
            id: "com_oparada_DiscordSC".into(),
            name: "DiscordSC".into(),
            category: "Streaming".into(),
            author: "oparada1988".into(),
            version: "1.2.4".into(),
            icon: "user-available-symbolic".into(),
            thumbnail: {
                let p = img_cache_dir.join("com_oparada_DiscordSC_thumb.png");
                p.exists().then(|| p.to_string_lossy().to_string())
            },
            is_file_icon: false,
            fallback_grad: "thumb-grad-streaming".into(),
            description: "Control your Discord client's mute, deafen, notifications, server status, and input modes directly from your Stream Deck.".into(),
            download_url: Some("https://github.com/oparada1988/DiscordSC/archive/refs/heads/main.zip".into()),
            local_path: None,
        },
        PluginInfo {
            id: "com_oparada_GoveeStreamController".into(),
            name: "Govee Controller".into(),
            category: "Utility".into(),
            author: "oparada1988".into(),
            version: "1.0.1".into(),
            icon: "lightbulb-symbolic".into(),
            thumbnail: {
                let p = img_cache_dir.join("com_oparada_GoveeStreamController_thumb.png");
                p.exists().then(|| p.to_string_lossy().to_string())
            },
            is_file_icon: false,
            fallback_grad: "thumb-grad-utility".into(),
            description: "Control Govee smart lights, LED strips, and home appliances directly from your Stream Deck.".into(),
            download_url: Some("https://github.com/oparada1988/Govee-Streamcontroller/archive/refs/heads/main.zip".into()),
            local_path: None,
        },
    ]
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
    let plugins_catalog = load_marketplace_catalog();

    // =========================================================================
    // Page 2: Plugin Detail Subview (Matching Mockup)
    // =========================================================================
    let detail_page_box = Box::new(Orientation::Vertical, 18);
    detail_page_box.set_margin_top(16);
    detail_page_box.set_margin_bottom(16);
    detail_page_box.set_margin_start(20);
    detail_page_box.set_margin_end(20);

    let detail_scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let detail_scroll_content = Box::new(Orientation::Vertical, 16);

    let detail_header_card = Box::new(Orientation::Horizontal, 16);
    detail_header_card.add_css_class("store-detail-header-card");
    detail_header_card.set_valign(gtk::Align::Center);

    let detail_icon = Image::from_icon_name("system-run-symbolic");
    detail_icon.set_pixel_size(64);
    detail_header_card.append(&detail_icon);

    let detail_title_col = Box::new(Orientation::Vertical, 4);
    detail_title_col.set_hexpand(true);
    detail_title_col.set_valign(gtk::Align::Center);

    let detail_name_lbl = Label::new(Some("Plugin Name"));
    detail_name_lbl.add_css_class("title-1");
    detail_name_lbl.set_xalign(0.0);

    let detail_meta_lbl = Label::new(Some("by author • v1.0.0 • Category"));
    detail_meta_lbl.add_css_class("dim-label");
    detail_meta_lbl.set_xalign(0.0);

    detail_title_col.append(&detail_name_lbl);
    detail_title_col.append(&detail_meta_lbl);
    detail_header_card.append(&detail_title_col);

    let detail_action_col = Box::new(Orientation::Vertical, 4);
    detail_action_col.set_valign(gtk::Align::Center);
    let detail_action_btn = Button::with_label("Install");
    detail_action_btn.add_css_class("btn-install");
    let detail_pbar = gtk::ProgressBar::new();
    detail_pbar.add_css_class("store-pbar");
    detail_pbar.set_size_request(88, 4);
    detail_pbar.set_visible(false);
    detail_action_col.append(&detail_action_btn);
    detail_action_col.append(&detail_pbar);
    detail_header_card.append(&detail_action_col);

    detail_scroll_content.append(&detail_header_card);

    let desc_title = Label::new(Some("Description"));
    desc_title.add_css_class("store-section-title");
    desc_title.set_xalign(0.0);
    detail_scroll_content.append(&desc_title);

    let desc_card = Box::new(Orientation::Vertical, 8);
    desc_card.add_css_class("store-detail-desc-card");
    let desc_lbl = Label::new(None);
    desc_lbl.set_wrap(true);
    desc_lbl.set_xalign(0.0);
    desc_card.append(&desc_lbl);
    detail_scroll_content.append(&desc_card);

    let shots_title = Label::new(Some("Screenshots"));
    shots_title.add_css_class("store-section-title");
    shots_title.set_xalign(0.0);
    detail_scroll_content.append(&shots_title);

    let shots_box = Box::new(Orientation::Horizontal, 12);
    shots_box.set_hexpand(true);
    shots_box.set_homogeneous(true);
    shots_box.set_vexpand(false);
    shots_box.set_size_request(-1, 105);
    shots_box.set_margin_bottom(6);

    let shot_box_1 = Box::new(Orientation::Vertical, 0);
    shot_box_1.add_css_class("store-screenshot-tile");
    shot_box_1.set_hexpand(true);
    shot_box_1.set_vexpand(false);
    shot_box_1.set_size_request(292, 105);

    let shot_pic_1 = Picture::new();
    shot_pic_1.set_can_shrink(true);
    shot_pic_1.set_content_fit(gtk::ContentFit::Fill);
    shot_pic_1.set_hexpand(true);
    shot_pic_1.set_vexpand(false);
    shot_pic_1.set_size_request(292, 105);
    shot_box_1.append(&shot_pic_1);
    shots_box.append(&shot_box_1);

    let shot_box_2 = Box::new(Orientation::Vertical, 0);
    shot_box_2.add_css_class("store-screenshot-tile");
    shot_box_2.add_css_class("thumb-grad-streaming");
    shot_box_2.set_hexpand(true);
    shot_box_2.set_vexpand(false);
    shot_box_2.set_size_request(292, 105);
    let shot_icon_2 = Image::from_icon_name("video-display-symbolic");
    shot_icon_2.set_pixel_size(36);
    shot_icon_2.set_halign(gtk::Align::Center);
    shot_icon_2.set_valign(gtk::Align::Center);
    shot_icon_2.set_vexpand(true);
    shot_box_2.append(&shot_icon_2);
    shots_box.append(&shot_box_2);

    let shot_box_3 = Box::new(Orientation::Vertical, 0);
    shot_box_3.add_css_class("store-screenshot-tile");
    shot_box_3.add_css_class("thumb-grad-system");
    shot_box_3.set_hexpand(true);
    shot_box_3.set_vexpand(false);
    shot_box_3.set_size_request(292, 105);
    let shot_icon_3 = Image::from_icon_name("utilities-system-monitor-symbolic");
    shot_icon_3.set_pixel_size(36);
    shot_icon_3.set_halign(gtk::Align::Center);
    shot_icon_3.set_valign(gtk::Align::Center);
    shot_icon_3.set_vexpand(true);
    shot_box_3.append(&shot_icon_3);
    shots_box.append(&shot_box_3);

    detail_scroll_content.append(&shots_box);

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
                let inst_ver = inst.version.trim_start_matches('v');
                let cat_ver = c.version.trim_start_matches('v');
                if cat_ver > inst_ver {
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
                        let inst_ver = p_obj.version.trim_start_matches('v');
                        let cat_ver = cat_item.version.trim_start_matches('v');
                        if cat_ver > inst_ver {
                            let update_btn = Button::with_label(&format!("Update to {}", cat_item.version));
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

    // Single persistent click handler for Detail Page Install button (prevents stacked duplicate downloads!)
    let current_detail_plugin: Rc<RefCell<Option<PluginInfo>>> = Rc::new(RefCell::new(None));
    {
        let cur_plugin_click = current_detail_plugin.clone();
        let btn_clone = detail_action_btn.clone();
        let pbar_clone = detail_pbar.clone();
        let pm_action = pm_detail.clone();
        let on_inst_action = on_installed_detail.clone();
        let ref_hdr_action = refresh_header_status.clone();
        let ref_cat_action = refresh_main_catalog_cell.clone();

        detail_action_btn.connect_clicked(move |_| {
            let Some(plugin) = cur_plugin_click.borrow().clone() else {
                return;
            };
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
            let d_url = plugin.download_url.clone().unwrap_or_default();
            let p_id = plugin.id.clone();
            let p_dir = pm_action.borrow().plugins_dir.clone();

            std::thread::spawn(move || {
                let res = PluginManager::download_and_extract_plugin(&p_dir, &p_id, &d_url);
                let _ = tx.send_blocking(res);
            });

            let btn_done = btn_clone.clone();
            let pbar_done = pbar_clone.clone();
            let on_inst_done = on_inst_action.clone();
            let ref_hdr_done = ref_hdr_action.clone();
            let ref_cat_done = ref_cat_action.clone();
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
                            btn_done.set_label("Installed");
                            btn_done.remove_css_class("btn-downloading");
                            btn_done.add_css_class("btn-installed");
                            btn_done.set_sensitive(false);
                            on_inst_done();
                            ref_hdr_done();
                            if let Some(ref rc) = *ref_cat_done.borrow() {
                                rc();
                            }
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

    let open_plugin_detail = Rc::new({
        let stack_inner = stack_ref.clone();
        let back_btn_inner = back_btn_ref.clone();
        let install_local_inner = install_local_btn_ref.clone();
        let win_title_inner = window_title_ref.clone();
        let name_lbl = detail_name_lbl.clone();
        let meta_lbl = detail_meta_lbl.clone();
        let desc_lbl = desc_lbl.clone();
        let icon_img = detail_icon.clone();
        let action_btn = detail_action_btn.clone();
        let pbar = detail_pbar.clone();
        let shot1 = shot_pic_1.clone();
        let pm_inner = pm_detail.clone();
        let cur_plugin_set = current_detail_plugin.clone();

        move |plugin: PluginInfo| {
            *cur_plugin_set.borrow_mut() = Some(plugin.clone());
            name_lbl.set_text(&plugin.name);
            meta_lbl.set_text(&format!("by {} • {} • {}", plugin.author, plugin.version, plugin.category));
            desc_lbl.set_text(&plugin.description);
            win_title_inner.set_title(Some(&plugin.name));

            // Load Icon
            if plugin.is_file_icon {
                if std::path::Path::new(&plugin.icon).exists() {
                    icon_img.set_from_file(Some(&plugin.icon));
                    icon_img.set_pixel_size(64);
                } else {
                    icon_img.set_icon_name(Some("system-run-symbolic"));
                    icon_img.set_pixel_size(64);
                }
            } else {
                icon_img.set_icon_name(Some(&plugin.icon));
                icon_img.set_pixel_size(64);
            }

            // Load Screenshot 1
            if let Some(ref thumb) = plugin.thumbnail {
                if std::path::Path::new(thumb).exists() {
                    let file = gtk::gio::File::for_path(thumb);
                    shot1.set_file(Some(&file));
                } else {
                    shot1.set_paintable(Option::<&gtk::gdk::Paintable>::None);
                }
            } else {
                shot1.set_paintable(Option::<&gtk::gdk::Paintable>::None);
            }

            // Check if installed
            let is_installed = pm_inner.borrow().plugins.contains_key(&plugin.id);
            action_btn.remove_css_class("btn-install");
            action_btn.remove_css_class("btn-downloading");
            action_btn.remove_css_class("btn-installed");
            pbar.set_visible(false);

            if is_installed {
                action_btn.set_label("Installed");
                action_btn.add_css_class("btn-installed");
                action_btn.set_sensitive(false);
            } else {
                action_btn.set_label("Install");
                action_btn.add_css_class("btn-install");
                action_btn.set_sensitive(true);
            }

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

    let mut chunk = Vec::new();
    for (i, item) in plugins_catalog.iter().enumerate() {
        let card = create_spotlight_card(item, open_plugin_detail.clone());
        chunk.push(card);

        if chunk.len() == 3 || i == plugins_catalog.len() - 1 {
            let slide_box = Box::new(Orientation::Horizontal, 12);
            slide_box.set_hexpand(true);
            slide_box.set_homogeneous(true);
            slide_box.set_size_request(-1, 105);
            for c in chunk.drain(..) {
                slide_box.append(&c);
            }
            carousel.append(&slide_box);
        }
    }

    scroll_content.append(&carousel);

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
    let escaped_desc = glib::markup_escape_text(&item.description);
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
            let inst_ver = inst.version.trim_start_matches('v');
            let cat_ver = item.version.trim_start_matches('v');
            if cat_ver > inst_ver {
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

    let author_line = format!("by {} • {} • {}", item.author, item.version, item.category);
    let meta = Label::new(Some(&author_line));
    meta.add_css_class("dim-label");
    meta.set_xalign(0.0);
    meta.set_margin_start(16);
    meta.set_margin_bottom(10);
    card.append(&meta);

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
