use crate::core::{ActionSettingType, InstalledApp, PluginManager};
use crate::models::{KeyAction, TextVerticalAlign};
use adw::prelude::*;
use gtk::gdk::RGBA;
use gtk::{
    Box, Button, CheckButton, ColorDialog, ColorDialogButton, DropDown, Entry, FontDialog,
    FontDialogButton, Label, Orientation, Overlay, Picture, SpinButton, Stack, StringList, Switch,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectorTarget {
    Key(usize),
    Dial(usize),
    Touchscreen,
}

pub struct PropertyInspector {
    pub container: Box,
    pub title_label: Label,
    pub plugin_settings_button: Button,
    pub delete_button: Button,
    pub stack: Stack,
    pub tab_appearance_btn: Button,
    pub tab_action_btn: Button,
    pub title_entry: Entry,
    pub title_toggle_btn: Button,
    pub font_dialog_button: FontDialogButton,
    pub font_size_spin: SpinButton,
    pub btn_color_caption: Label,
    pub btn_color_picker: ColorDialogButton,
    pub btn_color_reset: Button,
    pub text_color_picker: ColorDialogButton,
    pub icon_caption: Label,
    pub browse_icon_btn: Button,
    pub clear_icon_btn: Button,
    pub icon_scale_spin: SpinButton,
    pub pos_top_btn: Button,
    pub pos_mid_btn: Button,
    pub pos_bot_btn: Button,
    pub title_row: Box,
    pub font_row: Box,
    pub btn_color_row: Box,
    pub text_color_row: Box,
    pub pos_row: Box,
    pub icon_row: Box,
    pub scale_row: Box,
    pub scale_caption: Label,
    pub app_row: Box,
    pub args_row: Box,
    pub args_entry: Entry,
    pub empty_action_label: Label,
    pub action_info_box: Box,
    pub action_info_name: Label,
    pub action_info_desc: Label,
    pub dynamic_settings_box: Box,
    pub multi_action_box: Box,
    pub multi_active_seq: Rc<Cell<usize>>,
    pub on_multi_steps_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String, Vec<crate::native_sdk::MultiActionStep>)>>>>,
    pub swipe_row: Box,
    pub swipe_dropdown: DropDown,
    pub app_dropdown: DropDown,
    pub preview_tile: Overlay,
    pub preview_sizer: Box,
    pub preview_native_da: gtk::DrawingArea,
    pub preview_inner_label: Label,
    pub preview_top_label: Label,
    pub preview_center_label: Label,
    pub preview_icon: Picture,
    pub left_preview_col: Box,
    pub state_dots_box: Box,
    pub radio_state_0: CheckButton,
    pub radio_state_1: CheckButton,
    pub selected_state_idx: Rc<Cell<usize>>,
    pub is_updating: Rc<Cell<bool>>,
    pub current_target: Rc<RefCell<InspectorTarget>>,
    pub current_key_idx: Rc<RefCell<Option<usize>>>,
    pub installed_apps: Rc<RefCell<Vec<InstalledApp>>>,
    pub active_preview_anim: Rc<RefCell<Option<glib::SourceId>>>,
    pub current_plugin_id: Rc<RefCell<Option<String>>>,
    pub current_plugin_name: Rc<RefCell<Option<String>>>,
    pub on_open_plugin_settings: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String)>>>>,
    pub plugin_manager: Rc<RefCell<Option<PluginManager>>>,
    pub native_host: Rc<RefCell<Option<crate::native_sdk::NativePluginHost>>>,
    pub active_device_key: Rc<RefCell<String>>,
    pub on_parameter_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String)>>>>,
    pub on_config_row_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, usize, Option<usize>, Option<bool>, Option<String>, Option<String>, Option<f64>)>>>>,
    pub on_config_button_clicked: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, usize)>>>>,
    pub rendered_live_slot: Rc<RefCell<Option<String>>>,
    pub rendered_live_rows: Rc<RefCell<Vec<crate::core::plugin_bridge::BridgeConfigRow>>>,
    pub rendered_live_widgets: Rc<RefCell<Vec<gtk::Widget>>>,
    pub is_live_syncing: Rc<Cell<bool>>,
}

pub fn rgba_to_hex(rgba: &RGBA) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (rgba.red() * 255.0).round() as u8,
        (rgba.green() * 255.0).round() as u8,
        (rgba.blue() * 255.0).round() as u8
    )
}

pub fn hex_to_rgba(hex: &str) -> RGBA {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).unwrap_or(25);
        let g = u8::from_str_radix(&clean[2..4], 16).unwrap_or(25);
        let b = u8::from_str_radix(&clean[4..6], 16).unwrap_or(32);
        RGBA::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
    } else {
        RGBA::new(0.098, 0.098, 0.125, 1.0)
    }
}

impl PropertyInspector {
    pub fn new() -> Self {
        let container = Box::new(Orientation::Vertical, 10);
        container.add_css_class("property-inspector");

        // Top Header
        let header_box = Box::new(Orientation::Horizontal, 8);
        let title_label = Label::new(Some("Select a key to configure"));
        title_label.add_css_class("inspector-header");
        title_label.set_hexpand(true);
        title_label.set_xalign(0.0);

        let current_plugin_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let current_plugin_name: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let on_open_plugin_settings: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String)>>>> = Rc::new(RefCell::new(None));

        let plugin_settings_button = Button::from_icon_name("preferences-system-symbolic");
        plugin_settings_button.set_tooltip_text(Some("Plugin Settings"));
        plugin_settings_button.add_css_class("flat");
        plugin_settings_button.set_visible(false);

        let cur_pid_btn = current_plugin_id.clone();
        let cur_pname_btn = current_plugin_name.clone();
        let on_open_cb_btn = on_open_plugin_settings.clone();
        plugin_settings_button.connect_clicked(move |_| {
            if let Some(ref pid) = *cur_pid_btn.borrow() {
                let pname = cur_pname_btn.borrow().clone().unwrap_or_else(|| "Plugin".into());
                if let Some(ref cb) = *on_open_cb_btn.borrow() {
                    cb(pid.clone(), pname);
                }
            }
        });

        let delete_button = Button::from_icon_name("user-trash-symbolic");
        delete_button.set_tooltip_text(Some("Clear Key Action"));
        delete_button.add_css_class("flat");
        delete_button.set_sensitive(false);

        header_box.append(&title_label);
        header_box.append(&plugin_settings_button);
        header_box.append(&delete_button);
        container.append(&header_box);

        // Segmented Tab Switcher: Centered horizontally across the entire inspector
        let tab_box_wrapper = Box::new(Orientation::Horizontal, 0);
        tab_box_wrapper.set_halign(gtk::Align::Center);
        tab_box_wrapper.set_margin_top(2);
        tab_box_wrapper.set_margin_bottom(8);

        let tab_box = Box::new(Orientation::Horizontal, 4);
        tab_box.add_css_class("inspector-tab-pill");
        tab_box.set_halign(gtk::Align::Center);

        let tab_appearance_btn = Button::with_label("Appearance");
        tab_appearance_btn.add_css_class("flat");

        let tab_action_btn = Button::with_label("Action Settings");
        tab_action_btn.add_css_class("flat");

        tab_box.append(&tab_appearance_btn);
        tab_box.append(&tab_action_btn);
        tab_box_wrapper.append(&tab_box);
        container.append(&tab_box_wrapper);

        // Center Content Box: Keeps preview and settings grouped and centered horizontally on resize
        let center_content = Box::new(Orientation::Horizontal, 28);
        center_content.set_halign(gtk::Align::Center);
        center_content.set_valign(gtk::Align::Start);
        center_content.set_margin_top(2);
        center_content.set_margin_bottom(12);

        // Left Preview Column
        let left_preview_col = Box::new(Orientation::Vertical, 8);
        left_preview_col.set_halign(gtk::Align::Start);
        left_preview_col.set_valign(gtk::Align::Start);
        left_preview_col.set_vexpand(false);

        let preview_tile = Overlay::new();
        preview_tile.add_css_class("inspector-key-tile");
        preview_tile.set_size_request(80, 80);
        preview_tile.set_overflow(gtk::Overflow::Hidden);
        preview_tile.set_halign(gtk::Align::Center);
        preview_tile.set_valign(gtk::Align::Center);
        preview_tile.set_vexpand(false);
        preview_tile.set_hexpand(false);

        let preview_sizer = Box::new(Orientation::Vertical, 0);
        preview_sizer.set_size_request(78, 78);
        preview_sizer.set_hexpand(false);
        preview_sizer.set_vexpand(false);
        preview_sizer.set_can_target(false);

        let native_host: Rc<RefCell<Option<crate::native_sdk::NativePluginHost>>> =
            Rc::new(RefCell::new(None));
        let active_device_key: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
        let current_target_rc: Rc<RefCell<InspectorTarget>> =
            Rc::new(RefCell::new(InspectorTarget::Key(0)));

        let preview_native_da = gtk::DrawingArea::new();
        preview_native_da.set_hexpand(true);
        preview_native_da.set_vexpand(true);
        preview_native_da.set_halign(gtk::Align::Fill);
        preview_native_da.set_valign(gtk::Align::Fill);
        preview_native_da.set_visible(false);
        preview_native_da.set_can_target(false);
        {
            let nh_prev = native_host.clone();
            let dkey_prev = active_device_key.clone();
            let tgt_prev = current_target_rc.clone();
            preview_native_da.set_draw_func(move |_area, cr, width, height| {
                if width <= 0 || height <= 0 {
                    return;
                }
                let slot = match *tgt_prev.borrow() {
                    InspectorTarget::Key(k) => format!("key_{k}"),
                    InspectorTarget::Dial(d) => format!("dial_{d}"),
                    InspectorTarget::Touchscreen => "touchscreen".to_string(),
                };
                if let Some(ref host) = *nh_prev.borrow() {
                    let dkey = dkey_prev.borrow();
                    let _ = host.render_slot_visual(&dkey, &slot, cr, width as f64, height as f64);
                }
            });
        }

        let preview_icon = Picture::new();
        preview_icon.set_can_shrink(true);
        preview_icon.set_content_fit(gtk::ContentFit::Contain);
        preview_icon.set_halign(gtk::Align::Fill);
        preview_icon.set_valign(gtk::Align::Fill);
        preview_icon.set_hexpand(true);
        preview_icon.set_vexpand(true);
        preview_icon.set_visible(false);
        preview_icon.set_can_target(false);

        let make_prev_label = |valign: gtk::Align| {
            let lbl = Label::new(Some(""));
            lbl.set_valign(valign);
            lbl.set_halign(gtk::Align::Fill);
            lbl.set_xalign(0.5);
            lbl.set_wrap(false);
            lbl.set_single_line_mode(true);
            lbl.set_ellipsize(pango::EllipsizeMode::End);
            lbl.set_margin_start(4);
            lbl.set_margin_end(4);
            lbl.set_margin_top(3);
            lbl.set_margin_bottom(3);
            lbl.add_css_class("deck-key-label");
            lbl.set_visible(false);
            lbl
        };

        let preview_inner_label = make_prev_label(gtk::Align::End);
        let preview_top_label = make_prev_label(gtk::Align::Start);
        let preview_center_label = make_prev_label(gtk::Align::Center);

        preview_tile.set_child(Some(&preview_sizer));
        preview_tile.add_overlay(&preview_native_da);
        preview_tile.add_overlay(&preview_icon);
        preview_tile.add_overlay(&preview_top_label);
        preview_tile.add_overlay(&preview_center_label);
        preview_tile.add_overlay(&preview_inner_label);

        // State dots
        let state_dots_box = Box::new(Orientation::Horizontal, 6);
        state_dots_box.set_halign(gtk::Align::Center);
        let radio_state_0 = CheckButton::builder().active(true).build();
        let radio_state_1 = CheckButton::builder().group(&radio_state_0).build();
        state_dots_box.append(&radio_state_0);
        state_dots_box.append(&radio_state_1);

