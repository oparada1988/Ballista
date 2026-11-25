use crate::models::{DeviceKind, Profile, TextVerticalAlign};
use gtk::cairo;
use gtk::prelude::*;
use gtk::{Box, Button, CenterBox, DrawingArea, Grid, Label, Orientation, Overlay, Picture};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub fn create_rounded_texture(path: &str, width: i32, height: i32, radius: f64, preserve_aspect: bool) -> Option<gtk::gdk::Texture> {
    use gtk::gdk::prelude::GdkCairoContextExt;
    let pb = gdk_pixbuf::Pixbuf::from_file_at_scale(path, width, height, preserve_aspect).ok()?;
    let pw = pb.width();
    let ph = pb.height();

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, pw, ph).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;

    if radius > 0.0 {
        let r = radius.min((pw.min(ph) as f64) / 2.0);
        let w = pw as f64;
        let h = ph as f64;
        cr.new_sub_path();
        cr.arc(w - r, r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        cr.arc(w - r, h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        cr.arc(r, h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        cr.arc(r, r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
        cr.close_path();
        cr.clip();
    }

    cr.set_source_pixbuf(&pb, 0.0, 0.0);
    let _ = cr.paint();
    drop(cr);

    let stride = surface.stride();
    let data = surface.take_data().ok()?;
    let bytes = glib::Bytes::from(&*data);
    let texture = gtk::gdk::MemoryTexture::new(
        pw,
        ph,
        gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride as usize,
    );
    Some(texture.upcast())
}

/// Resolves an icon paintable from either a file path or an icon name (system/app icon)
pub fn lookup_icon_paintable(
    icon_path: Option<&str>,
    icon_name: Option<&str>,
    size: i32,
    radius: f64,
) -> Option<gtk::gdk::Paintable> {
    // 1. Explicit file path
    if let Some(path) = icon_path {
        if std::path::Path::new(path).exists() {
            if let Some(texture) = create_rounded_texture(path, size, size, radius, true) {
                return Some(texture.upcast());
            } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true) {
                return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
            }
        }
    }
    // 2. Icon name (app icon, system icon, or freedesktop path)
    if let Some(name) = icon_name {
        if !name.is_empty() {
            // Direct path check if name is a filepath
            if std::path::Path::new(name).exists() {
                if let Some(texture) = create_rounded_texture(name, size, size, radius, true) {
                    return Some(texture.upcast());
                } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(name, size, size, true) {
                    return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
                }
            }
            // Candidate icon paths in /usr/share/
            let candidates = [
                format!("/usr/share/pixmaps/{name}.png"),
                format!("/usr/share/pixmaps/{name}.svg"),
                format!("/usr/share/icons/hicolor/48x48/apps/{name}.png"),
                format!("/usr/share/icons/hicolor/scalable/apps/{name}.svg"),
                format!("/usr/share/icons/hicolor/128x128/apps/{name}.png"),
                format!("/usr/share/icons/hicolor/256x256/apps/{name}.png"),
            ];
            for cand in &candidates {
                if std::path::Path::new(cand).exists() {
                    if let Some(texture) = create_rounded_texture(cand, size, size, radius, true) {
                        return Some(texture.upcast());
                    } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(cand, size, size, true) {
                        return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
                    }
                }
            }
            // GtkIconTheme lookup
            if let Some(display) = gtk::gdk::Display::default() {
                let theme = gtk::IconTheme::for_display(&display);
                if theme.has_icon(name) {
                    let paintable = theme.lookup_icon(
                        name,
                        &[],
                        size,
                        1,
                        gtk::TextDirection::None,
                        gtk::IconLookupFlags::empty(),
                    );
                    return Some(paintable.upcast());
                }
                // Try short name without reverse-DNS domain
                let short_name = name.rsplit('.').next().unwrap_or(name);
                if short_name != name && theme.has_icon(short_name) {
                    let paintable = theme.lookup_icon(
                        short_name,
                        &[],
                        size,
                        1,
                        gtk::TextDirection::None,
                        gtk::IconLookupFlags::empty(),
                    );
                    return Some(paintable.upcast());
                }
            }
            // Fallback to default_app.png if available
            let fallback_app = "/home/oscar/.local/share/ballista/plugins/com_oparada_AppLauncher/assets/default_app.png";
            if std::path::Path::new(fallback_app).exists() {
                if let Some(texture) = create_rounded_texture(fallback_app, size, size, radius, true) {
                    return Some(texture.upcast());
                } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(fallback_app, size, size, true) {
                    return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
                }
            }
        }
    }
    None
}

pub struct Canvas {
    pub container: Box,
    pub hardware_area: Box,
    pub key_buttons: Rc<RefCell<Vec<Button>>>,
    pub key_labels: Rc<RefCell<Vec<Label>>>,
    pub key_icons: Rc<RefCell<Vec<Picture>>>,
    pub key_overlays: Rc<RefCell<Vec<Overlay>>>,
    pub touch_segments: Rc<RefCell<Vec<Box>>>,
    pub touch_labels: Rc<RefCell<Vec<Label>>>,
    pub touch_icons: Rc<RefCell<Vec<Picture>>>,
    pub touch_capsule: Rc<RefCell<Option<Box>>>,
    pub touch_overlay: Rc<RefCell<Option<Overlay>>>,
    pub touch_wallpaper_pic: Rc<RefCell<Option<Picture>>>,
    pub dial_draw_areas: Rc<RefCell<Vec<DrawingArea>>>,
    pub dial_angles: Rc<RefCell<Vec<Rc<Cell<f64>>>>>,
    pub selected_dial_idx: Rc<RefCell<Option<usize>>>,
    pub on_dial_rotate: Rc<RefCell<Option<std::boxed::Box<dyn Fn(usize, i32)>>>>,
    pub on_dial_clicked: Rc<RefCell<Option<std::boxed::Box<dyn Fn(usize)>>>>,
    pub on_touch_capsule_clicked: Rc<RefCell<Option<std::boxed::Box<dyn Fn()>>>>,
    pub on_action_dropped: Rc<RefCell<Option<std::boxed::Box<dyn Fn(crate::ui::inspector::InspectorTarget, crate::ui::sidebar::SelectedAction)>>>>,
    pub grid: Rc<RefCell<Option<Grid>>>,
    pub selected_idx: Rc<RefCell<Option<usize>>>,
    pub current_kind: Rc<RefCell<DeviceKind>>,
    pub current_btn_size: Rc<Cell<i32>>,
    pub css_provider: gtk::CssProvider,
    pub page_pill_box: Box,
    pub page_chips_box: Box,
    pub page_chips: Rc<RefCell<Vec<Button>>>,
    pub add_page_btn: Button,
    pub remove_page_btn: Button,
    pub on_page_selected: Rc<RefCell<Option<std::boxed::Box<dyn Fn(usize)>>>>,
    pub active_ui_animations: Rc<RefCell<std::collections::HashMap<usize, glib::SourceId>>>,
}

impl Canvas {
    pub fn new(initial_kind: DeviceKind) -> Self {
        let container = Box::new(Orientation::Vertical, 16);
        container.add_css_class("deck-canvas");
        container.set_valign(gtk::Align::Center);
        container.set_halign(gtk::Align::Center);
        container.set_vexpand(true);
        container.set_hexpand(true);

        let hardware_area = Box::new(Orientation::Vertical, 12);
        hardware_area.set_halign(gtk::Align::Center);
        hardware_area.set_valign(gtk::Align::Center);
        container.append(&hardware_area);

        // Elgato-Style Growing Pill Navigation Bar: Pages: [ (1) ] [ 2 ] [ 3 ] [ + ] [ 🗑 ]
        let pagination_bar = CenterBox::new();
        let page_pill_box = Box::new(Orientation::Horizontal, 6);
        page_pill_box.add_css_class("page-pill");

        let pages_caption = Label::new(Some("Pages:"));
        pages_caption.add_css_class("page-caption");

        let page_chips_box = Box::new(Orientation::Horizontal, 4);
        page_chips_box.add_css_class("page-chips-box");

        let add_page_btn = Button::from_icon_name("list-add-symbolic");
        add_page_btn.add_css_class("flat");
        add_page_btn.add_css_class("page-add-chip");
        add_page_btn.set_tooltip_text(Some("Add New Page (Max 10)"));

        let remove_page_btn = Button::from_icon_name("user-trash-symbolic");
        remove_page_btn.add_css_class("flat");
        remove_page_btn.add_css_class("page-remove-chip");
        remove_page_btn.set_tooltip_text(Some("Delete Current Page"));

        page_pill_box.append(&pages_caption);
        page_pill_box.append(&page_chips_box);
        page_pill_box.append(&add_page_btn);
        page_pill_box.append(&remove_page_btn);

        pagination_bar.set_center_widget(Some(&page_pill_box));
        container.append(&pagination_bar);

        let css_provider = gtk::CssProvider::new();
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }

        let s = Self {
            container,
            hardware_area,
            key_buttons: Rc::new(RefCell::new(Vec::new())),
            key_labels: Rc::new(RefCell::new(Vec::new())),
            key_icons: Rc::new(RefCell::new(Vec::new())),
            key_overlays: Rc::new(RefCell::new(Vec::new())),
            touch_segments: Rc::new(RefCell::new(Vec::new())),
            touch_labels: Rc::new(RefCell::new(Vec::new())),
            touch_icons: Rc::new(RefCell::new(Vec::new())),
            touch_capsule: Rc::new(RefCell::new(None)),
            touch_overlay: Rc::new(RefCell::new(None)),
            touch_wallpaper_pic: Rc::new(RefCell::new(None)),
            dial_draw_areas: Rc::new(RefCell::new(Vec::new())),
            dial_angles: Rc::new(RefCell::new(Vec::new())),
            selected_dial_idx: Rc::new(RefCell::new(None)),
            on_dial_rotate: Rc::new(RefCell::new(None)),
            on_dial_clicked: Rc::new(RefCell::new(None)),
            on_touch_capsule_clicked: Rc::new(RefCell::new(None)),
            on_action_dropped: Rc::new(RefCell::new(None)),
            grid: Rc::new(RefCell::new(None)),
            selected_idx: Rc::new(RefCell::new(None)),
            current_kind: Rc::new(RefCell::new(initial_kind)),
            current_btn_size: Rc::new(Cell::new(86)),
            css_provider,
            page_pill_box,
            page_chips_box,
            page_chips: Rc::new(RefCell::new(Vec::new())),
            add_page_btn,
            remove_page_btn,
            on_page_selected: Rc::new(RefCell::new(None)),
            active_ui_animations: Rc::new(RefCell::new(std::collections::HashMap::new())),
        };

        s.build_hardware_layout(initial_kind);
        s
    }

    pub fn set_page_callback<F: Fn(usize) + 'static>(&self, f: F) {
        *self.on_page_selected.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_action_dropped_callback<F: Fn(crate::ui::inspector::InspectorTarget, crate::ui::sidebar::SelectedAction) + 'static>(&self, f: F) {
        *self.on_action_dropped.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    /// Rebuilds the hardware canvas layout using a single unified Grid
    pub fn build_hardware_layout(&self, kind: DeviceKind) {
        *self.current_kind.borrow_mut() = kind;

        // Clear existing widgets
        while let Some(child) = self.hardware_area.first_child() {
            self.hardware_area.remove(&child);
        }
        self.key_buttons.borrow_mut().clear();
        self.key_labels.borrow_mut().clear();
        self.key_icons.borrow_mut().clear();
        self.key_overlays.borrow_mut().clear();
        self.touch_segments.borrow_mut().clear();
        self.touch_labels.borrow_mut().clear();
        self.touch_icons.borrow_mut().clear();
        self.dial_draw_areas.borrow_mut().clear();
        self.dial_angles.borrow_mut().clear();

        let (rows, cols) = kind.grid_dimensions();

        // 1. Unified Grid System (Locking Keys, Touchscreen, and Dials to identical column positions)
        let grid = Grid::new();
        grid.set_halign(gtk::Align::Center);
        grid.set_valign(gtk::Align::Center);
        grid.set_hexpand(false);
        grid.set_vexpand(false);

        let (btn_size, spacing) = match kind {
            DeviceKind::StreamDeckXL => (74, 10),
            DeviceKind::StreamDeckPlusXL => (68, 8),
            DeviceKind::StreamDeckMini => (94, 14),
            DeviceKind::StreamDeckPedal => (120, 18),
            _ => (86, 12), // StreamDeckMK2 (15) and StreamDeckPlus (8)
        };
        self.current_btn_size.set(btn_size);

        grid.set_row_spacing(spacing);
        grid.set_column_spacing(spacing);
        grid.set_column_homogeneous(true);
        grid.set_row_homogeneous(true);

        let mut key_buttons = Vec::new();
        let mut key_labels = Vec::new();
        let mut key_icons = Vec::new();
        let mut key_overlays = Vec::new();

        // Primary LCD Keys: Rows 0..rows (Pure blank initially, zero numbers)
        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let btn = Button::new();
                btn.add_css_class("deck-key");
                btn.add_css_class(&format!("deck-key-slot-{}", idx));
                btn.set_size_request(btn_size, btn_size);
                btn.set_hexpand(false);
                btn.set_vexpand(false);
                btn.set_overflow(gtk::Overflow::Hidden);

                // Drag and Drop Target for Actions dropped from Sidebar
                let drop_target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::COPY);
                let dropped_cb = self.on_action_dropped.clone();
                let k_idx = idx;
                drop_target.connect_drop(move |_target, value, _x, _y| {
                    if let Ok(json_str) = value.get::<String>() {
                        if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                            if let Some(ref cb) = *dropped_cb.borrow() {
                                cb(crate::ui::inspector::InspectorTarget::Key(k_idx), action);
                                return true;
                            }
                        }
                    }
                    false
                });
                btn.add_controller(drop_target);

                let overlay = Overlay::new();
                overlay.set_can_target(false);
                overlay.set_overflow(gtk::Overflow::Hidden);
                overlay.set_size_request(btn_size, btn_size);
                overlay.set_hexpand(false);
                overlay.set_vexpand(false);

                let icon = Picture::new();
                icon.set_can_shrink(true);
                icon.set_content_fit(gtk::ContentFit::Contain);
                icon.set_halign(gtk::Align::Center);
                icon.set_valign(gtk::Align::Center);
                icon.set_size_request(btn_size, btn_size);
                icon.set_visible(false);
                icon.set_can_target(false);

                let label = Label::new(None);
                label.add_css_class("deck-key-label");
                label.set_wrap(true);
                label.set_justify(gtk::Justification::Center);
                label.set_halign(gtk::Align::Center);
                label.set_valign(gtk::Align::End);
                label.set_margin_start(4);
                label.set_margin_end(4);
                label.set_margin_top(4);
                label.set_margin_bottom(4);
                label.set_visible(false);
                label.set_can_target(false);

                overlay.set_child(Some(&icon));
                overlay.add_overlay(&label);
                btn.set_child(Some(&overlay));

                grid.attach(&btn, c as i32, r as i32, 1, 1);
                key_buttons.push(btn);
                key_labels.push(label);
                key_icons.push(icon);
                key_overlays.push(overlay);
            }
        }

        // 2. Stream Deck Plus / Stream Deck Plus XL: Touchscreen Capsule & Dials
        if kind.has_dials() {
            let dial_count = kind.dial_count();
            let strip_width = cols as i32 * btn_size + (cols as i32 - 1) * spacing as i32;
            let strip_height = 52;

            // Row `rows`: Continuous Touchscreen Capsule spanning the entire width
            let touch_overlay = Overlay::new();
            touch_overlay.add_css_class("deck-touchscreen-capsule");
            touch_overlay.set_hexpand(false);
            touch_overlay.set_vexpand(false);
            touch_overlay.set_halign(gtk::Align::Center);
            touch_overlay.set_valign(gtk::Align::Center);
            touch_overlay.set_size_request(strip_width, strip_height);

            let touch_wallpaper_pic = Picture::new();
            touch_wallpaper_pic.set_can_shrink(true);
            touch_wallpaper_pic.set_content_fit(gtk::ContentFit::Cover);
            touch_wallpaper_pic.set_size_request(strip_width, strip_height);
            touch_wallpaper_pic.set_visible(false);
            touch_wallpaper_pic.set_can_target(false);
            touch_overlay.set_child(Some(&touch_wallpaper_pic));

            let touch_capsule = Box::new(Orientation::Horizontal, 0);
            touch_capsule.set_hexpand(false);
            touch_capsule.set_vexpand(false);
            touch_capsule.set_size_request(strip_width, strip_height);

            let mut touch_segments = Vec::new();
            let mut touch_labels = Vec::new();
            let mut touch_icons = Vec::new();

            for slot_idx in 0..dial_count {
                if slot_idx > 0 {
                    let sep = Box::new(Orientation::Vertical, 0);
                    sep.add_css_class("deck-touch-separator");
                    touch_capsule.append(&sep);
                }

                let slot_box = Box::new(Orientation::Vertical, 2);
                slot_box.add_css_class("deck-touch-slot");
                slot_box.add_css_class(&format!("deck-touch-slot-{}", slot_idx));
                slot_box.set_hexpand(false);
                slot_box.set_vexpand(false);
                slot_box.set_size_request(btn_size, strip_height);

                let slot_icon = Picture::new();
                slot_icon.set_can_shrink(true);
                slot_icon.set_content_fit(gtk::ContentFit::Contain);
                slot_icon.set_size_request(20, 20);
                slot_icon.set_visible(false);
                slot_icon.set_can_target(false);

                let slot_label = Label::new(None);
                slot_label.add_css_class("deck-touch-slot-label");
                slot_label.set_halign(gtk::Align::Center);
                slot_label.set_valign(gtk::Align::Center);
                slot_label.set_vexpand(true);
                slot_label.set_can_target(false);

                slot_box.append(&slot_icon);
                slot_box.append(&slot_label);

                // Drag and Drop Target for Actions dropped onto Dial Touchscreen Slot
                let drop_target = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::COPY);
                let dropped_cb = self.on_action_dropped.clone();
                let d_slot = slot_idx;
                drop_target.connect_drop(move |_target, value, _x, _y| {
                    if let Ok(json_str) = value.get::<String>() {
                        if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                            if let Some(ref cb) = *dropped_cb.borrow() {
                                cb(crate::ui::inspector::InspectorTarget::Dial(d_slot), action);
                                return true;
                            }
                        }
                    }
                    false
                });
                slot_box.add_controller(drop_target);

                touch_capsule.append(&slot_box);
                touch_segments.push(slot_box);
                touch_labels.push(slot_label);
                touch_icons.push(slot_icon);
            }

            touch_overlay.add_overlay(&touch_capsule);

            // Click controller on touch_overlay:
            // "when the touchscreen is clicks, no matter where, the whole screen is selected as one whole button"
            let capsule_click = gtk::GestureClick::new();
            let on_capsule_click_ref = self.on_touch_capsule_clicked.clone();
            capsule_click.connect_pressed(move |_g, _n, _x, _y| {
                if let Some(ref cb) = *on_capsule_click_ref.borrow() {
                    cb();
                }
            });
            touch_overlay.add_controller(capsule_click);

            // Drag and Drop Target for Actions dropped onto Touchscreen Panoramic
            let touch_drop = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::COPY);
            let dropped_cb_touch = self.on_action_dropped.clone();
            touch_drop.connect_drop(move |_target, value, _x, _y| {
                if let Ok(json_str) = value.get::<String>() {
                    if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                        if let Some(ref cb) = *dropped_cb_touch.borrow() {
                            cb(crate::ui::inspector::InspectorTarget::Touchscreen, action);
                            return true;
                        }
                    }
                }
                false
            });
            touch_overlay.add_controller(touch_drop);

            grid.attach(&touch_overlay, 0, rows as i32, cols as i32, 1);
            *self.touch_segments.borrow_mut() = touch_segments;
            *self.touch_labels.borrow_mut() = touch_labels;
            *self.touch_icons.borrow_mut() = touch_icons;
            *self.touch_capsule.borrow_mut() = Some(touch_capsule);
            *self.touch_overlay.borrow_mut() = Some(touch_overlay);
            *self.touch_wallpaper_pic.borrow_mut() = Some(touch_wallpaper_pic);

            // Row `rows + 1`: Rotary Dials centered directly under column 0..cols!
            let dial_row = (rows + 1) as i32;
            let mut dial_draw_areas = Vec::new();
            let mut dial_angles = Vec::new();

            for dial_idx in 0..dial_count {
                let dial_wrapper = Box::new(Orientation::Vertical, 0);
                dial_wrapper.set_halign(gtk::Align::Center);
                dial_wrapper.set_valign(gtk::Align::Center);
                dial_wrapper.set_size_request(btn_size, btn_size);

                // Drag and Drop Target for Actions dropped onto Rotary Dial
                let dial_drop = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::COPY);
                let dropped_cb_dial = self.on_action_dropped.clone();
                let d_num = dial_idx;
                dial_drop.connect_drop(move |_target, value, _x, _y| {
                    if let Ok(json_str) = value.get::<String>() {
                        if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                            if let Some(ref cb) = *dropped_cb_dial.borrow() {
                                cb(crate::ui::inspector::InspectorTarget::Dial(d_num), action);
                                return true;
                            }
                        }
                    }
                    false
                });
                dial_wrapper.add_controller(dial_drop);

                let da = DrawingArea::new();
                da.set_size_request(62, 62);
                da.set_content_width(62);
                da.set_content_height(62);
                da.set_halign(gtk::Align::Center);
                da.set_valign(gtk::Align::Center);
                da.set_cursor_from_name(Some("grab"));

                let angle_cell = Rc::new(Cell::new(0.0f64));
                dial_angles.push(angle_cell.clone());

                let is_hovered = Rc::new(Cell::new(false));
                let selected_dial_ref = self.selected_dial_idx.clone();
                let angle_draw = angle_cell.clone();
                let is_hovered_draw = is_hovered.clone();
                let d_idx = dial_idx;

                da.set_draw_func(move |_area, cr, width, height| {
                    let w = width as f64;
                    let h = height as f64;
                    let cx = w / 2.0;
                    let cy = h / 2.0;
                    let r = (w.min(h) / 2.0 - 5.0).max(10.0);
                    let angle = angle_draw.get();
                    let is_sel = selected_dial_ref.borrow().map_or(false, |sel| sel == d_idx);
                    let is_hov = is_hovered_draw.get();

                    // 1. Selection outer glow ring
                    if is_sel {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.40); // #9146ff glow
                        cr.set_line_width(6.0);
                        cr.arc(cx, cy, r + 2.0, 0.0, 2.0 * std::f64::consts::PI);
                        let _ = cr.stroke();
                    }

                    // 2. Knob body with metallic radial gradient (simulating ambient lighting from top)
                    let pat = cairo::RadialGradient::new(cx, cy - r * 0.22, r * 0.08, cx, cy, r);
                    pat.add_color_stop_rgb(0.0, 0.16, 0.16, 0.22);
                    pat.add_color_stop_rgb(0.70, 0.11, 0.11, 0.15);
                    pat.add_color_stop_rgb(1.0, 0.07, 0.07, 0.095);
                    let _ = cr.set_source(&pat);
                    cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.fill();

                    // 3. Knob circular border
                    if is_sel {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.95); // #9146ff solid rim
                        cr.set_line_width(2.2);
                    } else if is_hov {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.55);
                        cr.set_line_width(1.8);
                    } else {
                        cr.set_source_rgba(0.20, 0.20, 0.26, 0.9);
                        cr.set_line_width(1.5);
                    }
                    cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.stroke();

                    // 4. Inset shadow groove ring
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.5);
                    cr.set_line_width(1.0);
                    cr.arc(cx, cy, (r - 2.5).max(1.0), 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.stroke();

                    // 5. Rotated Marker Notch (Exact match to sleek purple marker)
                    let _ = cr.save();
                    cr.translate(cx, cy);
                    cr.rotate(angle);

                    let notch_w = (r * 0.13).clamp(3.0, 4.5);
                    let y_top = -r * 0.82;
                    let y_bottom = -r * 0.52;

                    // Neon violet glow around notch
                    cr.set_source_rgba(0.66, 0.33, 0.97, 0.35);
                    cr.set_line_cap(cairo::LineCap::Round);
                    cr.set_line_width(notch_w + 3.0);
                    cr.move_to(0.0, y_top);
                    cr.line_to(0.0, y_bottom);
                    let _ = cr.stroke();

                    // Bright core notch
                    cr.set_source_rgb(0.72, 0.40, 1.0); // #b866ff
                    cr.set_line_width(notch_w);
                    cr.move_to(0.0, y_top);
                    cr.line_to(0.0, y_bottom);
                    let _ = cr.stroke();

                    let _ = cr.restore();
                });

                // Hover Controller
                let motion = gtk::EventControllerMotion::new();
                let is_hov_enter = is_hovered.clone();
                let da_hov_enter = da.clone();
                motion.connect_enter(move |_m, _x, _y| {
                    is_hov_enter.set(true);
                    da_hov_enter.set_cursor_from_name(Some("grab"));
                    da_hov_enter.queue_draw();
                });
                let is_hov_leave = is_hovered.clone();
                let da_hov_leave = da.clone();
                motion.connect_leave(move |_m| {
                    is_hov_leave.set(false);
                    da_hov_leave.queue_draw();
                });
                da.add_controller(motion);

                // Click Controller (Selects Dial)
                let click = gtk::GestureClick::new();
                let dial_selected_click = self.selected_dial_idx.clone();
                let on_click_ref = self.on_dial_clicked.clone();
                let da_click = da.clone();
                click.connect_pressed(move |_g, _n, _x, _y| {
                    *dial_selected_click.borrow_mut() = Some(d_idx);
                    da_click.queue_draw();
                    if let Some(ref cb) = *on_click_ref.borrow() {
                        cb(d_idx);
                    }
                });
                da.add_controller(click);

                // Mouse Drag Gesture (Rotates Dial)
                let drag = gtk::GestureDrag::new();
                let start_pos_cell = Rc::new(Cell::new((0.0f64, 0.0f64)));
                let last_pos_cell = Rc::new(Cell::new((0.0f64, 0.0f64)));
                let da_drag_begin = da.clone();
                let dial_selected_drag = self.selected_dial_idx.clone();
                let on_click_drag = self.on_dial_clicked.clone();
                let start_pos_begin = start_pos_cell.clone();
                let last_pos_begin = last_pos_cell.clone();

                drag.connect_drag_begin(move |_g, start_x, start_y| {
                    da_drag_begin.set_cursor_from_name(Some("grabbing"));
                    *dial_selected_drag.borrow_mut() = Some(d_idx);
                    da_drag_begin.queue_draw();
                    if let Some(ref cb) = *on_click_drag.borrow() {
                        cb(d_idx);
                    }
                    start_pos_begin.set((start_x, start_y));
                    last_pos_begin.set((start_x, start_y));
                });

                let start_pos_update = start_pos_cell.clone();
                let last_pos_update = last_pos_cell.clone();
                let angle_update = angle_cell.clone();
                let da_drag_update = da.clone();
                let accum_update = Rc::new(Cell::new(0.0f64));
                let rotate_cb_update = self.on_dial_rotate.clone();

                drag.connect_drag_update(move |_g, offset_x, offset_y| {
                    let (start_x, start_y) = start_pos_update.get();
                    let cur_x = start_x + offset_x;
                    let cur_y = start_y + offset_y;
                    let (prev_x, prev_y) = last_pos_update.get();
                    last_pos_update.set((cur_x, cur_y));

                    let w = da_drag_update.width() as f64;
                    let h = da_drag_update.height() as f64;
                    let cx = w / 2.0;
                    let cy = h / 2.0;

                    let cur_polar = (cur_y - cy).atan2(cur_x - cx);
                    let prev_polar = (prev_y - cy).atan2(prev_x - cx);
                    let mut polar_delta = cur_polar - prev_polar;
                    while polar_delta > std::f64::consts::PI { polar_delta -= 2.0 * std::f64::consts::PI; }
                    while polar_delta < -std::f64::consts::PI { polar_delta += 2.0 * std::f64::consts::PI; }

                    // Linear drag: dragging up/right rotates clockwise, down/left counter-clockwise
                    let linear_delta = (-(cur_y - prev_y) * 0.04) + ((cur_x - prev_x) * 0.02);

                    let dist = ((cur_x - cx).powi(2) + (cur_y - cy).powi(2)).sqrt();
                    let delta = if dist >= 12.0 && polar_delta.abs() > 0.005 && polar_delta.abs() < 1.2 {
                        polar_delta
                    } else {
                        linear_delta
                    };

                    let mut new_a = angle_update.get() + delta;
                    while new_a >= 2.0 * std::f64::consts::PI { new_a -= 2.0 * std::f64::consts::PI; }
                    while new_a < 0.0 { new_a += 2.0 * std::f64::consts::PI; }
                    angle_update.set(new_a);
                    da_drag_update.queue_draw();

                    // Step threshold: approx 15 degrees (~0.2618 rad) per rotary step
                    let step_rad = std::f64::consts::PI / 12.0;
                    let acc = accum_update.get() + delta;
                    if acc.abs() >= step_rad {
                        let steps = (acc / step_rad).trunc() as i32;
                        accum_update.set(acc - (steps as f64 * step_rad));
                        if let Some(ref cb) = *rotate_cb_update.borrow() {
                            cb(d_idx, steps);
                        }
                    }
                });

                let da_drag_end = da.clone();
                drag.connect_drag_end(move |_g, _offset_x, _offset_y| {
                    da_drag_end.set_cursor_from_name(Some("grab"));
                });
                da.add_controller(drag);

                // Mouse Scroll Wheel Controller
                let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
                let angle_scroll = angle_cell.clone();
                let da_scroll = da.clone();
                let rotate_cb_scroll = self.on_dial_rotate.clone();

                scroll.connect_scroll(move |_c, _dx, dy| {
                    let step = if dy < 0.0 { 1 } else { -1 };
                    let delta_angle = (step as f64) * (std::f64::consts::PI / 12.0);
                    let mut new_a = angle_scroll.get() + delta_angle;
                    while new_a >= 2.0 * std::f64::consts::PI { new_a -= 2.0 * std::f64::consts::PI; }
                    while new_a < 0.0 { new_a += 2.0 * std::f64::consts::PI; }
                    angle_scroll.set(new_a);
                    da_scroll.queue_draw();

                    if let Some(ref cb) = *rotate_cb_scroll.borrow() {
                        cb(d_idx, step);
                    }
                    glib::Propagation::Stop
                });
                da.add_controller(scroll);

                dial_wrapper.append(&da);
                grid.attach(&dial_wrapper, dial_idx as i32, dial_row, 1, 1);
                dial_draw_areas.push(da);
            }
            *self.dial_draw_areas.borrow_mut() = dial_draw_areas;
            *self.dial_angles.borrow_mut() = dial_angles;
        } else {
            *self.touch_capsule.borrow_mut() = None;
        }

        self.hardware_area.append(&grid);
        *self.grid.borrow_mut() = Some(grid);
        *self.key_buttons.borrow_mut() = key_buttons;
        *self.key_labels.borrow_mut() = key_labels;
        *self.key_icons.borrow_mut() = key_icons;
        *self.key_overlays.borrow_mut() = key_overlays;
    }

    pub fn set_dial_rotate_callback<F: Fn(usize, i32) + 'static>(&self, f: F) {
        *self.on_dial_rotate.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_dial_click_callback<F: Fn(usize) + 'static>(&self, f: F) {
        *self.on_dial_clicked.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn rotate_dial(&self, dial_idx: usize, delta_steps: i32) {
        let angles = self.dial_angles.borrow();
        if let Some(angle_cell) = angles.get(dial_idx) {
            let delta_angle = (delta_steps as f64) * (std::f64::consts::PI / 12.0);
            let mut a = angle_cell.get() + delta_angle;
            while a >= 2.0 * std::f64::consts::PI { a -= 2.0 * std::f64::consts::PI; }
            while a < 0.0 { a += 2.0 * std::f64::consts::PI; }
            angle_cell.set(a);

            let areas = self.dial_draw_areas.borrow();
            if let Some(da) = areas.get(dial_idx) {
                da.queue_draw();
            }
        }
    }

    /// Proportionally scales the hardware canvas based on available window height while strictly preserving aspect ratios
    pub fn update_scale(&self, available_h: i32) {
        let kind = *self.current_kind.borrow();
        let (rows, cols) = kind.grid_dimensions();
        let row_count = if kind.has_dials() {
            rows as f64 + 1.8
        } else {
            rows as f64 + 0.6
        };

        let target_size = (((available_h - 110) as f64 / row_count) as i32).clamp(74, 115);
        if target_size == self.current_btn_size.get() {
            return;
        }
        self.current_btn_size.set(target_size);

        let spacing = (target_size as f64 * 0.14).clamp(8.0, 16.0) as u32;
        if let Some(ref g) = *self.grid.borrow() {
            g.set_row_spacing(spacing);
            g.set_column_spacing(spacing);
        }

        for btn in self.key_buttons.borrow().iter() {
            btn.set_size_request(target_size, target_size);
        }
        for ov in self.key_overlays.borrow().iter() {
            ov.set_size_request(target_size, target_size);
        }

        let strip_w = cols as i32 * target_size + (cols as i32 - 1) * spacing as i32;
        let strip_h = (target_size as f64 * 0.60).clamp(44.0, 68.0) as i32;
        if let Some(ref to) = *self.touch_overlay.borrow() {
            to.set_size_request(strip_w, strip_h);
        }
        if let Some(ref tc) = *self.touch_capsule.borrow() {
            tc.set_size_request(strip_w, strip_h);
        }
        if let Some(ref tw) = *self.touch_wallpaper_pic.borrow() {
            tw.set_size_request(strip_w, strip_h);
        }
        for t_btn in self.touch_segments.borrow().iter() {
            t_btn.set_size_request(target_size, strip_h);
        }

        let dial_size = (target_size as f64 * 0.72).clamp(52.0, 82.0) as i32;
        for da in self.dial_draw_areas.borrow().iter() {
            da.set_size_request(dial_size, dial_size);
            da.set_content_width(dial_size);
            da.set_content_height(dial_size);
            da.queue_draw();
        }
    }

    pub fn set_touch_capsule_click_callback<F: Fn() + 'static>(&self, f: F) {
        *self.on_touch_capsule_clicked.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn select_key(&self, index: usize) {
        let keys = self.key_buttons.borrow();
        for (i, btn) in keys.iter().enumerate() {
            if i == index {
                btn.add_css_class("selected");
            } else {
                btn.remove_css_class("selected");
            }
        }
        for seg in self.touch_segments.borrow().iter() {
            seg.remove_css_class("selected");
        }
        if let Some(ref tc) = *self.touch_capsule.borrow() {
            tc.remove_css_class("selected");
        }
        if let Some(ref to) = *self.touch_overlay.borrow() {
            to.remove_css_class("selected");
        }
        *self.selected_dial_idx.borrow_mut() = None;
        for da in self.dial_draw_areas.borrow().iter() {
            da.queue_draw();
        }
        *self.selected_idx.borrow_mut() = Some(index);
    }

    /// Highlights Dial `dial_idx` and its designated Touch Card directly above it together
    pub fn select_dial_and_touch(&self, dial_idx: usize) {
        // Clear key selection
        for btn in self.key_buttons.borrow().iter() {
            btn.remove_css_class("selected");
        }
        *self.selected_idx.borrow_mut() = None;

        if let Some(ref tc) = *self.touch_capsule.borrow() {
            tc.remove_css_class("selected");
        }
        if let Some(ref to) = *self.touch_overlay.borrow() {
            to.remove_css_class("selected");
        }

        // Highlight Touch Card
        for (i, seg) in self.touch_segments.borrow().iter().enumerate() {
            if i == dial_idx {
                seg.add_css_class("selected");
            } else {
                seg.remove_css_class("selected");
            }
        }

        // Highlight Rotary Dial
        *self.selected_dial_idx.borrow_mut() = Some(dial_idx);
        for da in self.dial_draw_areas.borrow().iter() {
            da.queue_draw();
        }
    }

    /// Highlights the entire Touchscreen as one unified Panoramic Button
    pub fn select_touchscreen_panoramic(&self) {
        for btn in self.key_buttons.borrow().iter() {
            btn.remove_css_class("selected");
        }
        *self.selected_idx.borrow_mut() = None;

        for seg in self.touch_segments.borrow().iter() {
            seg.remove_css_class("selected");
        }

        *self.selected_dial_idx.borrow_mut() = None;
        for da in self.dial_draw_areas.borrow().iter() {
            da.queue_draw();
        }

        if let Some(ref tc) = *self.touch_capsule.borrow() {
            tc.add_css_class("selected");
        }
        if let Some(ref to) = *self.touch_overlay.borrow() {
            to.add_css_class("selected");
        }
    }

    pub fn select_dial(&self, index: usize) {
        self.select_dial_and_touch(index);
    }

    /// Renders profile page data and dynamically rebuilds the animated page pill chips
    pub fn render_profile_page(&self, profile: &Profile) {
        let total_pages = profile.pages.len();
        let cur_idx = profile.current_page_idx;

        // Rebuild dynamic page chips [ (1) ] [ 2 ] [ 3 ] ...
        while let Some(child) = self.page_chips_box.first_child() {
            self.page_chips_box.remove(&child);
        }
        self.page_chips.borrow_mut().clear();

        for i in 0..total_pages {
            let chip = Button::with_label(&format!("{}", i + 1));
            chip.add_css_class("page-chip");
            if i == cur_idx {
                chip.add_css_class("active");
            }

            let cb_ref = self.on_page_selected.clone();
            chip.connect_clicked(move |_| {
                if let Some(ref cb) = *cb_ref.borrow() {
                    cb(i);
                }
            });

            self.page_chips_box.append(&chip);
            self.page_chips.borrow_mut().push(chip);
        }

        self.add_page_btn.set_sensitive(total_pages < 10);
        self.remove_page_btn.set_sensitive(total_pages > 1);

        let mut css = String::new();
        if let Some(page) = profile.current_page() {
            let keys = self.key_buttons.borrow();
            let labels = self.key_labels.borrow();
            let icons = self.key_icons.borrow();

            // 1. Primary LCD Keys
            for (idx, _btn) in keys.iter().enumerate() {
                if let Some(action) = page.keys.get(&idx) {
                    let display_title = if !action.appearance.show_title || action.appearance.title.is_empty() {
                        ""
                    } else {
                        &action.appearance.title
                    };

                    if let Some(label) = labels.get(idx) {
                        label.set_text(display_title);
                        label.set_visible(!display_title.is_empty());
                        match action.appearance.vertical_align {
                            TextVerticalAlign::Top => label.set_valign(gtk::Align::Start),
                            TextVerticalAlign::Center => label.set_valign(gtk::Align::Center),
                            TextVerticalAlign::Bottom => label.set_valign(gtk::Align::End),
                        }
                    }

                    if let Some(src) = self.active_ui_animations.borrow_mut().remove(&idx) {
                        src.remove();
                    }

                    if let Some(icon) = icons.get(idx) {
                        let btn_sz = self.current_btn_size.get() as f64;
                        let scale_pct = (action.appearance.icon_scale.clamp(20, 100) as f64) / 100.0;
                        let icon_sz = ((btn_sz * scale_pct) as i32).max(12);

                        let icon_path = action.appearance.icon_path.as_deref();
                        let icon_name = action.parameters.get("icon_name").map(|s| s.as_str())
                            .or_else(|| match action.action_id.as_str() {
                                "system.hotkey" => Some("input-keyboard-symbolic"),
                                "system.url" => Some("web-browser-symbolic"),
                                "system.terminal" => Some("utilities-terminal-symbolic"),
                                "app_launcher.launch" => Some("system-run"),
                                _ => None,
                            });

                        if let Some(path) = icon_path {
                            if path.to_lowercase().ends_with(".gif") && std::path::Path::new(path).exists() {
                                // 1. Immediately show first frame with zero UI hitch
                                if let Some(texture) = create_rounded_texture(path, icon_sz, icon_sz, 11.0, true) {
                                    icon.set_paintable(Some(&texture));
                                    icon.set_size_request(icon_sz, icon_sz);
                                    icon.set_visible(true);
                                }

                                // 2. Decode animation frames in background thread via async_channel
                                let (sender, receiver) = async_channel::unbounded::<Vec<crate::core::animation::RawGifFrame>>();
                                let pic_clone = icon.clone();
                                let active_anims = self.active_ui_animations.clone();
                                let key_idx = idx;

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

                                            if let Some(old) = active_anims.borrow_mut().insert(key_idx, source_id) {
                                                old.remove();
                                            }
                                        }
                                    }
                                });

                                let path_clone = path.to_string();
                                let dim = icon_sz as u32;
                                std::thread::spawn(move || {
                                    if let Some(raw_frames) = crate::core::animation::GifAnimation::load_key_gif_raw(&path_clone, dim, 11.0) {
                                        let _ = sender.send_blocking(raw_frames);
                                    }
                                });
                            } else if let Some(paintable) = lookup_icon_paintable(Some(path), icon_name, icon_sz, 11.0) {
                                icon.set_paintable(Some(&paintable));
                                icon.set_size_request(icon_sz, icon_sz);
                                icon.set_visible(true);
                            } else {
                                icon.set_visible(false);
                            }
                        } else if let Some(paintable) = lookup_icon_paintable(None, icon_name, icon_sz, 11.0) {
                            icon.set_paintable(Some(&paintable));
                            icon.set_size_request(icon_sz, icon_sz);
                            icon.set_visible(true);
                        } else {
                            icon.set_visible(false);
                        }
                    }

                    let bg = action.appearance.bg_color.as_deref().unwrap_or("#191920");
                    let fg = &action.appearance.text_color;
                    let font = &action.appearance.font_family;
                    let size = action.appearance.font_size;
                    css.push_str(&format!(
                        "button.deck-key.deck-key-slot-{} {{ background-color: {}; background-image: none; }}\n\
                         button.deck-key.deck-key-slot-{} label {{ color: {}; font-family: '{}'; font-size: {}pt; font-weight: bold; }}\n",
                        idx, bg, idx, fg, font, size
                    ));
                } else {
                    if let Some(label) = labels.get(idx) {
                        label.set_text("");
                        label.set_visible(false);
                    }
                    if let Some(icon) = icons.get(idx) {
                        icon.set_visible(false);
                    }
                    css.push_str(&format!(
                        "button.deck-key.deck-key-slot-{} {{ background-color: #191920; background-image: none; }}\n",
                        idx
                    ));
                }
            }

            // 2. Touchscreen & Dial Slots Rendering
            let touch_labels = self.touch_labels.borrow();
            let touch_icons = self.touch_icons.borrow();
            let touch_segs = self.touch_segments.borrow();

            let has_panoramic = page.touchscreen_action.as_ref().map_or(false, |act| {
                !act.appearance.title.is_empty()
                    || act.appearance.bg_color.is_some()
                    || act.appearance.icon_path.is_some()
            });

            // Update Touch Strip Wallpaper if configured
            if let Some(ref pic) = *self.touch_wallpaper_pic.borrow() {
                let wallpaper_path = page.touchscreen_action.as_ref().and_then(|a| a.appearance.icon_path.as_deref());
                if let Some(path) = wallpaper_path {
                    if std::path::Path::new(path).exists() {
                        let (_rows, cols) = self.current_kind.borrow().grid_dimensions();
                        let btn_size = self.current_btn_size.get();
                        let spacing = (btn_size as f64 * 0.14).clamp(8.0, 16.0) as u32;
                        let strip_w = cols as i32 * btn_size + (cols as i32 - 1) * spacing as i32;
                        let strip_h = (btn_size as f64 * 0.60).clamp(44.0, 68.0) as i32;

                        if path.to_lowercase().ends_with(".gif") {
                            pic.set_filename(Some(path));
                        } else if let Some(texture) = create_rounded_texture(path, strip_w, strip_h, 11.0, false) {
                            pic.set_paintable(Some(&texture));
                        } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, strip_w, strip_h, false) {
                            let texture = gtk::gdk::Texture::for_pixbuf(&pb);
                            pic.set_paintable(Some(&texture));
                        } else {
                            pic.set_filename(Some(path));
                        }
                        pic.set_size_request(strip_w, strip_h);
                        pic.set_visible(true);
                    } else {
                        pic.set_visible(false);
                    }
                } else {
                    pic.set_visible(false);
                }
            }

            if has_panoramic {
                if let Some(ref act) = page.touchscreen_action {
                    let bg = act.appearance.bg_color.as_deref().unwrap_or("transparent");
                    css.push_str(&format!(
                        ".deck-touchscreen-capsule {{ background-color: {}; }}\n",
                        bg
                    ));
                    if let Some(label) = touch_labels.first() {
                        label.set_text(&act.appearance.title);
                        label.set_visible(!act.appearance.title.is_empty());
                    }
                    for label in touch_labels.iter().skip(1) {
                        label.set_text("");
                        label.set_visible(false);
                    }
                    for icon in touch_icons.iter() {
                        icon.set_visible(false);
                    }
                }
            } else {
                for (idx, _seg) in touch_segs.iter().enumerate() {
                    if let Some(action) = page.dial_actions.get(&idx) {
                        let display_title = if !action.appearance.show_title || action.appearance.title.is_empty() {
                            ""
                        } else {
                            &action.appearance.title
                        };

                        if let Some(label) = touch_labels.get(idx) {
                            label.set_text(display_title);
                            label.set_visible(!display_title.is_empty());
                            match action.appearance.vertical_align {
                                TextVerticalAlign::Top => label.set_valign(gtk::Align::Start),
                                TextVerticalAlign::Center => label.set_valign(gtk::Align::Center),
                                TextVerticalAlign::Bottom => label.set_valign(gtk::Align::End),
                            }
                        }

                        if let Some(icon) = touch_icons.get(idx) {
                            let scale_pct = (action.appearance.icon_scale.clamp(20, 100) as f64) / 100.0;
                            let base_sz = if display_title.is_empty() { 36.0 } else { 22.0 };
                            let icon_sz = ((base_sz * scale_pct) as i32).max(10);

                            let icon_path = action.appearance.icon_path.as_deref();
                            let icon_name = action.parameters.get("icon_name").map(|s| s.as_str())
                                .or_else(|| match action.action_id.as_str() {
                                    "system.hotkey" => Some("input-keyboard-symbolic"),
                                    "system.url" => Some("web-browser-symbolic"),
                                    "system.terminal" => Some("utilities-terminal-symbolic"),
                                    "app_launcher.launch" => Some("system-run"),
                                    _ => None,
                                });

                            if let Some(paintable) = lookup_icon_paintable(icon_path, icon_name, icon_sz, 6.0) {
                                icon.set_paintable(Some(&paintable));
                                icon.set_size_request(icon_sz, icon_sz);
                                icon.set_visible(true);
                            } else {
                                icon.set_visible(false);
                            }
                        }

                        let bg = action.appearance.bg_color.as_deref().unwrap_or("transparent");
                        let fg = &action.appearance.text_color;
                        let font = &action.appearance.font_family;
                        let size = action.appearance.font_size;
                        css.push_str(&format!(
                            ".deck-touch-slot.deck-touch-slot-{} {{ background-color: {}; }}\n\
                             .deck-touch-slot.deck-touch-slot-{} label {{ color: {}; font-family: '{}'; font-size: {}pt; font-weight: bold; }}\n",
                            idx, bg, idx, fg, font, size
                        ));
                    } else {
                        if let Some(label) = touch_labels.get(idx) {
                            label.set_text("");
                            label.set_visible(false);
                        }
                        if let Some(icon) = touch_icons.get(idx) {
                            icon.set_visible(false);
                        }
                        css.push_str(&format!(
                            ".deck-touch-slot.deck-touch-slot-{} {{ background-color: transparent; }}\n",
                            idx
                        ));
                    }
                }
            }
        }
        self.css_provider.load_from_data(&css);
    }
}
