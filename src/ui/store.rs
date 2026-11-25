#![allow(deprecated)]

use crate::core::PluginManager;
use crate::models::WindowState;
use adw::prelude::*;
use adw::{ActionRow, Carousel, CarouselIndicatorDots, HeaderBar, ToolbarView, Window};
use gtk::{
    Box, Button, FileDialog, GestureClick, Grid, Image, Label, Orientation, Picture,
    ScrolledWindow, SearchEntry, Stack, StackTransitionType,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone)]
pub struct PluginInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub author: &'static str,
    pub version: &'static str,
    pub icon: &'static str,
    pub thumbnail: Option<&'static str>,
    pub is_file_icon: bool,
    pub fallback_grad: &'static str,
    pub description: &'static str,
    pub local_path: Option<&'static str>,
}

pub fn create_store_window(
    parent: &impl IsA<gtk::Window>,
    plugin_manager: Rc<RefCell<PluginManager>>,
    on_plugin_installed: Rc<dyn Fn()>,
) -> Window {
    let saved_state = WindowState::load();

    let window = Window::builder()
        .title("Plugin Store")
        .transient_for(parent)
        .modal(true)
        .default_width(saved_state.store_width)
        .default_height(saved_state.store_height)
        .build();

    window.add_css_class("ballista-modal");
    window.add_css_class("ballista-midnight-window");

    // Persist store window dimensions on close
    window.connect_close_request(|win| {
        let mut state = WindowState::load();
        let w = win.width().max(600);
        let h = win.height().max(400);
        state.store_width = w;
        state.store_height = h;
        state.save();
        glib::Propagation::Proceed
    });

    let toolbar_view = ToolbarView::new();

    // ===================== Header Bar =====================
    let header_bar = HeaderBar::new();
    header_bar.add_css_class("ballista-headerbar");

    let install_local_btn = Button::with_label("Install Local Plugin...");
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

    // Exactly 9 Featured Plugins Definition (3 slides x 3 cards = perfectly balanced)
    let plugins_catalog: [PluginInfo; 9] = [
        PluginInfo {
            id: "com_oparada_AppLauncher",
            name: "App Launcher",
            category: "Utility",
            author: "oparada1988",
            version: "v0.2.0",
            icon: "action_launch.png",
            thumbnail: Some("/home/oscar/.local/share/ballista/plugins/com_oparada_AppLauncher/assets/thumbnail.png"),
            is_file_icon: true,
            fallback_grad: "thumb-grad-utility",
            description: "Easily launch any installed host desktop application from your Stream Deck keys with automatic freedesktop icon and label setup. Compatible with StreamController manifests and workflows.",
            local_path: Some("/home/oscar/Github Projects/AppLauncher"),
        },
        PluginInfo {
            id: "com_community_obs",
            name: "OBS Studio",
            category: "Streaming",
            author: "Community",
            version: "v1.2.0",
            icon: "video-display-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-streaming",
            description: "Comprehensive Stream Deck control for OBS Studio. Switch scenes, trigger stinger transitions, start or stop streaming/recording, and toggle microphone mutes via OBS WebSocket.",
            local_path: None,
        },
        PluginInfo {
            id: "com_ballista_audioswitcher",
            name: "Audio Switcher",
            category: "Audio",
            author: "Ballista",
            version: "v1.0.0",
            icon: "audio-volume-high-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-audio",
            description: "Instantly toggle default PipeWire audio output sinks, input sources, headphone monitors, and custom hardware profile presets with zero latency.",
            local_path: None,
        },
        PluginInfo {
            id: "com_hardwarelabs_sysmon",
            name: "System Monitor",
            category: "System",
            author: "HardwareLabs",
            version: "v0.9.5",
            icon: "utilities-system-monitor-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-system",
            description: "Live hardware telemetry directly on your keys. Monitor real-time CPU usage, memory consumption, GPU temps, and network upload/download bandwidth.",
            local_path: None,
        },
        PluginInfo {
            id: "com_linuxmedia_controls",
            name: "Media Controls",
            category: "Utility",
            author: "LinuxMedia",
            version: "v1.1.0",
            icon: "media-playback-start-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-utility",
            description: "Native MPRIS integration for Spotify, YouTube, VLC, and desktop media players. Play, pause, skip tracks, and display live song titles on LCD keys.",
            local_path: None,
        },
        PluginInfo {
            id: "com_streamlabs_twitch",
            name: "Twitch Tools",
            category: "Streaming",
            author: "StreamLabs",
            version: "v1.4.2",
            icon: "camera-video-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-streaming",
            description: "Marker creation, commercial triggers, follower-only chat toggle, emote modes, and instantaneous stream clip automation for Twitch creators.",
            local_path: None,
        },
        PluginInfo {
            id: "com_voipmods_discord",
            name: "Discord Bridge",
            category: "Streaming",
            author: "VoipMods",
            version: "v2.1.0",
            icon: "user-available-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-streaming",
            description: "Mute microphone, deafen audio, switch voice channels, send soundboard soundbites, and display speaking indicators with Discord IPC.",
            local_path: None,
        },
        PluginInfo {
            id: "com_devops_terminalrunner",
            name: "Terminal Runner",
            category: "Developer",
            author: "DevOpsLinux",
            version: "v1.0.5",
            icon: "utilities-terminal-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-developer",
            description: "One-touch execution of custom bash scripts, docker container starts/stops, git pull commands, and automated SSH shortcuts.",
            local_path: None,
        },
        PluginInfo {
            id: "com_productivity_clockweather",
            name: "Clock & Weather",
            category: "Productivity",
            author: "TimeKeep",
            version: "v1.0.0",
            icon: "weather-clear-symbolic",
            thumbnail: None,
            is_file_icon: false,
            fallback_grad: "thumb-grad-productivity",
            description: "Custom digital world clocks, stopwatch timers, and live weather conditions with animated temperature badges on hardware keys.",
            local_path: None,
        },
    ];

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

    let detail_action_btn = Button::with_label("Install");
    detail_action_btn.add_css_class("btn-install");
    detail_action_btn.set_valign(gtk::Align::Center);
    detail_header_card.append(&detail_action_btn);

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

    let shots_scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Automatic)
        .vscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(145)
        .vexpand(false)
        .build();

    let shots_box = Box::new(Orientation::Horizontal, 14);
    shots_box.set_margin_bottom(6);

    let shot_pic_1 = Picture::new();
    shot_pic_1.set_can_shrink(true);
    shot_pic_1.set_content_fit(gtk::ContentFit::Cover);
    shot_pic_1.set_size_request(240, 135);
    shot_pic_1.add_css_class("store-screenshot-tile");
    shots_box.append(&shot_pic_1);

    let shot_box_2 = Box::new(Orientation::Vertical, 0);
    shot_box_2.add_css_class("store-screenshot-tile");
    shot_box_2.add_css_class("thumb-grad-streaming");
    shot_box_2.set_size_request(240, 135);
    let shot_icon_2 = Image::from_icon_name("video-display-symbolic");
    shot_icon_2.set_pixel_size(42);
    shot_icon_2.set_vexpand(true);
    shot_box_2.append(&shot_icon_2);
    shots_box.append(&shot_box_2);

    let shot_box_3 = Box::new(Orientation::Vertical, 0);
    shot_box_3.add_css_class("store-screenshot-tile");
    shot_box_3.add_css_class("thumb-grad-system");
    shot_box_3.set_size_request(240, 135);
    let shot_icon_3 = Image::from_icon_name("utilities-system-monitor-symbolic");
    shot_icon_3.set_pixel_size(42);
    shot_icon_3.set_vexpand(true);
    shot_box_3.append(&shot_icon_3);
    shots_box.append(&shot_box_3);

    shots_scrolled.set_child(Some(&shots_box));
    detail_scroll_content.append(&shots_scrolled);

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
            if let Some(inst) = pm_b.plugins.get(c.id) {
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

    let render_installed_list_cell: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let render_installed_list = {
        let list_box_inner = list_box_ref.clone();
        let pm_inner = pm_inst_refresh.clone();
        let on_cb_inner = on_inst_refresh_cb.clone();
        let refresh_hdr = refresh_header_status.clone();
        let catalog_copy = plugins_catalog.clone();
        let self_cell = render_installed_list_cell.clone();

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

                    let icon_path = p_obj.path.join("assets").join("action_launch.png");
                    if icon_path.exists() {
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

                    // Check if catalog offers an update
                    let cat_match = catalog_copy.iter().find(|c| c.id == p_id);
                    if let Some(cat_item) = cat_match {
                        let inst_ver = p_obj.version.trim_start_matches('v');
                        let cat_ver = cat_item.version.trim_start_matches('v');
                        if cat_ver > inst_ver {
                            let update_btn = Button::with_label(&format!("Update to {}", cat_item.version));
                            update_btn.add_css_class("store-updates-amber-btn");
                            update_btn.set_valign(gtk::Align::Center);
                            update_btn.set_halign(gtk::Align::End);

                            let manifest_path = p_obj.path.join("manifest.json");
                            let new_ver = cat_ver.to_string();
                            let pm_up = pm_inner.clone();
                            let on_uninst_cb = on_cb_inner.clone();
                            let refresh_hdr_click = refresh_hdr.clone();
                            let self_cell_clone = self_cell.clone();

                            update_btn.connect_clicked(move |_| {
                                if manifest_path.exists() {
                                    if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                                        if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
                                            if let Some(obj) = val.as_object_mut() {
                                                obj.insert("version".to_string(), serde_json::Value::String(new_ver.clone()));
                                                if let Ok(updated_str) = serde_json::to_string_pretty(&val) {
                                                    let _ = std::fs::write(&manifest_path, updated_str);
                                                }
                                            }
                                        }
                                    }
                                }
                                pm_up.borrow_mut().rescan_plugins();
                                on_uninst_cb();
                                refresh_hdr_click();
                                if let Some(ref r) = *self_cell_clone.borrow() {
                                    r();
                                }
                            });
                            btn_box.append(&update_btn);
                        }
                    }

                    let uninst_btn = Button::with_label("Uninstall");
                    uninst_btn.add_css_class("btn-uninstall");
                    uninst_btn.set_valign(gtk::Align::Center);
                    uninst_btn.set_halign(gtk::Align::End);

                    let pid_clone = p_id.clone();
                    let pm_uninst = pm_inner.clone();
                    let on_uninst_cb = on_cb_inner.clone();
                    let card_clone = card.clone();
                    let list_box_sub = list_box_inner.clone();
                    let refresh_hdr_uninst = refresh_hdr.clone();

                    uninst_btn.connect_clicked(move |_| {
                        let mut pm_mut = pm_uninst.borrow_mut();
                        let _ = pm_mut.uninstall_plugin(&pid_clone);
                        drop(pm_mut);
                        list_box_sub.remove(&card_clone);
                        on_uninst_cb();
                        refresh_hdr_uninst();
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
        let shot1 = shot_pic_1.clone();
        let pm_inner = pm_detail.clone();
        let on_inst_inner = on_installed_detail.clone();

        move |plugin: PluginInfo| {
            name_lbl.set_text(plugin.name);
            meta_lbl.set_text(&format!("by {} • {} • {}", plugin.author, plugin.version, plugin.category));
            desc_lbl.set_text(plugin.description);
            win_title_inner.set_title(Some(plugin.name));

            // Load Icon
            if plugin.is_file_icon {
                let full_path = format!("/home/oscar/.local/share/ballista/plugins/com_oparada_AppLauncher/assets/{}", plugin.icon);
                if std::path::Path::new(&full_path).exists() {
                    icon_img.set_from_file(Some(&full_path));
                    icon_img.set_pixel_size(64);
                } else {
                    icon_img.set_icon_name(Some("system-run-symbolic"));
                    icon_img.set_pixel_size(64);
                }
            } else {
                icon_img.set_icon_name(Some(plugin.icon));
                icon_img.set_pixel_size(64);
            }

            // Load Screenshot 1
            if let Some(thumb) = plugin.thumbnail {
                if std::path::Path::new(thumb).exists() {
                    if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(thumb, 480, 270, false) {
                        let texture = gtk::gdk::Texture::for_pixbuf(&pixbuf);
                        shot1.set_paintable(Some(&texture));
                    } else {
                        shot1.set_filename(Some(thumb));
                    }
                }
            } else {
                shot1.set_paintable(Option::<&gtk::gdk::Paintable>::None);
            }

            // Check if installed
            let is_installed = pm_inner.borrow().plugins.contains_key(plugin.id);
            action_btn.remove_css_class("btn-install");
            action_btn.remove_css_class("btn-installed");

            if is_installed {
                action_btn.set_label("Installed");
                action_btn.add_css_class("btn-installed");
                action_btn.set_sensitive(false);
            } else {
                action_btn.set_label("Install");
                action_btn.add_css_class("btn-install");
                action_btn.set_sensitive(true);

                let btn_clone = action_btn.clone();
                let pm_action = pm_inner.clone();
                let on_inst_action = on_inst_inner.clone();
                let local_path = plugin.local_path;

                action_btn.connect_clicked(move |_| {
                    if let Some(path_str) = local_path {
                        let path = PathBuf::from(path_str);
                        if path.exists() {
                            let res = pm_action.borrow_mut().install_from_dir(&path);
                            if res.is_ok() {
                                btn_clone.set_label("Installed");
                                btn_clone.remove_css_class("btn-install");
                                btn_clone.add_css_class("btn-installed");
                                btn_clone.set_sensitive(false);
                                on_inst_action();
                            }
                        }
                    }
                });
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
                    let is_installed = pm_cat.borrow().plugins.contains_key(item.id);
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

    let mut chunk = Vec::new();
    for (i, item) in plugins_catalog.iter().enumerate() {
        let card = create_spotlight_card(item, open_plugin_detail.clone());
        chunk.push(card);

        if chunk.len() == 3 || i == plugins_catalog.len() - 1 {
            let slide_box = Box::new(Orientation::Horizontal, 12);
            slide_box.set_hexpand(true);
            slide_box.set_homogeneous(true);
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
    // 3. 3-Column x 2-Row Category Grid
    // =========================================================================
    let cat_title = Label::new(Some("Categories"));
    cat_title.add_css_class("store-section-title");
    cat_title.set_xalign(0.0);
    scroll_content.append(&cat_title);

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
    scroll_content.append(&cat_grid);

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
    scroll_content.append(&pop_header_box);

    let list_box = Box::new(Orientation::Vertical, 10);
    let mut catalog_items: Vec<(Box, &'static str, &'static str)> = Vec::new();

    for item in plugins_catalog.iter() {
        let is_installed = plugin_manager.borrow().plugins.contains_key(item.id);
        let card = create_catalog_card(item, is_installed, open_plugin_detail.clone(), plugin_manager.clone(), on_plugin_installed.clone(), refresh_header_status.clone());
        list_box.append(&card);
        catalog_items.push((card, item.name, item.category));
    }

    let catalog_items_rc = Rc::new(catalog_items);
    let active_cat = Rc::new(RefCell::new(Option::<String>::None));

    let filter_catalog = {
        let items = catalog_items_rc.clone();
        let search_ent = search_entry.clone();
        let active_cat_ref = active_cat.clone();
        move || {
            let query = search_ent.text().to_string().to_lowercase();
            let cur_cat = active_cat_ref.borrow().clone();

            for (card, title, cats) in items.iter() {
                let matches_cat = match &cur_cat {
                    Some(c) => cats.contains(c.as_str()),
                    None => true,
                };
                let matches_search = query.is_empty() || title.to_lowercase().contains(&query);
                card.set_visible(matches_cat && matches_search);
            }
        }
    };

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

    scroll_content.append(&list_box);
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

    // Handle "Install Local Plugin..." dialog
    let pm_local = plugin_manager.clone();
    let on_installed_local = on_plugin_installed.clone();
    let window_ref = window.clone();
    install_local_btn.connect_clicked(move |_| {
        let dialog = FileDialog::builder()
            .title("Select Plugin Folder (containing manifest.json)")
            .build();

        let pm_inner = pm_local.clone();
        let cb_inner = on_installed_local.clone();
        dialog.select_folder(Some(&window_ref), Option::<&gio::Cancellable>::None, move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    let res = pm_inner.borrow_mut().install_from_dir(&path);
                    if res.is_ok() {
                        cb_inner();
                    }
                }
            }
        });
    });

    window
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
    card.set_overflow(gtk::Overflow::Hidden);
    card.set_cursor_from_name(Some("pointer"));
    card.set_hexpand(true);
    card.set_size_request(230, 125);

    // Top Hero Artwork Thumbnail - Occupies full card
    let thumb_pic = Picture::new();
    thumb_pic.set_can_shrink(true);
    thumb_pic.set_content_fit(gtk::ContentFit::Cover);
    thumb_pic.set_hexpand(true);
    thumb_pic.set_vexpand(true);
    thumb_pic.set_size_request(230, 125);
    thumb_pic.add_css_class("store-spotlight-thumbnail");

    let mut has_real_thumb = false;
    if let Some(path) = item.thumbnail {
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
        fallback_box.add_css_class(item.fallback_grad);
        fallback_box.set_hexpand(true);
        fallback_box.set_vexpand(true);
        fallback_box.set_size_request(230, 125);

        let icon_preview = Image::from_icon_name(if item.is_file_icon { "system-run-symbolic" } else { item.icon });
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

    let escaped_name = glib::markup_escape_text(item.name);
    let escaped_desc = glib::markup_escape_text(item.description);
    let row = ActionRow::builder()
        .title(escaped_name.as_str())
        .subtitle(escaped_desc.as_str())
        .build();

    let img = if item.is_file_icon {
        let full_path = format!("/home/oscar/.local/share/ballista/plugins/com_oparada_AppLauncher/assets/{}", item.icon);
        if std::path::Path::new(&full_path).exists() {
            let img = Image::from_file(&full_path);
            img.set_pixel_size(32);
            img
        } else {
            let img = Image::from_icon_name("system-run-symbolic");
            img.set_pixel_size(32);
            img
        }
    } else {
        let img = Image::from_icon_name(item.icon);
        img.set_pixel_size(32);
        img
    };
    row.add_prefix(&img);

    let btn = Button::with_label(if is_installed { "Installed" } else { "Install" });
    btn.set_valign(gtk::Align::Center);
    btn.set_halign(gtk::Align::End);

    if is_installed {
        let pm_b = plugin_manager.borrow();
        let mut has_update = false;
        let mut manifest_path_opt = None;
        if let Some(inst) = pm_b.plugins.get(item.id) {
            let inst_ver = inst.version.trim_start_matches('v');
            let cat_ver = item.version.trim_start_matches('v');
            if cat_ver > inst_ver {
                has_update = true;
                manifest_path_opt = Some(inst.path.join("manifest.json"));
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
            let new_ver = item.version.trim_start_matches('v').to_string();

            btn.connect_clicked(move |_| {
                if let Some(ref m_path) = manifest_path_opt {
                    if m_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(m_path) {
                            if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
                                if let Some(obj) = val.as_object_mut() {
                                    obj.insert("version".to_string(), serde_json::Value::String(new_ver.clone()));
                                    if let Ok(updated_str) = serde_json::to_string_pretty(&val) {
                                        let _ = std::fs::write(m_path, updated_str);
                                    }
                                }
                            }
                        }
                    }
                }
                pm_update.borrow_mut().rescan_plugins();
                btn_clone.set_label("Installed");
                btn_clone.remove_css_class("store-updates-amber-btn");
                btn_clone.add_css_class("btn-installed");
                btn_clone.set_sensitive(false);
                on_inst_action();
                on_ref_hdr();
            });
        } else {
            btn.add_css_class("btn-installed");
            btn.set_sensitive(false);
        }
    } else {
        btn.add_css_class("btn-install");
        if let Some(path_str) = item.local_path {
            let btn_clone = btn.clone();
            let pm_inner = plugin_manager.clone();
            let on_inst_inner = on_plugin_installed.clone();
            let on_ref_hdr = on_refresh_header.clone();
            btn.connect_clicked(move |_| {
                let path = PathBuf::from(path_str);
                if path.exists() {
                    let res = pm_inner.borrow_mut().install_from_dir(&path);
                    if res.is_ok() {
                        btn_clone.set_label("Installed");
                        btn_clone.remove_css_class("btn-install");
                        btn_clone.add_css_class("btn-installed");
                        btn_clone.set_sensitive(false);
                        on_inst_inner();
                        on_ref_hdr();
                    }
                }
            });
        }
    }
    row.add_suffix(&btn);
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