        left_preview_col.append(&preview_tile);
        left_preview_col.append(&state_dots_box);
        center_content.append(&left_preview_col);

        // Settings Stack Column
        let stack = Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_halign(gtk::Align::Start);
        stack.set_valign(gtk::Align::Start);

        // ===================== Tab 1: Appearance Page =====================
        let appearance_box = Box::new(Orientation::Vertical, 8);
        appearance_box.set_halign(gtk::Align::Start);

        // 1. Title row
        let title_row = Box::new(Orientation::Horizontal, 10);
        let title_caption = Label::new(Some("Title:"));
        title_caption.set_size_request(100, -1);
        title_caption.set_xalign(0.0);

        let title_entry = Entry::new();
        title_entry.set_placeholder_text(Some("Key title text..."));
        title_entry.set_size_request(240, -1);

        let title_toggle_btn = Button::from_icon_name("view-reveal-symbolic");
        title_toggle_btn.add_css_class("flat");
        title_toggle_btn.set_tooltip_text(Some("Hide Title on Key"));

        title_row.append(&title_caption);
        title_row.append(&title_entry);
        title_row.append(&title_toggle_btn);
        appearance_box.append(&title_row);

        // 2. Typography Row: Font family & Font size
        let font_row = Box::new(Orientation::Horizontal, 10);
        let font_caption = Label::new(Some("Font & Size:"));
        font_caption.set_size_request(100, -1);
        font_caption.set_xalign(0.0);

        let font_dialog = FontDialog::builder().title("Select Key Font").build();
        let font_dialog_button = FontDialogButton::new(Some(font_dialog));
        font_dialog_button.set_size_request(190, -1);

        let font_size_spin = SpinButton::with_range(6.0, 48.0, 1.0);
        font_size_spin.set_value(11.0);
        font_size_spin.set_tooltip_text(Some("Font Size in pt"));

        font_row.append(&font_caption);
        font_row.append(&font_dialog_button);
        font_row.append(&font_size_spin);
        appearance_box.append(&font_row);

        // 3. Button Color Row (Pre-configured chips removed per user request)
        let btn_color_row = Box::new(Orientation::Horizontal, 10);
        let btn_color_caption = Label::new(Some("Button Color:"));
        btn_color_caption.set_size_request(100, -1);
        btn_color_caption.set_xalign(0.0);

        let bg_dialog = ColorDialog::builder().with_alpha(false).build();
        let btn_color_picker = ColorDialogButton::new(Some(bg_dialog));
        btn_color_picker.set_rgba(&RGBA::new(0.098, 0.098, 0.125, 1.0));

        let btn_color_reset = Button::with_label("Reset Color");
        btn_color_reset.add_css_class("flat");
        btn_color_reset.set_tooltip_text(Some("Restore default key color"));

        btn_color_row.append(&btn_color_caption);
        btn_color_row.append(&btn_color_picker);
        btn_color_row.append(&btn_color_reset);
        appearance_box.append(&btn_color_row);

        // 4. Text Color Row
        let text_color_row = Box::new(Orientation::Horizontal, 10);
        let text_color_caption = Label::new(Some("Text Color:"));
        text_color_caption.set_size_request(100, -1);
        text_color_caption.set_xalign(0.0);

        let fg_dialog = ColorDialog::builder().with_alpha(false).build();
        let text_color_picker = ColorDialogButton::new(Some(fg_dialog));
        text_color_picker.set_rgba(&RGBA::new(1.0, 1.0, 1.0, 1.0));

        text_color_row.append(&text_color_caption);
        text_color_row.append(&text_color_picker);
        appearance_box.append(&text_color_row);

        // 5. Position Row
        let pos_row = Box::new(Orientation::Horizontal, 10);
        let pos_caption = Label::new(Some("Position:"));
        pos_caption.set_size_request(100, -1);
        pos_caption.set_xalign(0.0);
        let pos_top_btn = Button::with_label("Top");
        let pos_mid_btn = Button::with_label("Center");
        let pos_bot_btn = Button::with_label("Bottom");
        pos_bot_btn.add_css_class("suggested-action");

        pos_row.append(&pos_caption);
        pos_row.append(&pos_top_btn);
        pos_row.append(&pos_mid_btn);
        pos_row.append(&pos_bot_btn);
        appearance_box.append(&pos_row);

        // 6. Icon Row
        let icon_row = Box::new(Orientation::Horizontal, 10);
        let icon_caption = Label::new(Some("Icon:"));
        icon_caption.set_size_request(100, -1);
        icon_caption.set_xalign(0.0);
        let browse_icon_btn = Button::with_label("Browse Icon...");
        let clear_icon_btn = Button::with_label("Remove Icon");
        clear_icon_btn.add_css_class("flat");

        icon_row.append(&icon_caption);
        icon_row.append(&browse_icon_btn);
        icon_row.append(&clear_icon_btn);
        appearance_box.append(&icon_row);

        // 7. Icon Scale Row (20% - 100%)
        let scale_row = Box::new(Orientation::Horizontal, 10);
        let scale_caption = Label::new(Some("Icon Scale:"));
        scale_caption.set_size_request(100, -1);
        scale_caption.set_xalign(0.0);

        let icon_scale_spin = SpinButton::with_range(20.0, 100.0, 5.0);
        icon_scale_spin.set_value(100.0);
        icon_scale_spin.set_tooltip_text(Some("Icon / Wallpaper scale percentage (20% to 100%)"));

        scale_row.append(&scale_caption);
        scale_row.append(&icon_scale_spin);
        appearance_box.append(&scale_row);

        stack.add_named(&appearance_box, Some("appearance"));

        // ===================== Tab 2: Action Settings Page =====================
        let action_box = Box::new(Orientation::Vertical, 8);
        action_box.set_halign(gtk::Align::Start);

        let empty_action_label = Label::new(Some("No action assigned to this key.\nDrag and drop an action from the library on the right to configure it."));
        empty_action_label.set_wrap(true);
        empty_action_label.set_justify(gtk::Justification::Center);
        empty_action_label.set_halign(gtk::Align::Center);
        empty_action_label.set_margin_top(14);
        empty_action_label.set_margin_bottom(14);
        empty_action_label.add_css_class("dim-label");
        action_box.append(&empty_action_label);

        // Target Application Dropdown for AppLauncher
        let app_row = Box::new(Orientation::Horizontal, 10);
        app_row.set_visible(false);
        let app_caption = Label::new(Some("Application:"));
        app_caption.set_size_request(100, -1);
        app_caption.set_xalign(0.0);

        let dummy_list = StringList::new(&["Loading Applications..."]);
        let app_dropdown = DropDown::new(Some(dummy_list), Option::<gtk::Expression>::None);
        app_dropdown.set_size_request(280, -1);

        app_row.append(&app_caption);
        app_row.append(&app_dropdown);
        action_box.append(&app_row);

        // Parameter 2: Extra Launch arguments
        let args_row = Box::new(Orientation::Horizontal, 10);
        args_row.set_visible(false);
        let args_caption = Label::new(Some("Arguments:"));
        args_caption.set_size_request(100, -1);
        args_caption.set_xalign(0.0);

        let args_entry = Entry::new();
        args_entry.set_placeholder_text(Some("Optional command line flags..."));
        args_entry.set_size_request(280, -1);

        args_row.append(&args_caption);
        args_row.append(&args_entry);
        action_box.append(&args_row);

        // Action info row for actions with no extra settings (unified flat layout matching Appearance)
        let action_info_box = Box::new(Orientation::Vertical, 10);
        action_info_box.set_visible(false);
        action_info_box.set_halign(gtk::Align::Start);

        let info_name_row = Box::new(Orientation::Horizontal, 10);
        let info_name_caption = Label::new(Some("Action:"));
        info_name_caption.set_size_request(100, -1);
        info_name_caption.set_xalign(0.0);

        let action_info_name = Label::new(None);
        action_info_name.set_xalign(0.0);
        action_info_name.add_css_class("heading");

        info_name_row.append(&info_name_caption);
        info_name_row.append(&action_info_name);
        action_info_box.append(&info_name_row);

        let info_desc_row = Box::new(Orientation::Horizontal, 10);
        let info_desc_caption = Label::new(Some("Description:"));
        info_desc_caption.set_size_request(100, -1);
        info_desc_caption.set_xalign(0.0);

        let action_info_desc = Label::new(None);
        action_info_desc.set_xalign(0.0);
        action_info_desc.add_css_class("dim-label");

        info_desc_row.append(&info_desc_caption);
        info_desc_row.append(&action_info_desc);
        action_info_box.append(&info_desc_row);

        action_box.append(&action_info_box);

        // Dynamic Action Settings Container (unified flat layout matching Appearance)
        let dynamic_settings_box = Box::new(Orientation::Vertical, 10);
        dynamic_settings_box.set_visible(false);
        dynamic_settings_box.set_halign(gtk::Align::Start);
        action_box.append(&dynamic_settings_box);

        // Native Multi-Action Step Sequence Editor Container
        let multi_action_box = Box::new(Orientation::Vertical, 8);
        multi_action_box.set_visible(false);
        multi_action_box.set_halign(gtk::Align::Start);
        multi_action_box.set_margin_top(6);
        action_box.append(&multi_action_box);

        let multi_active_seq = Rc::new(Cell::new(0usize));
        let on_multi_steps_changed: Rc<
            RefCell<
                Option<
                    std::boxed::Box<dyn Fn(String, String, Vec<crate::native_sdk::MultiActionStep>)>,
                >,
            >,
        > = Rc::new(RefCell::new(None));

