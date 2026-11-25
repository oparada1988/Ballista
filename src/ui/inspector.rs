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
    pub empty_action_label: Label,
    pub action_info_box: Box,
    pub action_info_name: Label,
    pub action_info_desc: Label,
    pub dynamic_settings_box: Box,
    pub swipe_row: Box,
    pub swipe_dropdown: DropDown,
    pub app_dropdown: DropDown,
    pub preview_tile: Overlay,
    pub preview_inner_label: Label,
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
    pub on_parameter_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String)>>>>,
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

        let preview_icon = Picture::new();
        preview_icon.set_can_shrink(true);
        preview_icon.set_content_fit(gtk::ContentFit::Contain);
        preview_icon.set_halign(gtk::Align::Center);
        preview_icon.set_valign(gtk::Align::Center);
        preview_icon.set_size_request(80, 80);
        preview_icon.set_visible(false);

        let preview_inner_label = Label::new(Some(""));
        preview_inner_label.set_valign(gtk::Align::End);
        preview_inner_label.set_halign(gtk::Align::Center);
        preview_inner_label.set_wrap(true);
        preview_inner_label.set_justify(gtk::Justification::Center);
        preview_inner_label.add_css_class("deck-key-label");

        preview_tile.set_child(Some(&preview_icon));
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

        let on_parameter_changed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(String, String)>>>> = Rc::new(RefCell::new(None));

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
            empty_action_label,
            action_info_box,
            action_info_name,
            action_info_desc,
            dynamic_settings_box,
            swipe_row,
            swipe_dropdown,
            app_dropdown,
            preview_tile,
            preview_inner_label,
            preview_icon,
            left_preview_col,
            state_dots_box,
            radio_state_0,
            radio_state_1,
            selected_state_idx: Rc::new(Cell::new(0)),
            is_updating: Rc::new(Cell::new(false)),
            current_target: Rc::new(RefCell::new(InspectorTarget::Key(0))),
            current_key_idx: Rc::new(RefCell::new(Some(0))),
            installed_apps: Rc::new(RefCell::new(Vec::new())),
            active_preview_anim: Rc::new(RefCell::new(None)),
            current_plugin_id,
            current_plugin_name,
            on_open_plugin_settings,
            plugin_manager: Rc::new(RefCell::new(None)),
            on_parameter_changed,
        }
    }

    pub fn set_target(&self, target: InspectorTarget) {
        *self.current_target.borrow_mut() = target;
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
                self.preview_tile.set_size_request(80, 80);
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
                self.preview_tile.set_size_request(80, 50);
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
                self.preview_tile.set_size_request(160, 20);
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

            let is_app_launcher = act.action_id.contains("AppLauncher") || act.action_id == "app_launcher.launch";
            if is_app_launcher {
                self.app_row.set_visible(true);
                self.args_row.set_visible(true);
                self.action_info_box.set_visible(false);
                self.dynamic_settings_box.set_visible(false);
            } else if !act.plugin_id.is_empty() && act.plugin_id != "system" {
                self.app_row.set_visible(false);
                self.args_row.set_visible(false);

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
            } else {
                self.app_row.set_visible(false);
                self.args_row.set_visible(false);
                self.action_info_box.set_visible(false);
                self.dynamic_settings_box.set_visible(false);
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

            // Update Preview tile with strictly bounded texture scaling
            let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
            let (base_w, base_h) = match target {
                InspectorTarget::Touchscreen => (160, 20),
                InspectorTarget::Dial(_) => (90, 48),
                InspectorTarget::Key(_) => (80, 80),
            };
            let icon_w = ((base_w as f64 * scale_pct) as i32).max(12);
            let icon_h = ((base_h as f64 * scale_pct) as i32).max(12);

            let icon_path = app.icon_path.as_deref();
            let icon_name = act.parameters.get("icon_name").map(|s| s.as_str())
                .or_else(|| match act.action_id.as_str() {
                    "system.hotkey" => Some("input-keyboard-symbolic"),
                    "system.url" => Some("web-browser-symbolic"),
                    "system.terminal" => Some("utilities-terminal-symbolic"),
                    "app_launcher.launch" => Some("system-run"),
                    _ => None,
                });

            if let Some(icon) = icon_path {
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
                    } else if let Some(paintable) = crate::ui::canvas::lookup_icon_paintable(Some(icon), icon_name, icon_w, 10.0) {
                        self.preview_icon.set_paintable(Some(&paintable));
                    } else {
                        self.preview_icon.set_filename(Some(icon));
                    }
                    self.preview_icon.set_size_request(icon_w, icon_h);
                    self.preview_icon.set_visible(true);
                } else if let Some(paintable) = crate::ui::canvas::lookup_icon_paintable(None, icon_name, icon_w, 10.0) {
                    self.preview_icon.set_paintable(Some(&paintable));
                    self.preview_icon.set_size_request(icon_w, icon_h);
                    self.preview_icon.set_visible(true);
                } else {
                    self.preview_icon.set_visible(false);
                }
            } else if let Some(paintable) = crate::ui::canvas::lookup_icon_paintable(None, icon_name, icon_w, 10.0) {
                self.preview_icon.set_paintable(Some(&paintable));
                self.preview_icon.set_size_request(icon_w, icon_h);
                self.preview_icon.set_visible(true);
            } else {
                self.preview_icon.set_visible(false);
            }
            if app.show_title && !app.title.is_empty() {
                self.preview_inner_label.set_text(&app.title);
                self.preview_inner_label.set_visible(true);
            } else {
                self.preview_inner_label.set_text("");
                self.preview_inner_label.set_visible(false);
            }

            if app.show_title {
                self.title_toggle_btn.set_icon_name("view-reveal-symbolic");
                self.title_toggle_btn.set_tooltip_text(Some("Hide Title on Key"));
            } else {
                self.title_toggle_btn.set_icon_name("view-conceal-symbolic");
                self.title_toggle_btn.set_tooltip_text(Some("Show Title on Key"));
            }
            match app.vertical_align {
                TextVerticalAlign::Top => self.preview_inner_label.set_valign(gtk::Align::Start),
                TextVerticalAlign::Center => self.preview_inner_label.set_valign(gtk::Align::Center),
                TextVerticalAlign::Bottom => self.preview_inner_label.set_valign(gtk::Align::End),
            }
        } else {
            if let Some(src) = self.active_preview_anim.borrow_mut().take() {
                src.remove();
            }
            let target = *self.current_target.borrow();
            match target {
                InspectorTarget::Touchscreen => {
                    self.title_label.set_text("Stream Deck + Touch Strip");
                    self.preview_tile.set_size_request(160, 20);
                }
                InspectorTarget::Dial(d) => {
                    self.title_label.set_text(&format!("Stream Deck + Dial {} & Card {}", d + 1, d + 1));
                    self.preview_tile.set_size_request(100, 52);
                }
                InspectorTarget::Key(k) => {
                    self.title_label.set_text(&format!("Empty Key Slot {}", k + 1));
                    self.preview_tile.set_size_request(80, 80);
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
            self.title_entry.set_text("");
            self.preview_icon.set_visible(false);
            self.preview_inner_label.set_text("");
            self.btn_color_picker.set_rgba(&RGBA::new(0.098, 0.098, 0.125, 1.0));
            self.text_color_picker.set_rgba(&RGBA::new(1.0, 1.0, 1.0, 1.0));
            self.font_size_spin.set_value(11.0);
            self.icon_scale_spin.set_value(100.0);
            self.swipe_dropdown.set_selected(0);
            self.font_dialog_button.set_font_desc(&pango::FontDescription::from_string("Cantarell 11"));
        }

        self.is_updating.set(false);
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