        let on_parameter_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String)>>>> = Rc::new(RefCell::new(None));
        let on_config_row_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, usize, Option<usize>, Option<bool>, Option<String>, Option<String>, Option<f64>)>>>> = Rc::new(RefCell::new(None));
        let on_config_button_clicked: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, usize)>>>> = Rc::new(RefCell::new(None));

        // Parameter 3: Touch Strip Swipe Setting (Official Stream Deck + model)
        let swipe_row = Box::new(Orientation::Vertical, 6);
        swipe_row.set_visible(false);

        let swipe_header_row = Box::new(Orientation::Horizontal, 10);
        let swipe_caption = Label::new(Some("Swipe Action:"));
        swipe_caption.set_size_request(100, -1);
        swipe_caption.set_xalign(0.0);

        let swipe_list = StringList::new(&["Switch Pages (Default)", "Disabled"]);
        let swipe_dropdown = DropDown::new(Some(swipe_list), Option::<gtk::Expression>::None);
        swipe_dropdown.set_size_request(280, -1);
        swipe_header_row.append(&swipe_caption);
        swipe_header_row.append(&swipe_dropdown);

        let swipe_info_label = Label::new(Some("Swipe Left: Previous Page | Swipe Right: Next Page"));
        swipe_info_label.set_xalign(0.0);
        swipe_info_label.set_margin_start(110);

        swipe_row.append(&swipe_header_row);
        swipe_row.append(&swipe_info_label);
        action_box.append(&swipe_row);

        stack.add_named(&action_box, Some("action_settings"));

        center_content.append(&stack);
        container.append(&center_content);

        // Connect Tab switching
        let stack_clone1 = stack.clone();
        let tab_app_clone = tab_appearance_btn.clone();
        let tab_act_clone = tab_action_btn.clone();
        tab_appearance_btn.connect_clicked(move |_| {
            stack_clone1.set_visible_child_name("appearance");
            tab_app_clone.add_css_class("suggested-action");
            tab_act_clone.remove_css_class("suggested-action");
        });

        let stack_clone2 = stack.clone();
        let tab_app_clone2 = tab_appearance_btn.clone();
        let tab_act_clone2 = tab_action_btn.clone();
        tab_action_btn.connect_clicked(move |_| {
            stack_clone2.set_visible_child_name("action_settings");
            tab_act_clone2.add_css_class("suggested-action");
            tab_app_clone2.remove_css_class("suggested-action");
        });

        // Set default tab to Action Settings
        stack.set_visible_child_name("action_settings");
        tab_action_btn.add_css_class("suggested-action");

        Self {
            container,
            title_label,
            plugin_settings_button,
            delete_button,
            stack,
            tab_appearance_btn,
            tab_action_btn,
            title_entry,
            title_toggle_btn,
            font_dialog_button,
            font_size_spin,
            btn_color_caption,
            btn_color_picker,
            btn_color_reset,
            text_color_picker,
            icon_caption,
            browse_icon_btn,
            clear_icon_btn,
            icon_scale_spin,
            pos_top_btn,
            pos_mid_btn,
            pos_bot_btn,
            title_row,
            font_row,
            btn_color_row,
            text_color_row,
            pos_row,
            icon_row,
            scale_row,
            scale_caption,
            app_row,
            args_row,
            args_entry,
            empty_action_label,
            action_info_box,
            action_info_name,
            action_info_desc,
            dynamic_settings_box,
            multi_action_box,
            multi_active_seq,
            on_multi_steps_changed,
            swipe_row,
            swipe_dropdown,
            app_dropdown,
            preview_tile,
            preview_sizer,
            preview_native_da,
            preview_inner_label,
            preview_top_label,
            preview_center_label,
            preview_icon,
            left_preview_col,
            state_dots_box,
            radio_state_0,
            radio_state_1,
            selected_state_idx: Rc::new(Cell::new(0)),
            is_updating: Rc::new(Cell::new(false)),
            current_target: current_target_rc,
            current_key_idx: Rc::new(RefCell::new(Some(0))),
            installed_apps: Rc::new(RefCell::new(Vec::new())),
            active_preview_anim: Rc::new(RefCell::new(None)),
            current_plugin_id,
            current_plugin_name,
            on_open_plugin_settings,
            plugin_manager: Rc::new(RefCell::new(None)),
            native_host,
            active_device_key,
            on_parameter_changed,
            on_config_row_changed,
            on_config_button_clicked,
            rendered_live_slot: Rc::new(RefCell::new(None)),
            rendered_live_rows: Rc::new(RefCell::new(Vec::new())),
            rendered_live_widgets: Rc::new(RefCell::new(Vec::new())),
            is_live_syncing: Rc::new(Cell::new(false)),
        }
    }

    pub fn set_target(&self, target: InspectorTarget) {
        *self.current_target.borrow_mut() = target;
        self.preview_tile.remove_css_class("inspector-key-tile");
        self.preview_tile.remove_css_class("inspector-dial-tile");
        self.preview_tile.remove_css_class("inspector-touch-tile");
        match target {
            InspectorTarget::Key(k) => {
                *self.current_key_idx.borrow_mut() = Some(k);
                self.title_label.set_text(&format!("Stream Deck Key {}", k + 1));
                self.tab_appearance_btn.set_label("Appearance");
                self.tab_action_btn.set_label("Action Settings");
                self.left_preview_col.set_visible(true);
                self.title_row.set_visible(true);
                self.font_row.set_visible(true);
                self.btn_color_row.set_visible(true);
                self.btn_color_caption.set_text("Button Color:");
                self.text_color_row.set_visible(true);
                self.pos_row.set_visible(true);
                self.icon_row.set_visible(true);
                self.icon_caption.set_text("Icon:");
                self.browse_icon_btn.set_label("Browse Icon...");
                self.clear_icon_btn.set_label("Remove Icon");
                self.scale_row.set_visible(true);
                self.scale_caption.set_text("Icon Scale:");
                self.swipe_row.set_visible(false);
                self.state_dots_box.set_visible(false);
                self.preview_inner_label.set_visible(true);
                self.preview_tile.add_css_class("inspector-key-tile");
                self.preview_tile.set_size_request(80, 80);
                self.preview_sizer.set_size_request(78, 78);
            }
            InspectorTarget::Dial(d) => {
                *self.current_key_idx.borrow_mut() = None;
                self.title_label.set_text(&format!("Stream Deck + Dial {} & Card {}", d + 1, d + 1));
                self.tab_appearance_btn.set_label("Appearance");
                self.tab_action_btn.set_label("Action Settings");
                self.left_preview_col.set_visible(true);
                self.title_row.set_visible(true);
                self.font_row.set_visible(true);
                self.btn_color_row.set_visible(true);
                self.btn_color_caption.set_text("Card Color:");
                self.text_color_row.set_visible(true);
                self.pos_row.set_visible(true);
                self.icon_row.set_visible(true);
                self.icon_caption.set_text("Card Icon:");
                self.browse_icon_btn.set_label("Browse Icon...");
                self.clear_icon_btn.set_label("Remove Icon");
                self.scale_row.set_visible(true);
                self.scale_caption.set_text("Card Icon Scale:");
                self.swipe_row.set_visible(false);
                self.state_dots_box.set_visible(false);
                self.preview_inner_label.set_visible(true);
                self.preview_tile.add_css_class("inspector-dial-tile");
                self.preview_tile.set_size_request(160, 80);
                self.preview_sizer.set_size_request(158, 78);
            }
            InspectorTarget::Touchscreen => {
                *self.current_key_idx.borrow_mut() = None;
                self.title_label.set_text("Stream Deck + Touch Strip");
                self.tab_appearance_btn.set_label("Wallpaper");
                self.tab_action_btn.set_label("Swipe Action");
                // Touch Strip has no preview tile; strip is already displayed directly above on canvas
                self.left_preview_col.set_visible(false);
                // Strictly 2 settings for Touch Strip:
                self.title_row.set_visible(false);
                self.font_row.set_visible(false);
                self.text_color_row.set_visible(false);
                self.pos_row.set_visible(false);
                self.btn_color_row.set_visible(true);
                self.btn_color_caption.set_text("Background Color:");
                self.icon_row.set_visible(true);
                self.icon_caption.set_text("Wallpaper Image:");
                self.browse_icon_btn.set_label("Browse Wallpaper...");
                self.clear_icon_btn.set_label("Remove Wallpaper");
                // Wallpaper covers the full 800x100 strip; scale stepper is hidden
                self.scale_row.set_visible(false);
                self.app_row.set_visible(false);
                self.args_row.set_visible(false);
                self.swipe_row.set_visible(true);
                self.state_dots_box.set_visible(false);
                self.preview_inner_label.set_visible(false);
                self.preview_tile.add_css_class("inspector-touch-tile");
                self.preview_tile.set_size_request(160, 24);
            }
        }
    }

    pub fn populate_installed_apps(&self, apps: &[InstalledApp]) {
        *self.installed_apps.borrow_mut() = apps.to_vec();
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        let list = StringList::new(&names);
        self.app_dropdown.set_model(Some(&list));
    }

    pub fn highlight_action_settings(&self) {
        self.stack.set_visible_child_name("action_settings");
        self.tab_action_btn.add_css_class("suggested-action");
        self.tab_appearance_btn.remove_css_class("suggested-action");
    }

    pub fn set_key_action(&self, action: Option<&KeyAction>) {
        self.is_updating.set(true);

        if let Some(act) = action {
            let target = *self.current_target.borrow();
            match target {
                InspectorTarget::Touchscreen => {
                    self.title_label.set_text("Stream Deck + Touch Strip");
                }
                InspectorTarget::Dial(d) => {
                    self.title_label.set_text(&format!("Stream Deck + Dial {} & Card {}", d + 1, d + 1));
                }
                InspectorTarget::Key(k) => {
                    if act.action_id == "custom" || act.action_name.is_empty() {
                        self.title_label.set_text(&format!("Stream Deck Key {}", k + 1));
                    } else {
                        self.title_label.set_text(&format!("{}: {}", act.plugin_name, act.action_name));
                    }
                }
            }
            self.delete_button.set_sensitive(true);

            if !act.plugin_id.is_empty() && act.plugin_id != "system" {
                *self.current_plugin_id.borrow_mut() = Some(act.plugin_id.clone());
                *self.current_plugin_name.borrow_mut() = Some(act.plugin_name.clone());
                self.plugin_settings_button.set_tooltip_text(Some(&format!("{} Settings", act.plugin_name)));
                self.plugin_settings_button.set_visible(true);
            } else {
                *self.current_plugin_id.borrow_mut() = None;
                *self.current_plugin_name.borrow_mut() = None;
                self.plugin_settings_button.set_visible(false);
            }

            self.empty_action_label.set_visible(false);

            let is_builtin_app_launcher = act.action_id == "app_launcher.launch";
            if is_builtin_app_launcher {
                self.app_row.set_visible(true);
                self.args_row.set_visible(true);
                self.action_info_box.set_visible(false);
                self.dynamic_settings_box.set_visible(false);
                *self.rendered_live_slot.borrow_mut() = None;
                self.rendered_live_rows.borrow_mut().clear();
                self.rendered_live_widgets.borrow_mut().clear();

                // Select current application in dropdown
                if let Some(did) = act.parameters.get("desktop_id") {
                    let apps = self.installed_apps.borrow();
                    if let Some(pos) = apps.iter().position(|a| &a.desktop_id == did) {
                        self.app_dropdown.set_selected(pos as u32);
                    }
                }
                if let Some(args) = act.parameters.get("args") {
                    self.args_entry.set_text(args);
                } else {
                    self.args_entry.set_text("");
                }
            } else if !act.plugin_id.is_empty() && act.plugin_id != "system" {
                self.app_row.set_visible(false);
                self.args_row.set_visible(false);

                let target = *self.current_target.borrow();
                let slot = match target {
                    InspectorTarget::Key(k) => format!("key_{k}"),
                    InspectorTarget::Dial(d) => format!("dial_{d}"),
                    InspectorTarget::Touchscreen => "touchscreen".into(),
                };

                let is_native = self
                    .native_host
                    .borrow()
                    .as_ref()
                    .map(|h| h.is_native_plugin(&act.plugin_id))
                    .unwrap_or(false);

                let active_dkey = self.active_device_key.borrow().clone();
                let live_rows = if is_native {
                    self.native_host
                        .borrow()
                        .as_ref()
                        .and_then(|h| h.config_rows(&active_dkey, &slot))
                } else if let Some(ref pm) = *self.plugin_manager.borrow() {
                    pm.get_live_config_rows(&format!("{active_dkey}::{slot}"))
                        .or_else(|| pm.get_live_config_rows(&slot))
                } else {
                    None
                };

                if let Some(ref rows) = live_rows {
                    self.action_info_name.set_text(&act.action_name);
                    let desc = if is_native {
                        self.native_host
                            .borrow()
                            .as_ref()
                            .and_then(|h| h.find_descriptor(&act.plugin_id, &act.action_id))
                            .map(|d| d.description)
                            .unwrap_or_else(|| {
                                get_action_description(&act.action_id, &act.action_name, &act.plugin_name)
                            })
                    } else {
                        get_action_description(&act.action_id, &act.action_name, &act.plugin_name)
                    };
                    self.action_info_desc.set_text(&desc);
                    self.render_live_config_rows(&slot, rows);
                } else {
                    *self.rendered_live_slot.borrow_mut() = None;
                    self.rendered_live_rows.borrow_mut().clear();
                    self.rendered_live_widgets.borrow_mut().clear();
                    // Trigger live config row fetch from compatibility bridge / native plugin host
                    if let Some(ref pm) = *self.plugin_manager.borrow() {
                        let scoped_slot = if active_dkey.is_empty() {
                            slot.clone()
                        } else {
                            format!("{active_dkey}::{slot}")
                        };
                        pm.bridge.get_action_config_rows(&scoped_slot);
                    }

                    // Seamless check: does this action have real interactive settings?
                    let config_rows = if let Some(ref pm) = *self.plugin_manager.borrow() {
                        pm.get_action_config_rows(&act.plugin_id, &act.action_id)
                    } else {
                        Vec::new()
                    };

                    if !config_rows.is_empty() {
                        // Action HAS settings: hide info box, display actual action settings rows in unified flat layout matching Appearance tab
                        self.action_info_box.set_visible(false);

                        while let Some(child) = self.dynamic_settings_box.first_child() {
                            self.dynamic_settings_box.remove(&child);
                        }

                        for row in &config_rows {
                            let row_box = Box::new(Orientation::Horizontal, 10);

                            let caption_text = if row.title().ends_with(':') {
                                row.title().to_string()
                            } else {
                                format!("{}:", row.title())
                            };
                            let caption = Label::new(Some(&caption_text));
                            caption.set_size_request(100, -1);
                            caption.set_xalign(0.0);
                            row_box.append(&caption);

                            match &row.setting_type {
                                ActionSettingType::Combo { title: _, subtitle: _, options, default_idx } => {
                                    let str_opts: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
                                    let model = gtk::StringList::new(&str_opts);
                                    let dropdown = DropDown::new(Some(model), Option::<gtk::Expression>::None);
                                    dropdown.set_size_request(280, -1);

                                    if let Some(saved) = act.parameters.get(&row.key) {
                                        if let Some(idx) = options.iter().position(|o| o == saved) {
                                            dropdown.set_selected(idx as u32);
                                        }
                                    } else {
                                        dropdown.set_selected(*default_idx);
                                    }

                                    let on_param = self.on_parameter_changed.clone();
                                    let param_key = row.key.clone();
                                    let opts_clone = options.clone();
                                    dropdown.connect_selected_notify(move |dd| {
                                        let idx = dd.selected() as usize;
                                        if idx < opts_clone.len() {
                                            if let Some(ref cb) = *on_param.borrow() {
                                                cb(param_key.clone(), opts_clone[idx].clone());
                                            }
                                        }
                                    });

                                    row_box.append(&dropdown);
                                }
                                ActionSettingType::Switch { title: _, subtitle, default_active } => {
                                    let active = act.parameters.get(&row.key)
                                        .and_then(|v| v.parse::<bool>().ok())
                                        .unwrap_or(*default_active);

                                    let sw = Switch::new();
                                    sw.set_active(active);
                                    sw.set_valign(gtk::Align::Center);

                                    let on_param = self.on_parameter_changed.clone();
                                    let param_key = row.key.clone();
                                    sw.connect_active_notify(move |s| {
                                        let val = s.is_active();
                                        if let Some(ref cb) = *on_param.borrow() {
                                            cb(param_key.clone(), val.to_string());
                                        }
                                    });

                                    row_box.append(&sw);

                                    if !subtitle.is_empty() {
                                        let sub_lbl = Label::new(Some(subtitle));
                                        sub_lbl.add_css_class("dim-label");
                                        sub_lbl.set_valign(gtk::Align::Center);
                                        row_box.append(&sub_lbl);
                                    }
                                }
                                ActionSettingType::Text { title: _, subtitle: _, default_text } => {
                                    let text = act.parameters.get(&row.key).cloned().unwrap_or_else(|| default_text.clone());
                                    let entry = Entry::new();
                                    entry.set_text(&text);
                                    entry.set_size_request(280, -1);

                                    let on_param = self.on_parameter_changed.clone();
                                    let param_key = row.key.clone();
                                    entry.connect_changed(move |e| {
                                        let val = e.text().to_string();
                                        if let Some(ref cb) = *on_param.borrow() {
                                            cb(param_key.clone(), val);
                                        }
                                    });

                                    row_box.append(&entry);
                                }
                                ActionSettingType::Spin { title: _, subtitle: _, min, max, step, default_val } => {
                                    let val = act.parameters.get(&row.key)
                                        .and_then(|v| v.parse::<f64>().ok())
                                        .unwrap_or(*default_val);

                                    let spin = SpinButton::with_range(*min, *max, *step);
                                    spin.set_value(val);
                                    spin.set_size_request(100, -1);

                                    let on_param = self.on_parameter_changed.clone();
                                    let param_key = row.key.clone();
                                    spin.connect_value_changed(move |s| {
                                        let val = s.value();
                                        if let Some(ref cb) = *on_param.borrow() {
                                            cb(param_key.clone(), val.to_string());
                                        }
                                    });

                                    row_box.append(&spin);
                                }
                            }

                            self.dynamic_settings_box.append(&row_box);
                        }

                        self.dynamic_settings_box.set_visible(true);
                    } else {
                        // Action has NO interactive settings (e.g. Mute, Deafen)
                        // Display the unified flat summary rows
                        self.dynamic_settings_box.set_visible(false);

                        self.action_info_name.set_text(&act.action_name);
                        let desc = get_action_description(&act.action_id, &act.action_name, &act.plugin_name);
                        self.action_info_desc.set_text(&desc);

                        self.action_info_box.set_visible(true);
                    }
                }
            } else {
                self.app_row.set_visible(false);
                self.args_row.set_visible(false);
                self.action_info_box.set_visible(false);
                self.dynamic_settings_box.set_visible(false);
            }

            // Dedicated Interactive Step Editor for Native Multi-Action
            if act.action_id == "ballista.multi_action" {
                let target = *self.current_target.borrow();
                let slot = match target {
                    InspectorTarget::Key(k) => format!("key_{k}"),
                    InspectorTarget::Dial(d) => format!("dial_{d}"),
                    InspectorTarget::Touchscreen => "touchscreen".into(),
                };
                self.render_multi_action_editor(&slot, act);
                self.multi_action_box.set_visible(true);
            } else {
                self.multi_action_box.set_visible(false);
            }

            if act.states.len() > 1 {
                self.state_dots_box.set_visible(true);
                let st = act.current_state.min(act.states.len() - 1);
                self.selected_state_idx.set(st);
                if st == 0 {
                    self.radio_state_0.set_active(true);
                } else {
                    self.radio_state_1.set_active(true);
                }
            } else {
                self.state_dots_box.set_visible(false);
                self.selected_state_idx.set(0);
            }

            let app = act.active_appearance();

            self.title_entry.set_text(&app.title);

            // Font & Size
            self.font_size_spin.set_value(app.font_size as f64);
            let font_desc_str = format!("{} {}", app.font_family, app.font_size);
            self.font_dialog_button.set_font_desc(&pango::FontDescription::from_string(&font_desc_str));

            // Icon Scale
            self.icon_scale_spin.set_value(app.icon_scale as f64);

            // Update alignment button styling
            self.pos_top_btn.remove_css_class("suggested-action");
            self.pos_mid_btn.remove_css_class("suggested-action");
            self.pos_bot_btn.remove_css_class("suggested-action");
            match app.vertical_align {
                TextVerticalAlign::Top => self.pos_top_btn.add_css_class("suggested-action"),
                TextVerticalAlign::Center => self.pos_mid_btn.add_css_class("suggested-action"),
                TextVerticalAlign::Bottom => self.pos_bot_btn.add_css_class("suggested-action"),
            }

            // Update Color pickers
            if let Some(ref bg) = app.bg_color {
                self.btn_color_picker.set_rgba(&hex_to_rgba(bg));
            } else {
                self.btn_color_picker.set_rgba(&RGBA::new(0.098, 0.098, 0.125, 1.0));
            }
            self.text_color_picker.set_rgba(&hex_to_rgba(&app.text_color));

            if target == InspectorTarget::Touchscreen {
                if act.action_id == "disabled" {
                    self.swipe_dropdown.set_selected(1);
                } else {
                    self.swipe_dropdown.set_selected(0);
                }
            }

            if let Some(src) = self.active_preview_anim.borrow_mut().take() {
                src.remove();
            }

            // Update Preview tile with strictly bounded texture scaling (2:1 for dials, 1:1 for keys)
            let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
            let (base_w, base_h, radius) = match target {
                InspectorTarget::Touchscreen => (158, 22, 6.0),
                InspectorTarget::Dial(_) => (158, 78, 8.0),
                InspectorTarget::Key(_) => (78, 78, 10.0),
            };
            let icon_w = ((base_w as f64 * scale_pct) as i32).max(12);
            let icon_h = ((base_h as f64 * scale_pct) as i32).max(12);
            let pad_x = if app.icon_scale < 100 {
                ((base_w as f64 * (1.0 - scale_pct)) / 2.0).round() as i32
            } else {
                0
            };
            let pad_y = if app.icon_scale < 100 {
                ((base_h as f64 * (1.0 - scale_pct)) / 2.0).round() as i32
            } else {
                0
            };
            self.preview_icon.set_margin_start(pad_x);
            self.preview_icon.set_margin_end(pad_x);
            self.preview_icon.set_margin_top(pad_y);
            self.preview_icon.set_margin_bottom(pad_y);
            self.preview_icon.set_size_request(-1, -1);

            let mut icon_path = app.icon_path.as_deref();
            let mut icon_name = act.parameters.get("icon_name").map(|s| s.as_str())
                .or_else(|| match act.action_id.as_str() {
                    "system.hotkey" => Some("input-keyboard-symbolic"),
                    "system.url" => Some("web-browser-symbolic"),
                    "system.terminal" => Some("utilities-terminal-symbolic"),
                    "app_launcher.launch" => Some("system-run"),
                    _ => None,
                });

            // For App Launcher actions: ignore generic placeholder icons if an app has been configured
            if act.action_id.contains("AppLauncher") || act.action_id == "app_launcher.launch" {
                if let Some(p) = act.parameters.get("icon_path").filter(|s| !s.is_empty()) {
                    if !p.ends_with("action_launch.png") && !p.ends_with("default_app.png") {
                        icon_path = Some(p.as_str());
                    }
                } else if let Some(p) = icon_path {
                    if p.ends_with("action_launch.png") || p.ends_with("default_app.png") {
                        icon_path = None;
                    }
                }
                if let Some(inm) = act.parameters.get("icon_name").filter(|s| !s.is_empty()) {
                    icon_name = Some(inm.as_str());
                }
            }

            let target_slot_str = match target {
                InspectorTarget::Key(k) => format!("key_{k}"),
                InspectorTarget::Dial(d) => format!("dial_{d}"),
                InspectorTarget::Touchscreen => "touchscreen".into(),
            };
            let has_custom_path = icon_path
                .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
                .is_some();
            let active_dkey = self.active_device_key.borrow().clone();
            let show_native_prev = !has_custom_path
                && self
                    .native_host
                    .borrow()
                    .as_ref()
                    .map(|h| h.is_native_plugin(&act.plugin_id) && h.has_slot(&active_dkey, &target_slot_str))
                    .unwrap_or(false);

            self.preview_native_da.set_visible(show_native_prev);
            if show_native_prev {
                self.preview_native_da.queue_draw();
                self.preview_icon.set_visible(false);
            } else if let Some(icon) = icon_path {
                if std::path::Path::new(icon).exists() {
                    if icon.to_lowercase().ends_with(".gif") {
                        if let Some(texture) = crate::ui::canvas::create_rounded_texture(icon, icon_w, icon_h, 10.0, true) {
                            self.preview_icon.set_paintable(Some(&texture));
                        }

                        let (sender, receiver) = async_channel::unbounded::<Vec<crate::core::animation::RawGifFrame>>();
                        let pic_clone = self.preview_icon.clone();
                        let active_anim = self.active_preview_anim.clone();

                        glib::spawn_future_local(async move {
                            if let Ok(raw_frames) = receiver.recv().await {
                                if raw_frames.len() > 1 {
                                    use gtk::prelude::Cast;
                                    let mut textures: Vec<(gtk::gdk::Texture, u32)> = Vec::new();
                                    for f in raw_frames {
                                        let bytes = glib::Bytes::from(&f.rgba[..]);
                                        let texture = gtk::gdk::MemoryTexture::new(
                                            f.width as i32,
                                            f.height as i32,
                                            gtk::gdk::MemoryFormat::R8g8b8a8,
                                            &bytes,
                                            (f.width * 4) as usize,
                                        );
                                        textures.push((texture.upcast(), f.delay_ms));
                                    }

                                    let frames_rc = std::rc::Rc::new(textures);
                                    let frame_idx = std::rc::Rc::new(std::cell::Cell::new(0));
                                    let delay = frames_rc[0].1.max(30);

                                    let pic_anim = pic_clone.clone();
                                    let frames_anim = frames_rc.clone();
                                    let idx_anim = frame_idx.clone();

                                    let source_id = glib::timeout_add_local(
                                        std::time::Duration::from_millis(delay as u64),
                                        move || {
                                            let next_i = (idx_anim.get() + 1) % frames_anim.len();
                                            idx_anim.set(next_i);
                                            pic_anim.set_paintable(Some(&frames_anim[next_i].0));
                                            glib::ControlFlow::Continue
                                        }
                                    );

                                    if let Some(old) = active_anim.borrow_mut().replace(source_id) {
                                        old.remove();
                                    }
                                }
                            }
                        });

                        let path_clone = icon.to_string();
                        let dim = icon_w as u32;
                        std::thread::spawn(move || {
                            if let Some(raw_frames) = crate::core::animation::GifAnimation::load_key_gif_raw(&path_clone, dim, 10.0) {
                                let _ = sender.send_blocking(raw_frames);
                            }
                        });
                    } else if let Some(paintable) = crate::ui::canvas::lookup_rect_paintable(Some(icon), icon_name, icon_w, icon_h, radius) {
                        self.preview_icon.set_paintable(Some(&paintable));
                    } else {
                        self.preview_icon.set_filename(Some(icon));
                    }
                    self.preview_icon.set_visible(true);
                } else if let Some(paintable) = crate::ui::canvas::lookup_rect_paintable(None, icon_name, icon_w, icon_h, radius) {
                    self.preview_icon.set_paintable(Some(&paintable));
                    self.preview_icon.set_visible(true);
                } else {
                    self.preview_icon.set_visible(false);
                }
            } else if let Some(paintable) = crate::ui::canvas::lookup_rect_paintable(None, icon_name, icon_w, icon_h, radius) {
                self.preview_icon.set_paintable(Some(&paintable));
                self.preview_icon.set_visible(true);
            } else {
                self.preview_icon.set_visible(false);
            }
            if !app.show_title {
                self.preview_top_label.set_text("");
                self.preview_top_label.set_visible(false);
                self.preview_center_label.set_text("");
                self.preview_center_label.set_visible(false);
                self.preview_inner_label.set_text("");
                self.preview_inner_label.set_visible(false);
            } else if app.has_plugin_labels() {
                let top_txt = app.top_label.as_deref().unwrap_or("");
                self.preview_top_label.set_text(top_txt);
                self.preview_top_label.set_visible(!top_txt.is_empty());

                let cen_txt = app.center_label.as_deref().unwrap_or("");
                self.preview_center_label.set_text(cen_txt);
                self.preview_center_label.set_visible(!cen_txt.is_empty());

                let bot_txt = app.bottom_label.as_deref().unwrap_or("");
                self.preview_inner_label.set_text(bot_txt);
                self.preview_inner_label.set_visible(!bot_txt.is_empty());
                self.preview_inner_label.set_valign(gtk::Align::End);
            } else {
                self.preview_top_label.set_text("");
                self.preview_top_label.set_visible(false);
                self.preview_center_label.set_text("");
                self.preview_center_label.set_visible(false);
                if app.show_title && !app.title.is_empty() {
                    self.preview_inner_label.set_text(&app.title);
                    self.preview_inner_label.set_visible(true);
                } else {
                    self.preview_inner_label.set_text("");
                    self.preview_inner_label.set_visible(false);
                }
                match app.vertical_align {
                    TextVerticalAlign::Top => self.preview_inner_label.set_valign(gtk::Align::Start),
                    TextVerticalAlign::Center => self.preview_inner_label.set_valign(gtk::Align::Center),
                    TextVerticalAlign::Bottom => self.preview_inner_label.set_valign(gtk::Align::End),
                }
            }

            if app.show_title {
                self.title_toggle_btn.set_icon_name("view-reveal-symbolic");
                self.title_toggle_btn.set_tooltip_text(Some("Hide Title on Key"));
            } else {
                self.title_toggle_btn.set_icon_name("view-conceal-symbolic");
                self.title_toggle_btn.set_tooltip_text(Some("Show Title on Key"));
            }
        } else {
            if let Some(src) = self.active_preview_anim.borrow_mut().take() {
                src.remove();
            }
            *self.rendered_live_slot.borrow_mut() = None;
            self.rendered_live_rows.borrow_mut().clear();
            self.rendered_live_widgets.borrow_mut().clear();
            let target = *self.current_target.borrow();
            self.preview_tile.remove_css_class("inspector-key-tile");
            self.preview_tile.remove_css_class("inspector-dial-tile");
            self.preview_tile.remove_css_class("inspector-touch-tile");
            match target {
                InspectorTarget::Touchscreen => {
                    self.title_label.set_text("Stream Deck + Touch Strip");
                    self.preview_tile.add_css_class("inspector-touch-tile");
                    self.preview_tile.set_size_request(160, 24);
                    self.preview_sizer.set_size_request(158, 22);
                }
                InspectorTarget::Dial(d) => {
                    self.title_label.set_text(&format!("Stream Deck + Dial {} & Card {}", d + 1, d + 1));
                    self.preview_tile.add_css_class("inspector-dial-tile");
                    self.preview_tile.set_size_request(160, 80);
                    self.preview_sizer.set_size_request(158, 78);
                }
                InspectorTarget::Key(k) => {
                    self.title_label.set_text(&format!("Empty Key Slot {}", k + 1));
                    self.preview_tile.add_css_class("inspector-key-tile");
                    self.preview_tile.set_size_request(80, 80);
                    self.preview_sizer.set_size_request(78, 78);
                }
            }
            self.delete_button.set_sensitive(false);
            *self.current_plugin_id.borrow_mut() = None;
            *self.current_plugin_name.borrow_mut() = None;
            self.plugin_settings_button.set_visible(false);

            self.state_dots_box.set_visible(false);
            self.empty_action_label.set_visible(true);
            self.app_row.set_visible(false);
            self.args_row.set_visible(false);
            self.action_info_box.set_visible(false);
            self.dynamic_settings_box.set_visible(false);
            self.multi_action_box.set_visible(false);
            self.title_entry.set_text("");
            self.preview_native_da.set_visible(false);
            self.preview_icon.set_visible(false);
            self.preview_inner_label.set_text("");
            self.preview_inner_label.set_visible(false);
            self.preview_top_label.set_text("");
            self.preview_top_label.set_visible(false);
            self.preview_center_label.set_text("");
            self.preview_center_label.set_visible(false);
            self.btn_color_picker.set_rgba(&RGBA::new(0.098, 0.098, 0.125, 1.0));
            self.text_color_picker.set_rgba(&RGBA::new(1.0, 1.0, 1.0, 1.0));
            self.font_size_spin.set_value(11.0);
            self.icon_scale_spin.set_value(100.0);
            self.swipe_dropdown.set_selected(0);
            self.font_dialog_button.set_font_desc(&pango::FontDescription::from_string("Cantarell 11"));
        }

        self.is_updating.set(false);
    }

    /// Lightweight visual-only preview refresh (called on bridge SetMedia/SetLabel/SetBackgroundColor)
    /// without tearing down or resetting dynamic_settings_box!
    pub fn update_preview_only(&self, act: &KeyAction) {
        self.is_updating.set(true);
        let target = *self.current_target.borrow();
        let app = act.active_appearance();

        if let Some(ref bg) = app.bg_color {
            self.btn_color_picker.set_rgba(&hex_to_rgba(bg));
        } else {
            self.btn_color_picker.set_rgba(&RGBA::new(0.098, 0.098, 0.125, 1.0));
        }

        let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
        let (base_w, base_h, radius) = match target {
            InspectorTarget::Touchscreen => (160, 24, 6.0),
            InspectorTarget::Dial(_) => (160, 80, 8.0),
            InspectorTarget::Key(_) => (80, 80, 10.0),
        };
        let icon_w = ((base_w as f64 * scale_pct) as i32).max(12);
        let icon_h = ((base_h as f64 * scale_pct) as i32).max(12);
        let icon_path = app.icon_path.as_deref();
        let icon_name = act.parameters.get("icon_name").map(|s| s.as_str());

        let target_slot_str = match target {
            InspectorTarget::Key(k) => format!("key_{k}"),
            InspectorTarget::Dial(d) => format!("dial_{d}"),
            InspectorTarget::Touchscreen => "touchscreen".into(),
        };
        let has_custom_path = icon_path
            .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
            .is_some();
        let active_dkey = self.active_device_key.borrow().clone();
        let show_native_prev = !has_custom_path
            && self
                .native_host
                .borrow()
                .as_ref()
                .map(|h| h.is_native_plugin(&act.plugin_id) && h.has_slot(&active_dkey, &target_slot_str))
                .unwrap_or(false);

        self.preview_native_da.set_visible(show_native_prev);
        if show_native_prev {
            self.preview_native_da.queue_draw();
            self.preview_icon.set_visible(false);
        } else if let Some(icon) = icon_path.filter(|p| !p.is_empty() && std::path::Path::new(p).exists()) {
            if let Some(paintable) = crate::ui::canvas::lookup_rect_paintable(Some(icon), icon_name, icon_w, icon_h, radius) {
                self.preview_icon.set_paintable(Some(&paintable));
            } else {
                self.preview_icon.set_filename(Some(icon));
            }
            self.preview_icon.set_visible(true);
        } else if let Some(paintable) = crate::ui::canvas::lookup_rect_paintable(None, icon_name, icon_w, icon_h, radius) {
            self.preview_icon.set_paintable(Some(&paintable));
            self.preview_icon.set_visible(true);
        } else {
            self.preview_icon.set_visible(false);
        }

        if !app.show_title {
            self.preview_top_label.set_text("");
            self.preview_top_label.set_visible(false);
            self.preview_center_label.set_text("");
            self.preview_center_label.set_visible(false);
            self.preview_inner_label.set_text("");
            self.preview_inner_label.set_visible(false);
        } else if app.has_plugin_labels() {
            let top_txt = app.top_label.as_deref().unwrap_or("");
            self.preview_top_label.set_text(top_txt);
            self.preview_top_label.set_visible(!top_txt.is_empty());

            let cen_txt = app.center_label.as_deref().unwrap_or("");
            self.preview_center_label.set_text(cen_txt);
            self.preview_center_label.set_visible(!cen_txt.is_empty());

            let bot_txt = app.bottom_label.as_deref().unwrap_or("");
            self.preview_inner_label.set_text(bot_txt);
            self.preview_inner_label.set_visible(!bot_txt.is_empty());
            self.preview_inner_label.set_valign(gtk::Align::End);
        } else {
            self.preview_top_label.set_text("");
            self.preview_top_label.set_visible(false);
            self.preview_center_label.set_text("");
            self.preview_center_label.set_visible(false);
            if app.show_title && !app.title.is_empty() {
                self.preview_inner_label.set_text(&app.title);
                self.preview_inner_label.set_visible(true);
            } else {
                self.preview_inner_label.set_text("");
                self.preview_inner_label.set_visible(false);
            }
        }
        self.is_updating.set(false);
    }

    pub fn render_live_config_rows(&self, slot: &str, rows: &[crate::core::plugin_bridge::BridgeConfigRow]) {
        let visible_rows: Vec<crate::core::plugin_bridge::BridgeConfigRow> =
            rows.iter().filter(|r| r.visible).cloned().collect();

        let has_interactive = visible_rows.iter().any(|r| {
            matches!(
                r.row_type.as_str(),
                "ComboRow" | "SwitchRow" | "EntryRow" | "PasswordRow" | "SpinRow" | "ScaleRow" | "ColorRow" | "ButtonRow"
            )
        });

        if !has_interactive {
            while let Some(child) = self.dynamic_settings_box.first_child() {
                self.dynamic_settings_box.remove(&child);
            }
            *self.rendered_live_slot.borrow_mut() = Some(slot.to_string());
            self.rendered_live_rows.borrow_mut().clear();
            self.rendered_live_widgets.borrow_mut().clear();
            self.dynamic_settings_box.set_visible(false);
            // If plugin returned an informational ActionRow, display its title & subtitle cleanly
            if let Some(info_row) = visible_rows.first() {
                if !info_row.title.is_empty() {
                    self.action_info_name.set_text(&info_row.title);
                }
                if !info_row.subtitle.is_empty() {
                    self.action_info_desc.set_text(&info_row.subtitle);
                }
            }
            self.action_info_box.set_visible(true);
            return;
        }

        // Smart in-place patch check: avoid destroying widgets (losing Entry focus / Scale drag!) when structure matches
        let can_patch = {
            let prev_slot = self.rendered_live_slot.borrow();
            let prev_rows = self.rendered_live_rows.borrow();
            let prev_widgets = self.rendered_live_widgets.borrow();
            prev_slot.as_deref() == Some(slot)
                && prev_rows.len() == visible_rows.len()
                && prev_widgets.len() == visible_rows.len()
                && prev_rows.iter().zip(visible_rows.iter()).all(|(a, b)| {
                    a.index == b.index && a.row_type == b.row_type && a.title == b.title
                })
        };

        if can_patch {
            self.is_live_syncing.set(true);
            let prev_rows = self.rendered_live_rows.borrow().clone();
            let widgets = self.rendered_live_widgets.borrow();
            for ((prev, row), widget) in prev_rows.iter().zip(visible_rows.iter()).zip(widgets.iter()) {
                widget.set_sensitive(row.sensitive);
                match row.row_type.as_str() {
                    "ComboRow" => {
                        if let Some(dd) = widget.downcast_ref::<DropDown>() {
                            if prev.options != row.options {
                                let str_opts: Vec<&str> = row.options.iter().map(|s| s.as_str()).collect();
                                let model = gtk::StringList::new(&str_opts);
                                dd.set_model(Some(&model));
                                dd.set_enable_search(row.options.len() > 8);
                            }
                            if row.selected < row.options.len() && dd.selected() as usize != row.selected {
                                dd.set_selected(row.selected as u32);
                            }
                        }
                    }
                    "SwitchRow" => {
                        if let Some(sw) = widget.downcast_ref::<Switch>() {
                            if sw.is_active() != row.active {
                                sw.set_active(row.active);
                            }
                        }
                    }
                    "EntryRow" => {
                        if let Some(entry) = widget.downcast_ref::<Entry>() {
                            entry.set_editable(row.editable);
                            if !entry.has_focus() && entry.text().as_str() != row.text {
                                entry.set_text(&row.text);
                            }
                        }
                    }
                    "PasswordRow" => {
                        if let Some(pe) = widget.downcast_ref::<gtk::PasswordEntry>() {
                            pe.set_editable(row.editable);
                            if !pe.has_focus() && pe.text().as_str() != row.text {
                                pe.set_text(&row.text);
                            }
                        }
                    }
                    "SpinRow" => {
                        if let Some(spin) = widget.downcast_ref::<SpinButton>() {
                            if !spin.has_focus() && (spin.value() - row.value).abs() > 1e-6 {
                                spin.set_value(row.value);
                            }
                        }
                    }
                    "ScaleRow" => {
                        if let Some(scale) = widget.downcast_ref::<gtk::Scale>() {
                            if (scale.value() - row.value).abs() > 1e-6 {
                                scale.set_value(row.value);
                            }
                        }
                    }
                    "ColorRow" => {
                        if let Some(cb) = widget.downcast_ref::<ColorDialogButton>() {
                            if prev.color != row.color {
                                cb.set_rgba(&hex_to_rgba(&row.color));
                            }
                        }
                    }
                    "ButtonRow" => {
                        if let Some(btn) = widget.downcast_ref::<Button>() {
                            if !row.button_label.is_empty() {
                                btn.set_label(&row.button_label);
                            }
                            if row.suggested {
                                btn.add_css_class("suggested-action");
                            } else {
                                btn.remove_css_class("suggested-action");
                            }
                            if let Some(sub) = btn
                                .next_sibling()
                                .and_then(|w| w.downcast::<Label>().ok())
                            {
                                if sub.text().as_str() != row.subtitle {
                                    sub.set_text(&row.subtitle);
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(lbl) = widget.downcast_ref::<Label>() {
                            if lbl.text().as_str() != row.subtitle {
                                lbl.set_text(&row.subtitle);
                            }
                        }
                    }
                }
            }
            drop(widgets);
            *self.rendered_live_rows.borrow_mut() = visible_rows;
            self.is_live_syncing.set(false);
            self.action_info_box.set_visible(false);
            self.dynamic_settings_box.set_visible(true);
            return;
        }

        // Full rebuild for new slot or structural row change
        while let Some(child) = self.dynamic_settings_box.first_child() {
            self.dynamic_settings_box.remove(&child);
        }
        self.rendered_live_widgets.borrow_mut().clear();

        self.action_info_box.set_visible(false);
        self.is_live_syncing.set(true);

        for row in &visible_rows {
            let row_box = Box::new(Orientation::Horizontal, 10);

            let caption_text = if row.title.ends_with(':') {
                row.title.clone()
            } else {
                format!("{}:", row.title)
            };
            let caption = Label::new(Some(&caption_text));
            caption.set_size_request(120, -1);
            caption.set_xalign(0.0);
            caption.set_wrap(true);
            row_box.append(&caption);

            let slot_clone = slot.to_string();
            let row_idx = row.index;
            let sync_flag = self.is_live_syncing.clone();

            match row.row_type.as_str() {
                "ComboRow" => {
                    let str_opts: Vec<&str> = row.options.iter().map(|s| s.as_str()).collect();
                    let model = gtk::StringList::new(&str_opts);
                    let search_expr = gtk::PropertyExpression::new(
                        gtk::StringObject::static_type(),
                        gtk::Expression::NONE,
                        "string",
                    );
                    let dd = DropDown::new(Some(model), Some(search_expr));
                    dd.set_size_request(280, -1);
                    dd.set_enable_search(row.options.len() > 8);
                    dd.set_sensitive(row.sensitive);
                    if row.selected < row.options.len() {
                        dd.set_selected(row.selected as u32);
                    }

                    let on_row_changed = self.on_config_row_changed.clone();
                    dd.connect_selected_notify(move |combo| {
                        if sync_flag.get() {
                            return;
                        }
                        let sel = combo.selected() as usize;
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, Some(sel), None, None, None, None);
                        }
                    });

                    row_box.append(&dd);
                    self.rendered_live_widgets.borrow_mut().push(dd.upcast());
                }
                "SwitchRow" => {
                    let sw = Switch::new();
                    sw.set_active(row.active);
                    sw.set_valign(gtk::Align::Center);
                    sw.set_sensitive(row.sensitive);

                    let on_row_changed = self.on_config_row_changed.clone();
                    sw.connect_active_notify(move |s| {
                        if sync_flag.get() {
                            return;
                        }
                        let act = s.is_active();
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, None, Some(act), None, None, None);
                        }
                    });

                    row_box.append(&sw);
                    self.rendered_live_widgets.borrow_mut().push(sw.upcast());

                    if !row.subtitle.is_empty() {
                        let sub = Label::new(Some(&row.subtitle));
                        sub.add_css_class("dim-label");
                        sub.set_valign(gtk::Align::Center);
                        row_box.append(&sub);
                    }
                }
                "EntryRow" => {
                    let entry = Entry::new();
                    entry.set_text(&row.text);
                    entry.set_editable(row.editable);
                    entry.set_sensitive(row.sensitive);
                    entry.set_size_request(280, -1);

                    let on_row_changed = self.on_config_row_changed.clone();
                    entry.connect_changed(move |e| {
                        if sync_flag.get() {
                            return;
                        }
                        let val = e.text().to_string();
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, None, None, Some(val), None, None);
                        }
                    });

                    row_box.append(&entry);
                    self.rendered_live_widgets.borrow_mut().push(entry.upcast());
                }
                "PasswordRow" => {
                    let entry = gtk::PasswordEntry::new();
                    entry.set_text(&row.text);
                    entry.set_show_peek_icon(true);
                    entry.set_editable(row.editable);
                    entry.set_sensitive(row.sensitive);
                    entry.set_size_request(280, -1);

                    let on_row_changed = self.on_config_row_changed.clone();
                    entry.connect_changed(move |e| {
                        if sync_flag.get() {
                            return;
                        }
                        let val = e.text().to_string();
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, None, None, Some(val), None, None);
                        }
                    });

                    row_box.append(&entry);
                    self.rendered_live_widgets.borrow_mut().push(entry.upcast());
                }
                "SpinRow" => {
                    let step = if row.step > 0.0 { row.step } else { 1.0 };
                    let max_v = if row.max > row.min { row.max } else { row.min + 100.0 };
                    let spin = SpinButton::with_range(row.min, max_v, step);
                    spin.set_digits(row.digits);
                    spin.set_value(row.value);
                    spin.set_sensitive(row.sensitive);
                    spin.set_size_request(110, -1);

                    let on_row_changed = self.on_config_row_changed.clone();
                    spin.connect_value_changed(move |s| {
                        if sync_flag.get() {
                            return;
                        }
                        let val = s.value();
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, None, None, None, None, Some(val));
                        }
                    });

                    row_box.append(&spin);
                    self.rendered_live_widgets.borrow_mut().push(spin.upcast());
                }
                "ScaleRow" => {
                    let step = if row.step > 0.0 { row.step } else { 1.0 };
                    let max_v = if row.max > row.min { row.max } else { row.min + 100.0 };
                    let scale = gtk::Scale::with_range(Orientation::Horizontal, row.min, max_v, step);
                    scale.set_digits(row.digits as i32);
                    scale.set_draw_value(true);
                    scale.set_value_pos(gtk::PositionType::Right);
                    scale.set_value(row.value);
                    scale.set_sensitive(row.sensitive);
                    scale.set_size_request(280, -1);
                    scale.set_valign(gtk::Align::Center);

                    let on_row_changed = self.on_config_row_changed.clone();
                    scale.connect_value_changed(move |s| {
                        if sync_flag.get() {
                            return;
                        }
                        let val = s.value();
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, None, None, None, None, Some(val));
                        }
                    });

                    row_box.append(&scale);
                    self.rendered_live_widgets.borrow_mut().push(scale.upcast());
                }
                "ColorRow" => {
                    let bg_dialog = ColorDialog::builder().with_alpha(false).build();
                    let color_btn = ColorDialogButton::new(Some(bg_dialog));
                    color_btn.set_rgba(&hex_to_rgba(&row.color));
                    color_btn.set_valign(gtk::Align::Center);
                    color_btn.set_sensitive(row.sensitive);

                    let on_row_changed = self.on_config_row_changed.clone();
                    color_btn.connect_rgba_notify(move |cb_btn| {
                        if sync_flag.get() {
                            return;
                        }
                        let rgba = cb_btn.rgba();
                        let hex = rgba_to_hex(&rgba);
                        if let Some(ref cb) = *on_row_changed.borrow() {
                            cb(slot_clone.clone(), row_idx, None, None, None, Some(hex), None);
                        }
                    });

                    row_box.append(&color_btn);
                    self.rendered_live_widgets.borrow_mut().push(color_btn.upcast());

                    if !row.subtitle.is_empty() {
                        let sub = Label::new(Some(&row.subtitle));
                        sub.add_css_class("dim-label");
                        sub.set_valign(gtk::Align::Center);
                        row_box.append(&sub);
                    }
                }
                "ButtonRow" => {
                    let btn = if !row.button_label.is_empty() {
                        Button::with_label(&row.button_label)
                    } else if !row.button_icon.is_empty() {
                        Button::from_icon_name(&row.button_icon)
                    } else {
                        Button::with_label("Refresh")
                    };
                    btn.set_valign(gtk::Align::Center);
                    btn.set_sensitive(row.sensitive);
                    if row.suggested {
                        btn.add_css_class("suggested-action");
                    } else {
                        btn.add_css_class("flat");
                    }

                    let on_btn = self.on_config_button_clicked.clone();
                    btn.connect_clicked(move |_| {
                        if let Some(ref cb) = *on_btn.borrow() {
                            cb(slot_clone.clone(), row_idx);
                        }
                    });

                    row_box.append(&btn);
                    self.rendered_live_widgets.borrow_mut().push(btn.upcast());

                    if !row.subtitle.is_empty() {
                        let sub = Label::new(Some(&row.subtitle));
                        sub.add_css_class("dim-label");
                        sub.set_valign(gtk::Align::Center);
                        row_box.append(&sub);
                    }
                }
                _ => {
                    let sub = Label::new(Some(&row.subtitle));
                    sub.add_css_class("dim-label");
                    sub.set_valign(gtk::Align::Center);
                    sub.set_wrap(true);
                    row_box.append(&sub);
                    self.rendered_live_widgets.borrow_mut().push(sub.upcast());
                }
            }

            self.dynamic_settings_box.append(&row_box);
        }

        *self.rendered_live_slot.borrow_mut() = Some(slot.to_string());
        *self.rendered_live_rows.borrow_mut() = visible_rows;
        self.is_live_syncing.set(false);
        self.dynamic_settings_box.set_visible(true);
    }

    pub fn set_native_host(&self, host: crate::native_sdk::NativePluginHost) {
        *self.native_host.borrow_mut() = Some(host);
    }

    pub fn set_active_device_key(&self, key: &str) {
        *self.active_device_key.borrow_mut() = key.to_string();
    }

    pub fn redraw_native_preview(&self, slot: &str) {
        let cur_slot = match *self.current_target.borrow() {
            InspectorTarget::Key(k) => format!("key_{k}"),
            InspectorTarget::Dial(d) => format!("dial_{d}"),
            InspectorTarget::Touchscreen => "touchscreen".to_string(),
        };
        if cur_slot == slot && self.preview_native_da.is_visible() {
            self.preview_native_da.queue_draw();
        }
    }

    /// Renders the interactive Multi-Action Step Sequence Editor inside `self.multi_action_box`.
    pub fn render_multi_action_editor(&self, slot: &str, act: &KeyAction) {
        use crate::native_sdk::MultiActionStep;

        while let Some(child) = self.multi_action_box.first_child() {
            self.multi_action_box.remove(&child);
        }

        let merged = act.merged_settings_json();
        let is_two_state = merged
            .get("mode")
            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(0)
            == 1;

        let active_seq = if is_two_state {
            self.multi_active_seq.get().min(1)
        } else {
            0
        };
        let state_key = if active_seq == 1 {
            "steps_b".to_string()
        } else {
            "steps".to_string()
        };

        // Optional Sequence 1 / Sequence 2 switcher when in Two-State Toggle mode
        if is_two_state {
            let seq_row = Box::new(Orientation::Horizontal, 6);
            let seq_lbl = Label::new(Some("Sequence:"));
            seq_lbl.set_size_request(100, -1);
            seq_lbl.set_xalign(0.0);

            let btn_s1 = Button::with_label("State 1");
            let btn_s2 = Button::with_label("State 2");
            if active_seq == 0 {
                btn_s1.add_css_class("suggested-action");
                btn_s2.add_css_class("flat");
            } else {
                btn_s1.add_css_class("flat");
                btn_s2.add_css_class("suggested-action");
            }
            let seq_cell_1 = self.multi_active_seq.clone();
            let cb_1 = self.on_multi_steps_changed.clone();
            let slot_1 = slot.to_string();
            let steps_a_cur: Vec<MultiActionStep> = merged
                .get("steps")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let steps_b_cur: Vec<MultiActionStep> = merged
                .get("steps_b")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let sa_1 = steps_a_cur.clone();
            btn_s1.connect_clicked(move |_| {
                seq_cell_1.set(0);
                if let Some(ref cb) = *cb_1.borrow() {
                    cb(slot_1.clone(), "steps".to_string(), sa_1.clone());
                }
            });
            let seq_cell_2 = self.multi_active_seq.clone();
            let cb_2 = self.on_multi_steps_changed.clone();
            let slot_2 = slot.to_string();
            let sb_2 = steps_b_cur.clone();
            btn_s2.connect_clicked(move |_| {
                seq_cell_2.set(1);
                if let Some(ref cb) = *cb_2.borrow() {
                    cb(slot_2.clone(), "steps_b".to_string(), sb_2.clone());
                }
            });

            seq_row.append(&seq_lbl);
            seq_row.append(&btn_s1);
            seq_row.append(&btn_s2);
            self.multi_action_box.append(&seq_row);
        }

        let steps: Vec<MultiActionStep> = merged
            .get(&state_key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        // Add Step Bar
        let add_row = Box::new(Orientation::Horizontal, 8);
        let add_lbl = Label::new(Some("Add Step:"));
        add_lbl.set_size_request(100, -1);
        add_lbl.set_xalign(0.0);

        let step_kinds = StringList::new(&[
            "Delay (ms)",
            "Run Command",
            "Hotkey",
            "Type Text",
            "Launch App",
            "Open Website",
            "Media Control",
            "Switch Page",
            "Set Brightness",
            "Trigger Key",
        ]);
        let kind_dd = DropDown::new(Some(step_kinds), Option::<gtk::Expression>::None);
        kind_dd.set_size_request(190, -1);

        let add_btn = Button::with_label("+ Add");
        add_btn.add_css_class("suggested-action");
        {
            let dd_ref = kind_dd.clone();
            let steps_add = steps.clone();
            let cb_ref = self.on_multi_steps_changed.clone();
            let slot_s = slot.to_string();
            let key_s = state_key.clone();
            add_btn.connect_clicked(move |_| {
                let new_step = match dd_ref.selected() {
                    0 => MultiActionStep::Delay { ms: 500 },
                    1 => MultiActionStep::RunCommand { command: String::new() },
                    2 => MultiActionStep::Hotkey { keys: "Ctrl+Shift+M".into() },
                    3 => MultiActionStep::TypeText { text: String::new(), press_enter: true },
                    4 => MultiActionStep::LaunchApp { desktop_id: String::new(), app_name: String::new() },
                    5 => MultiActionStep::OpenUrl { url: "https://".into() },
                    6 => MultiActionStep::Media { action: "play_pause".into() },
                    7 => MultiActionStep::SwitchPage { target: "next".into() },
                    8 => MultiActionStep::Brightness { percent: 80 },
                    _ => MultiActionStep::TriggerKey { key_index: 0 },
                };
                let mut next = steps_add.clone();
                next.push(new_step);
                if let Some(ref cb) = *cb_ref.borrow() {
                    cb(slot_s.clone(), key_s.clone(), next);
                }
            });
        }

        add_row.append(&add_lbl);
        add_row.append(&kind_dd);
        add_row.append(&add_btn);
        self.multi_action_box.append(&add_row);

        if steps.is_empty() {
            let hint = Label::new(Some("No steps yet — pick an action above and click + Add."));
            hint.add_css_class("dim-label");
            hint.set_xalign(0.0);
            hint.set_margin_start(110);
            self.multi_action_box.append(&hint);
            return;
        }

        // Render ordered step cards
        for (idx, step) in steps.iter().enumerate() {
            let card = Box::new(Orientation::Horizontal, 8);
            card.add_css_class("multi-step-card");

            let badge = Label::new(Some(&format!("{}", idx + 1)));
            badge.add_css_class("multi-step-badge");
            badge.set_valign(gtk::Align::Center);
            card.append(&badge);

            // Inline editor tailored to the step kind
            match step {
                MultiActionStep::Delay { ms } => {
                    let lbl = Label::new(Some("Delay (ms):"));
                    lbl.set_valign(gtk::Align::Center);
                    let spin = SpinButton::with_range(50.0, 60000.0, 50.0);
                    spin.set_value(*ms as f64);
                    spin.set_size_request(110, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    spin.connect_value_changed(move |sp| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::Delay { ms }) = next.get_mut(idx) {
                            *ms = sp.value().round().max(10.0) as u64;
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&lbl);
                    card.append(&spin);
                }
                MultiActionStep::RunCommand { command } => {
                    let entry = Entry::new();
                    entry.set_placeholder_text(Some("Shell command..."));
                    entry.set_text(command);
                    entry.set_size_request(210, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    entry.connect_changed(move |e| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::RunCommand { command }) = next.get_mut(idx) {
                            *command = e.text().to_string();
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&entry);
                }
                MultiActionStep::Hotkey { keys } => {
                    let entry = Entry::new();
                    entry.set_placeholder_text(Some("Ctrl+Shift+M"));
                    entry.set_text(keys);
                    entry.set_size_request(210, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    entry.connect_changed(move |e| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::Hotkey { keys }) = next.get_mut(idx) {
                            *keys = e.text().to_string();
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&entry);
                }
                MultiActionStep::TypeText { text, .. } => {
                    let entry = Entry::new();
                    entry.set_placeholder_text(Some("Text snippet to type..."));
                    entry.set_text(text);
                    entry.set_size_request(210, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    entry.connect_changed(move |e| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::TypeText { text, .. }) = next.get_mut(idx) {
                            *text = e.text().to_string();
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&entry);
                }
                MultiActionStep::LaunchApp { app_name, desktop_id } => {
                    let btn_label = if !app_name.is_empty() {
                        format!("App: {}", app_name)
                    } else if !desktop_id.is_empty() {
                        format!("App: {}", desktop_id)
                    } else {
                        "Choose Application...".to_string()
                    };
                    let pick_btn = Button::with_label(&btn_label);
                    pick_btn.set_size_request(210, -1);
                    let apps_ref = self.installed_apps.clone();
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    let anchor = self.container.clone();
                    pick_btn.connect_clicked(move |_| {
                        let apps = apps_ref.borrow().clone();
                        let steps_inner = steps_c.clone();
                        let cb_inner = cb.clone();
                        let s_inner = s_str.clone();
                        let k_inner = k_str.clone();
                        show_app_picker_dialog(&anchor, &apps, move |picked| {
                            let mut next = steps_inner.clone();
                            if let Some(MultiActionStep::LaunchApp { desktop_id, app_name }) = next.get_mut(idx) {
                                *desktop_id = picked.desktop_id.clone();
                                *app_name = picked.name.clone();
                            }
                            if let Some(ref f) = *cb_inner.borrow() {
                                f(s_inner.clone(), k_inner.clone(), next);
                            }
                        });
                    });
                    card.append(&pick_btn);
                }
                MultiActionStep::OpenUrl { url } => {
                    let entry = Entry::new();
                    entry.set_placeholder_text(Some("https://..."));
                    entry.set_text(url);
                    entry.set_size_request(210, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    entry.connect_changed(move |e| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::OpenUrl { url }) = next.get_mut(idx) {
                            *url = e.text().to_string();
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&entry);
                }
                MultiActionStep::Media { action } => {
                    let opts = ["play_pause", "play", "pause", "next", "previous", "stop"];
                    let labels = ["Play / Pause", "Play", "Pause", "Next Track", "Previous Track", "Stop"];
                    let dd = DropDown::new(Some(StringList::new(&labels)), Option::<gtk::Expression>::None);
                    dd.set_size_request(210, -1);
                    if let Some(pos) = opts.iter().position(|o| *o == action.as_str()) {
                        dd.set_selected(pos as u32);
                    }
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    dd.connect_selected_notify(move |d| {
                        let sel = d.selected() as usize;
                        if let Some(&val) = opts.get(sel) {
                            let mut next = steps_c.clone();
                            if let Some(MultiActionStep::Media { action }) = next.get_mut(idx) {
                                *action = val.to_string();
                            }
                            if let Some(ref f) = *cb.borrow() {
                                f(s_str.clone(), k_str.clone(), next);
                            }
                        }
                    });
                    card.append(&dd);
                }
                MultiActionStep::SwitchPage { target } => {
                    let opts = ["next", "prev", "1", "2", "3", "4", "5"];
                    let labels = ["Next Page", "Previous Page", "Page 1", "Page 2", "Page 3", "Page 4", "Page 5"];
                    let dd = DropDown::new(Some(StringList::new(&labels)), Option::<gtk::Expression>::None);
                    dd.set_size_request(210, -1);
                    if let Some(pos) = opts.iter().position(|o| *o == target.as_str()) {
                        dd.set_selected(pos as u32);
                    }
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    dd.connect_selected_notify(move |d| {
                        let sel = d.selected() as usize;
                        if let Some(&val) = opts.get(sel) {
                            let mut next = steps_c.clone();
                            if let Some(MultiActionStep::SwitchPage { target }) = next.get_mut(idx) {
                                *target = val.to_string();
                            }
                            if let Some(ref f) = *cb.borrow() {
                                f(s_str.clone(), k_str.clone(), next);
                            }
                        }
                    });
                    card.append(&dd);
                }
                MultiActionStep::Brightness { percent } => {
                    let lbl = Label::new(Some("Brightness %:"));
                    lbl.set_valign(gtk::Align::Center);
                    let spin = SpinButton::with_range(0.0, 100.0, 5.0);
                    spin.set_value(*percent as f64);
                    spin.set_size_request(100, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    spin.connect_value_changed(move |sp| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::Brightness { percent }) = next.get_mut(idx) {
                            *percent = sp.value().round().clamp(0.0, 100.0) as u8;
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&lbl);
                    card.append(&spin);
                }
                MultiActionStep::TriggerKey { key_index } => {
                    let lbl = Label::new(Some("Trigger Key #:"));
                    lbl.set_valign(gtk::Align::Center);
                    let spin = SpinButton::with_range(1.0, 36.0, 1.0);
                    spin.set_value((*key_index + 1) as f64);
                    spin.set_size_request(100, -1);
                    let steps_c = steps.clone();
                    let cb = self.on_multi_steps_changed.clone();
                    let s_str = slot.to_string();
                    let k_str = state_key.clone();
                    spin.connect_value_changed(move |sp| {
                        let mut next = steps_c.clone();
                        if let Some(MultiActionStep::TriggerKey { key_index }) = next.get_mut(idx) {
                            *key_index = (sp.value().round() as usize).saturating_sub(1);
                        }
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    });
                    card.append(&lbl);
                    card.append(&spin);
                }
            }

            // Reorder Up / Down & Remove buttons
            let up_btn = Button::from_icon_name("go-up-symbolic");
            up_btn.add_css_class("flat");
            up_btn.set_sensitive(idx > 0);
            {
                let steps_c = steps.clone();
                let cb = self.on_multi_steps_changed.clone();
                let s_str = slot.to_string();
                let k_str = state_key.clone();
                up_btn.connect_clicked(move |_| {
                    if idx > 0 && idx < steps_c.len() {
                        let mut next = steps_c.clone();
                        next.swap(idx - 1, idx);
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    }
                });
            }

            let down_btn = Button::from_icon_name("go-down-symbolic");
            down_btn.add_css_class("flat");
            down_btn.set_sensitive(idx + 1 < steps.len());
            {
                let steps_c = steps.clone();
                let cb = self.on_multi_steps_changed.clone();
                let s_str = slot.to_string();
                let k_str = state_key.clone();
                down_btn.connect_clicked(move |_| {
                    if idx + 1 < steps_c.len() {
                        let mut next = steps_c.clone();
                        next.swap(idx, idx + 1);
                        if let Some(ref f) = *cb.borrow() {
                            f(s_str.clone(), k_str.clone(), next);
                        }
                    }
                });
            }

            let del_btn = Button::from_icon_name("user-trash-symbolic");
            del_btn.add_css_class("flat");
            del_btn.add_css_class("destructive-action");
            {
                let steps_c = steps.clone();
                let cb = self.on_multi_steps_changed.clone();
                let s_str = slot.to_string();
                let k_str = state_key.clone();
                del_btn.connect_clicked(move |_| {
                    let mut next = steps_c.clone();
                    if idx < next.len() {
                        next.remove(idx);
                    }
                    if let Some(ref f) = *cb.borrow() {
                        f(s_str.clone(), k_str.clone(), next);
                    }
                });
            }

            card.append(&up_btn);
            card.append(&down_btn);
            card.append(&del_btn);
            self.multi_action_box.append(&card);
        }
    }

    pub fn mark_assigned(&self) {
        self.delete_button.set_sensitive(true);
        let target = *self.current_target.borrow();
        match target {
            InspectorTarget::Key(k) => {
                if self.title_label.text().starts_with("Empty Key Slot") {
                    self.title_label.set_text(&format!("Stream Deck Key {}", k + 1));
                }
            }
            _ => {}
        }
    }
}

/// Midnight-themed Visual Application Picker Dialog showing each desktop application's real icon + name
/// with instant search filtering — zero typing commands required!
pub fn show_app_picker_dialog<F: Fn(InstalledApp) + 'static>(
    parent: &impl IsA<gtk::Widget>,
    apps: &[InstalledApp],
    on_picked: F,
) {
    let dialog = adw::Dialog::builder()
        .title("Choose Application")
        .content_width(440)
        .content_height(520)
        .build();

    let root = Box::new(Orientation::Vertical, 12);
    root.add_css_class("app-picker-root");

    let header_row = Box::new(Orientation::Horizontal, 8);
    let heading = Label::new(Some("Choose Application"));
    heading.add_css_class("title-3");
    heading.set_xalign(0.0);
    heading.set_hexpand(true);

    let close_btn = Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("flat");
    close_btn.add_css_class("circular");
    let dlg_close = dialog.clone();
    close_btn.connect_clicked(move |_| {
        dlg_close.close();
    });
    header_row.append(&heading);
    header_row.append(&close_btn);
    root.append(&header_row);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search installed applications..."));
    search.add_css_class("store-search-entry");
    root.append(&search);

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let list_box = Box::new(Orientation::Vertical, 4);
    let on_picked_rc = Rc::new(on_picked);
    let mut row_entries: Vec<(Button, String)> = Vec::new();

    for app in apps {
        let row_btn = Button::new();
        row_btn.add_css_class("flat");
        row_btn.add_css_class("app-picker-row");

        let inner = Box::new(Orientation::Horizontal, 12);
        inner.set_halign(gtk::Align::Fill);

        let resolved_picker_icon = super::canvas::resolve_icon_path(&app.icon, 48);
        let icon_img = if let Some(ref ip) = resolved_picker_icon {
            let img = gtk::Image::from_file(ip);
            img.set_pixel_size(32);
            img
        } else if !app.icon.is_empty() && std::path::Path::new(&app.icon).exists() {
            let img = gtk::Image::from_file(&app.icon);
            img.set_pixel_size(32);
            img
        } else if !app.icon.is_empty() {
            let img = gtk::Image::from_icon_name(&app.icon);
            img.set_pixel_size(32);
            img
        } else {
            let img = gtk::Image::from_icon_name("application-x-executable-symbolic");
            img.set_pixel_size(32);
            img
        };

        let text_col = Box::new(Orientation::Vertical, 2);
        text_col.set_valign(gtk::Align::Center);
        text_col.set_hexpand(true);

        let name_lbl = Label::new(Some(&app.name));
        name_lbl.set_xalign(0.0);
        name_lbl.add_css_class("heading");
        name_lbl.set_ellipsize(pango::EllipsizeMode::End);

        let id_lbl = Label::new(Some(app.desktop_id.trim_end_matches(".desktop")));
        id_lbl.set_xalign(0.0);
        id_lbl.add_css_class("dim-label");
        id_lbl.set_ellipsize(pango::EllipsizeMode::End);

        text_col.append(&name_lbl);
        text_col.append(&id_lbl);
        inner.append(&icon_img);
        inner.append(&text_col);

        if !app.source.is_empty() {
            let src_badge = Label::new(Some(&app.source.to_uppercase()));
            src_badge.add_css_class("cap-badge");
            match app.source.as_str() {
                "Flatpak" => src_badge.add_css_class("cap-badge-dial"),
                "Snap" | "AppImage" => src_badge.add_css_class("cap-badge-both"),
                _ => src_badge.add_css_class("cap-badge-key"),
            }
            src_badge.set_valign(gtk::Align::Center);
            inner.append(&src_badge);
        }

        row_btn.set_child(Some(&inner));

        let app_clone = app.clone();
        let cb = on_picked_rc.clone();
        let dlg_pick = dialog.clone();
        row_btn.connect_clicked(move |_| {
            cb(app_clone.clone());
            dlg_pick.close();
        });

        list_box.append(&row_btn);
        let search_key = format!(
            "{} {} {}",
            app.name.to_lowercase(),
            app.desktop_id.to_lowercase(),
            app.source.to_lowercase()
        );
        row_entries.push((row_btn, search_key));
    }

    if apps.is_empty() {
        let empty = Label::new(Some("Scanning desktop applications... try again in a moment."));
        empty.add_css_class("dim-label");
        empty.set_margin_top(24);
        list_box.append(&empty);
    }

    let entries_rc = Rc::new(row_entries);
    search.connect_search_changed(move |se| {
        let q = se.text().trim().to_lowercase();
        for (btn, key) in entries_rc.iter() {
            btn.set_visible(q.is_empty() || key.contains(&q));
        }
    });

    scrolled.set_child(Some(&list_box));
    root.append(&scrolled);
    dialog.set_child(Some(&root));
    dialog.present(Some(parent));
    search.grab_focus();
}

pub fn get_action_description(action_id: &str, action_name: &str, plugin_name: &str) -> String {
    let lower_id = action_id.to_lowercase();
    let lower_name = action_name.to_lowercase();

    // DiscordSC Actions
    if lower_id.contains("callmixer") || lower_name.contains("call mixer") {
        "Controls Discord voice call volume and channel audio".into()
    } else if lower_id.contains("deafen") || lower_name.contains("deafen") {
        "Toggles deafen state on Discord".into()
    } else if lower_id.contains("mute") || lower_name.contains("mute") {
        "Toggles microphone mute state on Discord".into()
    } else if lower_id.contains("inputmode") || lower_name.contains("input mode") {
        "Switches between Voice Activity and Push-to-Talk".into()
    } else if lower_id.contains("pushtotalk") || lower_name.contains("push to talk") {
        "Transmits voice while active or held".into()
    } else if lower_id.contains("notification") || lower_name.contains("notification") {
        "Toggles Discord audio and desktop notifications".into()
    } else if lower_id.contains("serverstatus") || lower_name.contains("server status") {
        "Displays real-time status of connected Discord server".into()
    } else if lower_id.contains("soundboard") || lower_name.contains("soundboard") {
        "Plays selected sound from your Discord soundboard".into()
    } else if lower_id.contains("textchannel") || lower_name.contains("text channel") {
        "Navigates directly to assigned text channel".into()
    } else if lower_id.contains("voicechannel") || lower_name.contains("voice channel") {
        "Connects to assigned voice room or channel".into()
    // Govee Actions
    } else if lower_id.contains("power") || lower_name.contains("power") {
        "Toggles device power on or off".into()
    } else if lower_id.contains("brightness") || lower_name.contains("brightness") {
        "Adjusts lighting brightness level".into()
    } else if lower_id.contains("color") || lower_name.contains("color") {
        "Sets lighting color or ambiance".into()
    } else if lower_id.contains("scene") || lower_name.contains("scene") {
        "Activates selected lighting scene".into()
    // OBS Actions
    } else if lower_id.contains("record") || lower_name.contains("record") {
        "Starts or stops OBS recording".into()
    } else if lower_id.contains("stream") || lower_name.contains("stream") {
        "Starts or stops live broadcasting".into()
    // App Launcher Actions
    } else if lower_id.contains("applauncher") || lower_name.contains("launch app") {
        "Launches the configured application".into()
    // System Actions
    } else if lower_id.contains("hotkey") {
        "Sends keyboard shortcut combination".into()
    } else if lower_id.contains("url") {
        "Opens web URL in default browser".into()
    } else if lower_id.contains("terminal") {
        "Executes custom shell command".into()
    } else {
        format!("Action provided by {plugin_name}")
    }
}
