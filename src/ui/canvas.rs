use crate::models::{DeviceKind, Profile, TextVerticalAlign};
use gio::prelude::FileExt;
use gtk::cairo;
use gtk::prelude::*;
use gtk::{Box, Button, CenterBox, DrawingArea, Grid, Label, Orientation, Overlay, Picture};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub fn resolve_icon_path(icon_name_or_path: &str, size: i32) -> Option<String> {
    if icon_name_or_path.is_empty() {
        return None;
    }
    // 1. Direct path check (handles AppImage, Snap, and user absolute icon paths)
    let p = std::path::Path::new(icon_name_or_path);
    if p.is_absolute() && p.exists() {
        return Some(icon_name_or_path.to_string());
    }

    let raw_name = p.file_name().and_then(|n| n.to_str()).unwrap_or(icon_name_or_path);
    let clean_name = raw_name
        .strip_suffix(".svg")
        .or_else(|| raw_name.strip_suffix(".png"))
        .or_else(|| raw_name.strip_suffix(".xpm"))
        .unwrap_or(raw_name);

    // 2. GtkIconTheme lookup (main-thread only — augment search paths with Flatpak & Snap icon roots!)
    if gtk::is_initialized_main_thread() {
        if let Some(display) = gtk::gdk::Display::default() {
            let theme = gtk::IconTheme::for_display(&display);
            let home = std::env::var("HOME").unwrap_or_default();
            let extra_roots = [
                format!("{home}/.local/share/flatpak/exports/share/icons"),
                "/var/lib/flatpak/exports/share/icons".to_string(),
                "/var/lib/snapd/desktop/icons".to_string(),
                format!("{home}/.local/share/icons"),
            ];
            let existing_paths = theme.search_path();
            for root in &extra_roots {
                let rp = std::path::Path::new(root);
                if rp.exists() && !existing_paths.iter().any(|ep| ep == rp) {
                    theme.add_search_path(rp);
                }
            }

            if theme.has_icon(clean_name) {
                let paintable = theme.lookup_icon(clean_name, &[], size, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
                if let Some(file) = paintable.file() {
                    if let Some(fp) = file.path() {
                        if fp.exists() {
                            return Some(fp.to_string_lossy().to_string());
                        }
                    }
                }
            }
            let short = clean_name.rsplit('.').next().unwrap_or(clean_name);
            if short != clean_name && theme.has_icon(short) {
                let paintable = theme.lookup_icon(short, &[], size, 1, gtk::TextDirection::None, gtk::IconLookupFlags::empty());
                if let Some(file) = paintable.file() {
                    if let Some(fp) = file.path() {
                        if fp.exists() {
                            return Some(fp.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    // 3. Comprehensive filesystem search across Flatpak (user & system), User/AppImage, Snap, Local, and System icon roots
    let home = std::env::var("HOME").unwrap_or_default();
    let icon_roots = [
        format!("{home}/.local/share/flatpak/exports/share/icons"),
        "/var/lib/flatpak/exports/share/icons".to_string(),
        format!("{home}/.local/share/icons"),
        "/var/lib/snapd/desktop/icons".to_string(),
        "/usr/local/share/icons".to_string(),
        "/usr/share/icons".to_string(),
    ];
    let subdirs = [
        "hicolor/scalable/apps",
        "hicolor/512x512/apps",
        "hicolor/256x256/apps",
        "hicolor/128x128/apps",
        "hicolor/96x96/apps",
        "hicolor/64x64/apps",
        "hicolor/48x48/apps",
    ];
    let exts = ["svg", "png"];

    for root in &icon_roots {
        if !std::path::Path::new(root).exists() {
            continue;
        }
        // Check direct files inside ~/.local/share/icons (common for Gear Lever / AppImage icons)
        for ext in &exts {
            let direct = format!("{root}/{clean_name}.{ext}");
            if std::path::Path::new(&direct).exists() {
                return Some(direct);
            }
        }
        for sub in &subdirs {
            for ext in &exts {
                let cand = format!("{root}/{sub}/{clean_name}.{ext}");
                if std::path::Path::new(&cand).exists() {
                    return Some(cand);
                }
            }
        }
    }

    let pixmap_candidates = [
        format!("{home}/.cache/streamcontroller_apps/icons/{clean_name}.png"),
        format!("{home}/.cache/streamcontroller_apps/icons/{clean_name}.svg"),
        format!("/usr/share/pixmaps/{clean_name}.svg"),
        format!("/usr/share/pixmaps/{clean_name}.png"),
        format!("/usr/share/pixmaps/{raw_name}"),
    ];
    for cand in &pixmap_candidates {
        if std::path::Path::new(cand).exists() {
            return Some(cand.clone());
        }
    }

    None
}

pub fn create_rounded_texture(path: &str, width: i32, height: i32, radius: f64, preserve_aspect: bool) -> Option<gtk::gdk::Texture> {
    use gtk::gdk::prelude::GdkCairoContextExt;
    let is_svg = path.to_lowercase().ends_with(".svg");
    let pb = if is_svg {
        // Rasterize vector artwork at crisp 2x HiDPI resolution
        gdk_pixbuf::Pixbuf::from_file_at_scale(path, (width * 2).max(16), (height * 2).max(16), preserve_aspect).ok()?
    } else if !preserve_aspect {
        let raw = gdk_pixbuf::Pixbuf::from_file(path).ok()?;
        let tw = (width * 2).max(16).min(raw.width().max(width));
        let th = (height * 2).max(16).min(raw.height().max(height));
        if raw.width() == tw && raw.height() == th {
            raw
        } else {
            raw.scale_simple(tw, th, gdk_pixbuf::InterpType::Hyper)?
        }
    } else {
        // Preserve native raster resolution (up to 2x target) so crisp 200x100 dial cards
        // and high-res icons are NEVER downsampled to blurry low-res bitmaps!
        let raw = gdk_pixbuf::Pixbuf::from_file(path).ok()?;
        let max_w = (width * 2).max(200);
        let max_h = (height * 2).max(100);
        if raw.width() <= max_w && raw.height() <= max_h {
            raw
        } else {
            let scale = (max_w as f64 / raw.width() as f64).min(max_h as f64 / raw.height() as f64);
            let nw = (raw.width() as f64 * scale).round().max(1.0) as i32;
            let nh = (raw.height() as f64 * scale).round().max(1.0) as i32;
            raw.scale_simple(nw, nh, gdk_pixbuf::InterpType::Hyper)?
        }
    };
    let pw = pb.width();
    let ph = pb.height();

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, pw, ph).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;

    if radius > 0.0 {
        let scale_r = if width > 0 { (pw as f64 / width as f64).max(1.0) } else { 1.0 };
        let r = (radius * scale_r).min((pw.min(ph) as f64) / 2.0);
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

/// Resolves a rectangular (e.g. 2:1 dial card) paintable at crisp HiDPI resolution
pub fn lookup_rect_paintable(
    icon_path: Option<&str>,
    icon_name: Option<&str>,
    width: i32,
    height: i32,
    radius: f64,
) -> Option<gtk::gdk::Paintable> {
    if let Some(path) = icon_path {
        if std::path::Path::new(path).exists() {
            if let Some(texture) = create_rounded_texture(path, width, height, radius, true) {
                return Some(texture.upcast());
            } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file(path) {
                return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
            }
        }
    }
    lookup_icon_paintable(None, icon_name, width.min(height).max(24), radius)
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
            // Check resolve_icon_path (resolves via GtkIconTheme, theme cache, or system directories)
            if let Some(resolved) = resolve_icon_path(name, size) {
                if let Some(texture) = create_rounded_texture(&resolved, size, size, radius, true) {
                    return Some(texture.upcast());
                } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(&resolved, size, size, true) {
                    return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
                }
            }
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
            // Fallback to AppLauncher default_app.png if installed
            let fallback_app = glib::user_data_dir()
                .join("ballista")
                .join("plugins")
                .join("com_oparada_AppLauncher")
                .join("assets")
                .join("default_app.png");
            if fallback_app.exists() {
                let fallback_str = fallback_app.to_string_lossy();
                if let Some(texture) = create_rounded_texture(&fallback_str, size, size, radius, true) {
                    return Some(texture.upcast());
                } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(&fallback_app, size, size, true) {
                    return Some(gtk::gdk::Texture::for_pixbuf(&pb).upcast());
                }
            }
        }
    }
    None
}

/// Renders a uniform, crisp 44x44 rounded Midnight drag chip containing the action's icon
/// so drag-and-drop cursor previews stay compact and consistent across all plugins.
pub fn create_drag_chip_texture(icon_path: Option<&str>, icon_name: Option<&str>) -> Option<gtk::gdk::Texture> {
    use gtk::gdk::prelude::GdkCairoContextExt;
    const CHIP_SIZE: i32 = 44;
    const ICON_MAX: i32 = 24;
    const RADIUS: f64 = 10.0;

    let resolved_path: Option<String> = if let Some(p) = icon_path.filter(|s| !s.is_empty() && std::path::Path::new(s).exists()) {
        Some(p.to_string())
    } else if let Some(name) = icon_name.filter(|s| !s.is_empty()) {
        if std::path::Path::new(name).exists() {
            Some(name.to_string())
        } else {
            resolve_icon_path(name, ICON_MAX * 2)
        }
    } else {
        None
    };

    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, CHIP_SIZE, CHIP_SIZE).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    let s = CHIP_SIZE as f64;

    let rounded_rect = |cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64| {
        cr.new_sub_path();
        cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
        cr.close_path();
    };

    // Dark Midnight rounded chip background (#1e1e2a)
    rounded_rect(&cr, 1.0, 1.0, s - 2.0, s - 2.0, RADIUS);
    cr.set_source_rgba(0.118, 0.118, 0.165, 0.94);
    let _ = cr.fill_preserve();

    // Accent border (#9146ff)
    cr.set_source_rgba(0.569, 0.275, 1.0, 0.92);
    cr.set_line_width(1.5);
    let _ = cr.stroke();

    // Centered action icon (bounded strictly within ICON_MAX x ICON_MAX)
    if let Some(ref path) = resolved_path {
        let is_symbolic = path.contains("-symbolic");
        let pb_opt = if path.to_lowercase().ends_with(".svg") {
            gdk_pixbuf::Pixbuf::from_file_at_scale(path, ICON_MAX, ICON_MAX, true).ok()
        } else {
            gdk_pixbuf::Pixbuf::from_file(path).ok().and_then(|raw| {
                let rw = raw.width().max(1) as f64;
                let rh = raw.height().max(1) as f64;
                let sc = (ICON_MAX as f64 / rw).min(ICON_MAX as f64 / rh);
                let nw = (rw * sc).round().max(1.0) as i32;
                let nh = (rh * sc).round().max(1.0) as i32;
                raw.scale_simple(nw, nh, gdk_pixbuf::InterpType::Hyper)
            })
        };

        if let Some(pb) = pb_opt {
            let iw = pb.width() as f64;
            let ih = pb.height() as f64;
            let dx = ((s - iw) / 2.0).round();
            let dy = ((s - ih) / 2.0).round();
            if is_symbolic {
                // Tint monochrome symbolic icons crisp white on dark chip
                if let Ok(icon_surf) = cairo::ImageSurface::create(cairo::Format::ARgb32, pb.width(), pb.height()) {
                    if let Ok(icr) = cairo::Context::new(&icon_surf) {
                        icr.set_source_pixbuf(&pb, 0.0, 0.0);
                        let _ = icr.paint();
                        icr.set_operator(cairo::Operator::In);
                        icr.set_source_rgba(0.92, 0.92, 0.96, 1.0);
                        let _ = icr.paint();
                    }
                    let _ = cr.set_source_surface(&icon_surf, dx, dy);
                    let _ = cr.paint();
                }
            } else {
                cr.set_source_pixbuf(&pb, dx, dy);
                let _ = cr.paint();
            }
        }
    } else {
        // Clean centered '+' fallback glyph
        cr.set_source_rgba(0.89, 0.89, 0.94, 0.95);
        cr.set_line_width(2.0);
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(s / 2.0 - 6.0, s / 2.0);
        cr.line_to(s / 2.0 + 6.0, s / 2.0);
        cr.move_to(s / 2.0, s / 2.0 - 6.0);
        cr.line_to(s / 2.0, s / 2.0 + 6.0);
        let _ = cr.stroke();
    }

    drop(cr);
    let stride = surface.stride();
    let data = surface.take_data().ok()?;
    let bytes = glib::Bytes::from(&*data);
    let texture = gtk::gdk::MemoryTexture::new(
        CHIP_SIZE,
        CHIP_SIZE,
        gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride as usize,
    );
    Some(texture.upcast())
}

const SLOT_MOVE_PREFIX: &str = "ballista-slot-move:";

#[derive(Clone, Default)]
pub struct SlotDragMeta {
    pub has_action: bool,
    pub icon_path: Option<String>,
    pub icon_name: Option<String>,
    pub is_native: bool,
    pub caps: crate::native_sdk::TargetCaps,
}

fn target_to_surface(t: crate::ui::inspector::InspectorTarget) -> crate::native_sdk::SlotSurface {
    match t {
        crate::ui::inspector::InspectorTarget::Key(_) => crate::native_sdk::SlotSurface::Key,
        crate::ui::inspector::InspectorTarget::Dial(_) => crate::native_sdk::SlotSurface::Dial,
        crate::ui::inspector::InspectorTarget::Touchscreen => {
            crate::native_sdk::SlotSurface::Touchscreen
        }
    }
}

/// Enforces `TargetCaps` exclusivity during both Sidebar -> Canvas drags and Canvas Move/Swap drags.
fn can_accept_drag_on_target(
    drag_caps: Option<crate::native_sdk::TargetCaps>,
    drag_src: Option<crate::ui::inspector::InspectorTarget>,
    dest_target: crate::ui::inspector::InspectorTarget,
    dest_meta: Option<&SlotDragMeta>,
) -> bool {
    if drag_src == Some(dest_target) {
        return false;
    }
    let dest_surface = target_to_surface(dest_target);
    if let Some(caps) = drag_caps {
        if !caps.supports(dest_surface) {
            return false;
        }
    }
    // If moving/swapping between different surface types (e.g. Dial -> occupied Key),
    // verify the displaced action also supports the source surface!
    if let Some(src_t) = drag_src {
        if let Some(dm) = dest_meta {
            if dm.has_action && !dm.caps.supports(target_to_surface(src_t)) {
                return false;
            }
        }
    }
    true
}

fn encode_slot_move(target: crate::ui::inspector::InspectorTarget) -> Option<String> {
    match target {
        crate::ui::inspector::InspectorTarget::Key(k) => Some(format!("{SLOT_MOVE_PREFIX}key:{k}")),
        crate::ui::inspector::InspectorTarget::Dial(d) => Some(format!("{SLOT_MOVE_PREFIX}dial:{d}")),
        crate::ui::inspector::InspectorTarget::Touchscreen => None,
    }
}

fn decode_slot_move(payload: &str) -> Option<crate::ui::inspector::InspectorTarget> {
    let rest = payload.strip_prefix(SLOT_MOVE_PREFIX)?;
    if let Some(k) = rest.strip_prefix("key:") {
        k.parse::<usize>().ok().map(crate::ui::inspector::InspectorTarget::Key)
    } else if let Some(d) = rest.strip_prefix("dial:") {
        d.parse::<usize>().ok().map(crate::ui::inspector::InspectorTarget::Dial)
    } else {
        None
    }
}

/// Renders a crisp 48x48 rounded Midnight drag chip for moving/swapping an existing canvas slot.
/// For Ballista-native actions without a custom image path, draws a live miniature Cairo tile directly.
fn create_slot_drag_chip_texture(
    slot_name: &str,
    meta: &SlotDragMeta,
    native_host: Option<&crate::native_sdk::NativePluginHost>,
) -> Option<gtk::gdk::Texture> {
    let has_custom_path = meta
        .icon_path
        .as_deref()
        .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
        .is_some();

    if meta.is_native && !has_custom_path {
        if let Some(host) = native_host {
            if host.has_slot(slot_name) {
                const CHIP: i32 = 48;
                const R: f64 = 10.0;
                let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, CHIP, CHIP).ok()?;
                let cr = cairo::Context::new(&surface).ok()?;
                let s = CHIP as f64;

                let rounded_rect = |cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64| {
                    cr.new_sub_path();
                    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
                    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
                    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
                    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
                    cr.close_path();
                };

                // Clipped live miniature native action tile
                let _ = cr.save();
                rounded_rect(&cr, 1.5, 1.5, s - 3.0, s - 3.0, R - 1.0);
                cr.clip();
                cr.set_source_rgba(0.098, 0.098, 0.125, 0.96);
                let _ = cr.paint();
                cr.translate(1.5, 1.5);
                let _ = host.render_slot_visual(slot_name, &cr, s - 3.0, s - 3.0);
                let _ = cr.restore();

                // Crisp purple accent border
                rounded_rect(&cr, 1.0, 1.0, s - 2.0, s - 2.0, R);
                cr.set_source_rgba(0.569, 0.275, 1.0, 0.95);
                cr.set_line_width(1.75);
                let _ = cr.stroke();

                drop(cr);
                let stride = surface.stride();
                let data = surface.take_data().ok()?;
                let bytes = glib::Bytes::from(&*data);
                let texture = gtk::gdk::MemoryTexture::new(
                    CHIP,
                    CHIP,
                    gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied,
                    &bytes,
                    stride as usize,
                );
                return Some(texture.upcast());
            }
        }
    }

    create_drag_chip_texture(meta.icon_path.as_deref(), meta.icon_name.as_deref())
}

/// Computes exact outer and inner touchscreen capsule dimensions so each dial zone
/// maintains a true 2:1 inner content aspect ratio (matching 200x100 on both Stream Deck +
/// and Stream Deck + XL) after accounting for the 2px CSS border on `.deck-touchscreen-capsule`.
fn touch_strip_dimensions(kind: DeviceKind, btn_size: i32, spacing: u32) -> (i32, i32, i32, i32) {
    const CAPSULE_BORDER_PX: f64 = 4.0; // 2px top/bottom and left/right CSS border
    let (_rows, cols) = kind.grid_dimensions();
    let dial_count = kind.dial_count().max(1) as f64;
    let strip_w = cols as i32 * btn_size + (cols as i32 - 1) * spacing as i32;
    let inner_w = (strip_w as f64 - CAPSULE_BORDER_PX).max(40.0);
    let inner_h = (inner_w / (dial_count * 2.0)).round().max(36.0);
    let strip_h = (inner_h + CAPSULE_BORDER_PX).round() as i32;
    let slot_w = (inner_w / dial_count).round().max(40.0) as i32;
    (strip_w, strip_h, slot_w, inner_h as i32)
}

#[derive(Clone)]
pub struct KeyLabelSlotState {
    pub top: String,
    pub center: String,
    pub bottom: String,
    pub font_family: String,
    pub font_size: u32,
    pub text_color: String,
    pub marquee_start: std::time::Instant,
    pub needs_scroll: Rc<Cell<bool>>,
}

impl Default for KeyLabelSlotState {
    fn default() -> Self {
        Self {
            top: String::new(),
            center: String::new(),
            bottom: String::new(),
            font_family: "Cantarell".to_string(),
            font_size: 11,
            text_color: "#ffffff".to_string(),
            marquee_start: std::time::Instant::now(),
            needs_scroll: Rc::new(Cell::new(false)),
        }
    }
}

pub struct Canvas {
    pub container: Box,
    pub folder_bar_box: Box,
    pub folder_breadcrumb_label: Label,
    pub folder_back_btn: Button,
    pub on_folder_back: Rc<RefCell<Option<std::boxed::Box<dyn Fn()>>>>,
    pub native_host: Rc<RefCell<Option<crate::native_sdk::NativePluginHost>>>,
    pub hardware_area: Box,
    pub key_buttons: Rc<RefCell<Vec<Button>>>,
    pub key_sizers: Rc<RefCell<Vec<Box>>>,
    pub key_native_canvases: Rc<RefCell<Vec<DrawingArea>>>,
    pub key_label_canvases: Rc<RefCell<Vec<DrawingArea>>>,
    pub key_label_states: Rc<RefCell<Vec<Rc<RefCell<KeyLabelSlotState>>>>>,
    pub marquee_timer: Rc<RefCell<Option<glib::SourceId>>>,
    pub key_icons: Rc<RefCell<Vec<Picture>>>,
    pub key_overlays: Rc<RefCell<Vec<Overlay>>>,
    pub touch_segments: Rc<RefCell<Vec<Box>>>,
    pub touch_native_canvases: Rc<RefCell<Vec<DrawingArea>>>,
    pub touch_labels: Rc<RefCell<Vec<Label>>>,
    pub touch_icons: Rc<RefCell<Vec<Picture>>>,
    pub touch_capsule: Rc<RefCell<Option<Box>>>,
    pub touch_overlay: Rc<RefCell<Option<Overlay>>>,
    pub touch_sizer: Rc<RefCell<Option<Box>>>,
    pub touch_wallpaper_pic: Rc<RefCell<Option<Picture>>>,
    pub dials_row_box: Rc<RefCell<Option<Box>>>,
    pub neo_info_label: Rc<RefCell<Option<Label>>>,
    pub dial_draw_areas: Rc<RefCell<Vec<DrawingArea>>>,
    pub dial_angles: Rc<RefCell<Vec<Rc<Cell<f64>>>>>,
    pub dial_drop_hovered: Rc<RefCell<Vec<Rc<Cell<bool>>>>>,
    pub dial_pressed_states: Rc<RefCell<Vec<Rc<Cell<bool>>>>>,
    pub selected_dial_idx: Rc<RefCell<Option<usize>>>,
    pub on_dial_rotate: Rc<RefCell<Option<std::boxed::Box<dyn Fn(usize, i32)>>>>,
    pub on_dial_clicked: Rc<RefCell<Option<std::boxed::Box<dyn Fn(usize)>>>>,
    pub on_dial_pressed: Rc<RefCell<Option<std::boxed::Box<dyn Fn(usize)>>>>,
    pub on_touch_capsule_clicked: Rc<RefCell<Option<std::boxed::Box<dyn Fn()>>>>,
    pub on_action_dropped: Rc<RefCell<Option<std::boxed::Box<dyn Fn(crate::ui::inspector::InspectorTarget, crate::ui::sidebar::SelectedAction)>>>>,
    pub on_slot_moved: Rc<RefCell<Option<std::boxed::Box<dyn Fn(crate::ui::inspector::InspectorTarget, crate::ui::inspector::InspectorTarget)>>>>,
    pub key_drag_meta: Rc<RefCell<Vec<SlotDragMeta>>>,
    pub dial_drag_meta: Rc<RefCell<Vec<SlotDragMeta>>>,
    pub dragging_source: Rc<RefCell<Option<crate::ui::inspector::InspectorTarget>>>,
    pub active_drag_caps: Rc<Cell<Option<crate::native_sdk::TargetCaps>>>,
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
    pub key_pressed_since: Rc<RefCell<std::collections::HashMap<usize, std::time::Instant>>>,
    pub key_release_timers: Rc<RefCell<std::collections::HashMap<usize, glib::SourceId>>>,
    pub dial_pressed_since: Rc<RefCell<std::collections::HashMap<usize, std::time::Instant>>>,
    pub dial_release_timers: Rc<RefCell<std::collections::HashMap<usize, glib::SourceId>>>,
}

impl Canvas {
    pub fn new(initial_kind: DeviceKind) -> Self {
        let container = Box::new(Orientation::Vertical, 16);
        container.add_css_class("deck-canvas");
        container.set_valign(gtk::Align::Center);
        container.set_halign(gtk::Align::Center);
        container.set_vexpand(true);
        container.set_hexpand(true);

        // Sleek Folder Breadcrumb Pill (visible only when navigated inside a Folder)
        let folder_bar_box = Box::new(Orientation::Horizontal, 8);
        folder_bar_box.add_css_class("folder-breadcrumb-pill");
        folder_bar_box.set_halign(gtk::Align::Center);
        folder_bar_box.set_visible(false);

        let folder_back_btn = Button::new();
        let back_inner = Box::new(Orientation::Horizontal, 4);
        let back_icon = gtk::Image::from_icon_name("go-previous-symbolic");
        back_icon.set_pixel_size(14);
        let back_lbl = Label::new(Some("Back"));
        back_inner.append(&back_icon);
        back_inner.append(&back_lbl);
        folder_back_btn.set_child(Some(&back_inner));
        folder_back_btn.add_css_class("flat");
        folder_back_btn.add_css_class("folder-back-btn");

        let folder_icon = gtk::Image::from_icon_name("folder-open-symbolic");
        folder_icon.set_pixel_size(14);
        folder_icon.add_css_class("folder-breadcrumb-icon");

        let folder_breadcrumb_label = Label::new(Some("Page 1"));
        folder_breadcrumb_label.add_css_class("folder-breadcrumb-label");

        folder_bar_box.append(&folder_back_btn);
        folder_bar_box.append(&folder_icon);
        folder_bar_box.append(&folder_breadcrumb_label);
        container.append(&folder_bar_box);

        let on_folder_back: Rc<RefCell<Option<std::boxed::Box<dyn Fn()>>>> =
            Rc::new(RefCell::new(None));
        let back_cb_ref = on_folder_back.clone();
        folder_back_btn.connect_clicked(move |_| {
            if let Some(ref cb) = *back_cb_ref.borrow() {
                cb();
            }
        });

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
            folder_bar_box,
            folder_breadcrumb_label,
            folder_back_btn,
            on_folder_back,
            native_host: Rc::new(RefCell::new(None)),
            hardware_area,
            key_buttons: Rc::new(RefCell::new(Vec::new())),
            key_sizers: Rc::new(RefCell::new(Vec::new())),
            key_native_canvases: Rc::new(RefCell::new(Vec::new())),
            key_label_canvases: Rc::new(RefCell::new(Vec::new())),
            key_label_states: Rc::new(RefCell::new(Vec::new())),
            marquee_timer: Rc::new(RefCell::new(None)),
            key_icons: Rc::new(RefCell::new(Vec::new())),
            key_overlays: Rc::new(RefCell::new(Vec::new())),
            touch_segments: Rc::new(RefCell::new(Vec::new())),
            touch_native_canvases: Rc::new(RefCell::new(Vec::new())),
            touch_labels: Rc::new(RefCell::new(Vec::new())),
            touch_icons: Rc::new(RefCell::new(Vec::new())),
            touch_capsule: Rc::new(RefCell::new(None)),
            touch_overlay: Rc::new(RefCell::new(None)),
            touch_sizer: Rc::new(RefCell::new(None)),
            touch_wallpaper_pic: Rc::new(RefCell::new(None)),
            dials_row_box: Rc::new(RefCell::new(None)),
            neo_info_label: Rc::new(RefCell::new(None)),
            dial_draw_areas: Rc::new(RefCell::new(Vec::new())),
            dial_angles: Rc::new(RefCell::new(Vec::new())),
            dial_drop_hovered: Rc::new(RefCell::new(Vec::new())),
            dial_pressed_states: Rc::new(RefCell::new(Vec::new())),
            selected_dial_idx: Rc::new(RefCell::new(None)),
            on_dial_rotate: Rc::new(RefCell::new(None)),
            on_dial_clicked: Rc::new(RefCell::new(None)),
            on_dial_pressed: Rc::new(RefCell::new(None)),
            on_touch_capsule_clicked: Rc::new(RefCell::new(None)),
            on_action_dropped: Rc::new(RefCell::new(None)),
            on_slot_moved: Rc::new(RefCell::new(None)),
            key_drag_meta: Rc::new(RefCell::new(Vec::new())),
            dial_drag_meta: Rc::new(RefCell::new(Vec::new())),
            dragging_source: Rc::new(RefCell::new(None)),
            active_drag_caps: Rc::new(Cell::new(None)),
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
            key_pressed_since: Rc::new(RefCell::new(std::collections::HashMap::new())),
            key_release_timers: Rc::new(RefCell::new(std::collections::HashMap::new())),
            dial_pressed_since: Rc::new(RefCell::new(std::collections::HashMap::new())),
            dial_release_timers: Rc::new(RefCell::new(std::collections::HashMap::new())),
        };

        s.build_hardware_layout(initial_kind);
        s
    }

    pub fn set_page_callback<F: Fn(usize) + 'static>(&self, f: F) {
        *self.on_page_selected.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_folder_back_callback<F: Fn() + 'static>(&self, f: F) {
        *self.on_folder_back.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_native_host(&self, host: crate::native_sdk::NativePluginHost) {
        *self.native_host.borrow_mut() = Some(host);
    }

    /// Ultra-fast single-slot UI redraw for 1Hz native action ticks (Clock, System Metrics, Media).
    pub fn redraw_native_slot(&self, slot: &str) {
        if let Some(k_str) = slot.strip_prefix("key_") {
            if let Ok(idx) = k_str.parse::<usize>() {
                if let Some(da) = self.key_native_canvases.borrow().get(idx) {
                    da.queue_draw();
                }
            }
        } else if let Some(d_str) = slot.strip_prefix("dial_") {
            if let Ok(idx) = d_str.parse::<usize>() {
                if let Some(da) = self.touch_native_canvases.borrow().get(idx) {
                    da.queue_draw();
                }
            }
        }
    }

    pub fn set_action_dropped_callback<F: Fn(crate::ui::inspector::InspectorTarget, crate::ui::sidebar::SelectedAction) + 'static>(&self, f: F) {
        *self.on_action_dropped.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_slot_moved_callback<F: Fn(crate::ui::inspector::InspectorTarget, crate::ui::inspector::InspectorTarget) + 'static>(&self, f: F) {
        *self.on_slot_moved.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    /// Rebuilds the hardware canvas layout using a single unified Grid
    pub fn build_hardware_layout(&self, kind: DeviceKind) {
        *self.current_kind.borrow_mut() = kind;

        // Clear existing widgets
        while let Some(child) = self.hardware_area.first_child() {
            self.hardware_area.remove(&child);
        }
        self.key_buttons.borrow_mut().clear();
        self.key_sizers.borrow_mut().clear();
        self.key_drag_meta.borrow_mut().clear();
        self.dial_drag_meta.borrow_mut().clear();
        *self.dragging_source.borrow_mut() = None;
        self.active_drag_caps.set(None);
        self.key_native_canvases.borrow_mut().clear();
        self.key_label_canvases.borrow_mut().clear();
        self.key_label_states.borrow_mut().clear();
        if let Some(src) = self.marquee_timer.borrow_mut().take() {
            src.remove();
        }
        self.key_icons.borrow_mut().clear();
        self.key_overlays.borrow_mut().clear();
        self.touch_segments.borrow_mut().clear();
        self.touch_native_canvases.borrow_mut().clear();
        self.touch_labels.borrow_mut().clear();
        self.touch_icons.borrow_mut().clear();
        *self.touch_capsule.borrow_mut() = None;
        *self.touch_overlay.borrow_mut() = None;
        *self.touch_sizer.borrow_mut() = None;
        *self.touch_wallpaper_pic.borrow_mut() = None;
        *self.dials_row_box.borrow_mut() = None;
        *self.neo_info_label.borrow_mut() = None;
        self.dial_draw_areas.borrow_mut().clear();
        self.dial_angles.borrow_mut().clear();
        self.dial_drop_hovered.borrow_mut().clear();
        self.dial_pressed_states.borrow_mut().clear();
        for (_, src) in self.key_release_timers.borrow_mut().drain() {
            src.remove();
        }
        self.key_pressed_since.borrow_mut().clear();
        for (_, src) in self.dial_release_timers.borrow_mut().drain() {
            src.remove();
        }
        self.dial_pressed_since.borrow_mut().clear();

        let (rows, cols) = kind.grid_dimensions();

        // 1. Unified Grid System
        let grid = Grid::new();
        grid.set_halign(gtk::Align::Center);
        grid.set_valign(gtk::Align::Center);
        grid.set_hexpand(false);
        grid.set_vexpand(false);

        let (btn_size, spacing) = match kind {
            DeviceKind::StreamDeckStudio => (44, 6),
            DeviceKind::StreamDeckPlusXL => (64, 8),
            DeviceKind::StreamDeckXL => (72, 10),
            DeviceKind::StreamDeckMini => (94, 14),
            DeviceKind::StreamDeckPedal => (120, 18),
            _ => (86, 12), // Original, MK.2, Neo, Plus
        };
        let btn_height = if kind == DeviceKind::StreamDeckStudio {
            (btn_size as f64 * 1.45) as i32
        } else {
            btn_size
        };
        self.current_btn_size.set(btn_size);

        grid.set_row_spacing(spacing);
        grid.set_column_spacing(spacing);
        grid.set_column_homogeneous(true);
        grid.set_row_homogeneous(false);

        let mut key_buttons = Vec::new();
        let mut key_sizers = Vec::new();
        let mut key_native_canvases = Vec::new();
        let mut key_label_canvases = Vec::new();
        let mut key_label_states = Vec::new();
        let mut key_icons = Vec::new();
        let mut key_overlays = Vec::new();

        let inner_btn_w = (btn_size - 4).max(20);
        let inner_btn_h = (btn_height - 4).max(20);

        // Primary LCD Keys: Rows 0..rows (Pure blank initially, zero numbers)
        for r in 0..rows {
            for c in 0..cols {
                let idx = r * cols + c;
                let btn = Button::new();
                btn.add_css_class("deck-key");
                btn.add_css_class(&format!("deck-key-slot-{}", idx));
                btn.set_size_request(btn_size, btn_height);
                btn.set_hexpand(false);
                btn.set_vexpand(false);
                btn.set_focus_on_click(false);
                btn.set_overflow(gtk::Overflow::Hidden);

                let overlay = Overlay::new();
                overlay.set_can_target(false);
                overlay.set_overflow(gtk::Overflow::Hidden);
                overlay.set_hexpand(false);
                overlay.set_vexpand(false);

                // Fixed-size transparent sizer locks min == nat == (inner_btn_w, inner_btn_h)
                // so high-res icons or multi-line labels NEVER shift or resize the button grid!
                let sizer = Box::new(Orientation::Vertical, 0);
                sizer.set_size_request(inner_btn_w, inner_btn_h);
                sizer.set_hexpand(false);
                sizer.set_vexpand(false);
                sizer.set_can_target(false);

                let icon = Picture::new();
                icon.set_can_shrink(true);
                icon.set_content_fit(gtk::ContentFit::Contain);
                icon.set_halign(gtk::Align::Fill);
                icon.set_valign(gtk::Align::Fill);
                icon.set_hexpand(true);
                icon.set_vexpand(true);
                icon.set_visible(false);
                icon.set_can_target(false);

                // Unified single-line Cairo/Pango label overlay (Top / Center / Bottom)
                // with 1:1 WYSIWYG hardware scaling and smooth marquee scrolling when overflowing.
                let label_state = Rc::new(RefCell::new(KeyLabelSlotState::default()));
                let label_da = DrawingArea::new();
                label_da.set_hexpand(true);
                label_da.set_vexpand(true);
                label_da.set_halign(gtk::Align::Fill);
                label_da.set_valign(gtk::Align::Fill);
                label_da.set_can_target(false);

                let st_draw = label_state.clone();
                let marquee_timer_draw = self.marquee_timer.clone();
                let all_das_draw = self.key_label_canvases.clone();
                let all_states_draw = self.key_label_states.clone();
                let container_draw = self.container.clone();
                label_da.set_draw_func(move |_area, cr, width, height| {
                    let st = st_draw.borrow();
                    if st.top.is_empty() && st.center.is_empty() && st.bottom.is_empty() {
                        st.needs_scroll.set(false);
                        return;
                    }
                    let w = width as f64;
                    let h = height as f64;
                    let elapsed = st.marquee_start.elapsed().as_secs_f64();
                    let mut any_overflow = false;
                    if !st.top.is_empty() {
                        any_overflow |= crate::core::hardware::draw_single_line_key_label(
                            cr,
                            w,
                            h,
                            &st.top,
                            TextVerticalAlign::Top,
                            &st.font_family,
                            st.font_size,
                            &st.text_color,
                            elapsed,
                        );
                    }
                    if !st.center.is_empty() {
                        any_overflow |= crate::core::hardware::draw_single_line_key_label(
                            cr,
                            w,
                            h,
                            &st.center,
                            TextVerticalAlign::Center,
                            &st.font_family,
                            st.font_size,
                            &st.text_color,
                            elapsed,
                        );
                    }
                    if !st.bottom.is_empty() {
                        any_overflow |= crate::core::hardware::draw_single_line_key_label(
                            cr,
                            w,
                            h,
                            &st.bottom,
                            TextVerticalAlign::Bottom,
                            &st.font_family,
                            st.font_size,
                            &st.text_color,
                            elapsed,
                        );
                    }
                    st.needs_scroll.set(any_overflow);
                    drop(st);

                    if any_overflow && marquee_timer_draw.borrow().is_none() {
                        let timer_slot = marquee_timer_draw.clone();
                        let das_tick = all_das_draw.clone();
                        let states_tick = all_states_draw.clone();
                        let cont_tick = container_draw.clone();
                        let src = glib::timeout_add_local(
                            std::time::Duration::from_millis(33),
                            move || {
                                let states = states_tick.borrow();
                                let das = das_tick.borrow();
                                let mut still_scrolling = false;
                                let is_visible = cont_tick.is_mapped();
                                for (i, st_rc) in states.iter().enumerate() {
                                    if st_rc.borrow().needs_scroll.get() {
                                        still_scrolling = true;
                                        if is_visible {
                                            if let Some(da) = das.get(i) {
                                                da.queue_draw();
                                            }
                                        }
                                    }
                                }
                                if !still_scrolling {
                                    *timer_slot.borrow_mut() = None;
                                    glib::ControlFlow::Break
                                } else {
                                    glib::ControlFlow::Continue
                                }
                            },
                        );
                        *marquee_timer_draw.borrow_mut() = Some(src);
                    }
                });

                // Top-layer physical push-in depth vignette (visible even over opaque icons)
                let press_veil = Box::new(Orientation::Vertical, 0);
                press_veil.add_css_class("deck-key-press-veil");
                press_veil.set_hexpand(true);
                press_veil.set_vexpand(true);
                press_veil.set_can_target(false);

                // Top-layer rounded drag-and-drop highlight overlay (visible on empty & filled keys)
                let drop_veil = Box::new(Orientation::Vertical, 0);
                drop_veil.add_css_class("deck-key-drop-veil");
                drop_veil.set_hexpand(true);
                drop_veil.set_vexpand(true);
                drop_veil.set_halign(gtk::Align::Fill);
                drop_veil.set_valign(gtk::Align::Fill);
                drop_veil.set_visible(false);
                drop_veil.set_can_target(false);

                let drop_plus = gtk::Image::from_icon_name("list-add-symbolic");
                drop_plus.set_pixel_size(24);
                drop_plus.add_css_class("deck-drop-plus");
                drop_plus.set_halign(gtk::Align::Center);
                drop_plus.set_valign(gtk::Align::Center);
                drop_plus.set_hexpand(true);
                drop_plus.set_vexpand(true);
                drop_plus.set_can_target(false);
                drop_veil.append(&drop_plus);

                // Drag Source: allow dragging a populated button onto another button or dial to Move / Swap
                {
                    let drag_src = gtk::DragSource::new();
                    drag_src.set_actions(gtk::gdk::DragAction::MOVE);
                    drag_src.set_propagation_phase(gtk::PropagationPhase::Capture);

                    let meta_prep = self.key_drag_meta.clone();
                    let k_prep = idx;
                    drag_src.connect_prepare(move |_src, _x, _y| {
                        let has = meta_prep
                            .borrow()
                            .get(k_prep)
                            .map(|m| m.has_action)
                            .unwrap_or(false);
                        if !has {
                            return None;
                        }
                        let payload = encode_slot_move(crate::ui::inspector::InspectorTarget::Key(k_prep))?;
                        Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
                    });

                    let meta_begin = self.key_drag_meta.clone();
                    let nh_begin = self.native_host.clone();
                    let drag_state_begin = self.dragging_source.clone();
                    let drag_caps_begin = self.active_drag_caps.clone();
                    let btn_begin = btn.clone();
                    let k_begin = idx;
                    drag_src.connect_drag_begin(move |src, _drag| {
                        let meta = meta_begin
                            .borrow()
                            .get(k_begin)
                            .cloned()
                            .unwrap_or_default();
                        *drag_state_begin.borrow_mut() =
                            Some(crate::ui::inspector::InspectorTarget::Key(k_begin));
                        drag_caps_begin.set(Some(meta.caps));
                        btn_begin.add_css_class("drag-source");
                        let nh_opt = nh_begin.borrow().clone();
                        let slot_name = format!("key_{k_begin}");
                        if let Some(tex) =
                            create_slot_drag_chip_texture(&slot_name, &meta, nh_opt.as_ref())
                        {
                            let paintable: gtk::gdk::Paintable = tex.upcast();
                            src.set_icon(Some(&paintable), 24, 24);
                        }
                    });

                    let drag_state_end = self.dragging_source.clone();
                    let drag_caps_end = self.active_drag_caps.clone();
                    let btn_end = btn.clone();
                    drag_src.connect_drag_end(move |_src, _drag, _delete| {
                        *drag_state_end.borrow_mut() = None;
                        drag_caps_end.set(None);
                        btn_end.remove_css_class("drag-source");
                    });

                    let drag_state_cancel = self.dragging_source.clone();
                    let drag_caps_cancel = self.active_drag_caps.clone();
                    let btn_cancel = btn.clone();
                    drag_src.connect_drag_cancel(move |_src, _drag, _reason| {
                        *drag_state_cancel.borrow_mut() = None;
                        drag_caps_cancel.set(None);
                        btn_cancel.remove_css_class("drag-source");
                        true
                    });

                    btn.add_controller(drag_src);
                }

                // Drag and Drop Target for Sidebar Actions (COPY) & Canvas Move/Swap (MOVE) with TargetCaps Exclusivity
                let drop_target = gtk::DropTarget::new(
                    glib::Type::STRING,
                    gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
                );
                let btn_enter = btn.clone();
                let veil_enter = drop_veil.clone();
                let plus_enter = drop_plus.clone();
                let drag_state_enter = self.dragging_source.clone();
                let drag_caps_enter = self.active_drag_caps.clone();
                let meta_enter = self.key_drag_meta.clone();
                let k_enter = idx;
                drop_target.connect_enter(move |_target, _x, _y| {
                    let src = *drag_state_enter.borrow();
                    let dest = crate::ui::inspector::InspectorTarget::Key(k_enter);
                    let meta_b = meta_enter.borrow();
                    let dest_meta = meta_b.get(k_enter);
                    if !can_accept_drag_on_target(drag_caps_enter.get(), src, dest, dest_meta) {
                        return gtk::gdk::DragAction::empty();
                    }
                    let is_move = src.is_some();
                    if is_move {
                        let occupied = dest_meta.map(|m| m.has_action).unwrap_or(false);
                        plus_enter.set_icon_name(Some(if occupied {
                            "object-flip-horizontal-symbolic"
                        } else {
                            "go-down-symbolic"
                        }));
                    } else {
                        plus_enter.set_icon_name(Some("list-add-symbolic"));
                    }
                    btn_enter.add_css_class("drop-hover");
                    veil_enter.set_visible(true);
                    if is_move {
                        gtk::gdk::DragAction::MOVE
                    } else {
                        gtk::gdk::DragAction::COPY
                    }
                });
                let drag_state_motion = self.dragging_source.clone();
                let drag_caps_motion = self.active_drag_caps.clone();
                let meta_motion = self.key_drag_meta.clone();
                let k_motion = idx;
                drop_target.connect_motion(move |_target, _x, _y| {
                    let src = *drag_state_motion.borrow();
                    let dest = crate::ui::inspector::InspectorTarget::Key(k_motion);
                    let meta_b = meta_motion.borrow();
                    if !can_accept_drag_on_target(
                        drag_caps_motion.get(),
                        src,
                        dest,
                        meta_b.get(k_motion),
                    ) {
                        gtk::gdk::DragAction::empty()
                    } else if src.is_some() {
                        gtk::gdk::DragAction::MOVE
                    } else {
                        gtk::gdk::DragAction::COPY
                    }
                });
                let btn_leave = btn.clone();
                let veil_leave = drop_veil.clone();
                drop_target.connect_leave(move |_target| {
                    btn_leave.remove_css_class("drop-hover");
                    veil_leave.set_visible(false);
                });
                let dropped_cb = self.on_action_dropped.clone();
                let moved_cb = self.on_slot_moved.clone();
                let drag_state_drop = self.dragging_source.clone();
                let drag_caps_drop = self.active_drag_caps.clone();
                let meta_drop = self.key_drag_meta.clone();
                let btn_drop = btn.clone();
                let veil_drop = drop_veil.clone();
                let k_idx = idx;
                drop_target.connect_drop(move |_target, value, _x, _y| {
                    btn_drop.remove_css_class("drop-hover");
                    veil_drop.set_visible(false);
                    let cur_caps = drag_caps_drop.get();
                    drag_caps_drop.set(None);
                    if let Ok(json_str) = value.get::<String>() {
                        let dest = crate::ui::inspector::InspectorTarget::Key(k_idx);
                        if let Some(from_target) = decode_slot_move(&json_str) {
                            *drag_state_drop.borrow_mut() = None;
                            let meta_b = meta_drop.borrow();
                            if !can_accept_drag_on_target(
                                cur_caps,
                                Some(from_target),
                                dest,
                                meta_b.get(k_idx),
                            ) {
                                return false;
                            }
                            drop(meta_b);
                            if let Some(ref cb) = *moved_cb.borrow() {
                                cb(from_target, dest);
                                return true;
                            }
                            return false;
                        }
                        if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                            if !action.caps.supports(crate::native_sdk::SlotSurface::Key) {
                                return false;
                            }
                            if let Some(ref cb) = *dropped_cb.borrow() {
                                cb(dest, action);
                                return true;
                            }
                        }
                    }
                    false
                });
                btn.add_controller(drop_target);

                // Direct Cairo vector canvas for Ballista-native Rust actions (System Metrics, Clock, Media, etc.)
                let native_da = DrawingArea::new();
                native_da.set_hexpand(true);
                native_da.set_vexpand(true);
                native_da.set_halign(gtk::Align::Fill);
                native_da.set_valign(gtk::Align::Fill);
                native_da.set_visible(false);
                native_da.set_can_target(false);
                {
                    let nh_draw = self.native_host.clone();
                    let slot_name = format!("key_{idx}");
                    native_da.set_draw_func(move |_area, cr, width, height| {
                        let w = width as f64;
                        let h = height as f64;
                        let r = 11.0_f64.min(w / 2.0).min(h / 2.0);
                        cr.new_sub_path();
                        cr.arc(w - r, r, r, -std::f64::consts::FRAC_PI_2, 0.0);
                        cr.arc(w - r, h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
                        cr.arc(r, h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
                        cr.arc(r, r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
                        cr.close_path();
                        cr.clip();
                        if let Some(ref host) = *nh_draw.borrow() {
                            let _ = host.render_slot_visual(&slot_name, cr, w, h);
                        }
                    });
                }

                overlay.set_child(Some(&sizer));
                overlay.add_overlay(&native_da);
                overlay.add_overlay(&icon);
                overlay.add_overlay(&label_da);
                overlay.add_overlay(&press_veil);
                overlay.add_overlay(&drop_veil);
                btn.set_child(Some(&overlay));

                grid.attach(&btn, c as i32, r as i32, 1, 1);
                key_buttons.push(btn);
                key_sizers.push(sizer);
                key_native_canvases.push(native_da);
                key_label_canvases.push(label_da);
                key_label_states.push(label_state);
                key_icons.push(icon);
                key_overlays.push(overlay);
            }
        }

        let (strip_width, strip_height, _, _) = touch_strip_dimensions(kind, btn_size, spacing);

        // Pre-allocate dial drop-hover & press state cells so touch strip slots and dials stay in sync
        let total_dials = kind.dial_count();
        let dial_drop_hovered: Vec<Rc<Cell<bool>>> = (0..total_dials).map(|_| Rc::new(Cell::new(false))).collect();
        let dial_pressed_states: Vec<Rc<Cell<bool>>> = (0..total_dials).map(|_| Rc::new(Cell::new(false))).collect();
        let mut touch_drop_veils: Vec<Box> = Vec::new();
        let mut touch_drop_icons: Vec<gtk::Image> = Vec::new();
        *self.key_drag_meta.borrow_mut() = vec![SlotDragMeta::default(); rows * cols];
        *self.dial_drag_meta.borrow_mut() = vec![SlotDragMeta::default(); total_dials];

        // 2a. Stream Deck Plus / Stream Deck Plus XL: Full-Width Touchscreen Capsule
        if kind.has_touchscreen() {
            let dial_count = kind.dial_count().max(1);
            let inner_w = (strip_width - 4).max(40);
            let inner_h = (strip_height - 4).max(36);

            // Row `rows`: Continuous Touchscreen Capsule spanning all `cols` columns
            let touch_overlay = Overlay::new();
            touch_overlay.add_css_class("deck-touchscreen-capsule");
            touch_overlay.set_overflow(gtk::Overflow::Hidden);
            touch_overlay.set_hexpand(false);
            touch_overlay.set_vexpand(false);
            touch_overlay.set_halign(gtk::Align::Center);
            touch_overlay.set_valign(gtk::Align::Center);
            touch_overlay.set_margin_top(6);
            touch_overlay.set_size_request(strip_width, strip_height);

            // Fixed sizer locks touch_overlay to exact inner dimensions regardless of wallpaper resolution
            let touch_sizer = Box::new(Orientation::Vertical, 0);
            touch_sizer.set_size_request(inner_w, inner_h);
            touch_sizer.set_hexpand(false);
            touch_sizer.set_vexpand(false);
            touch_sizer.set_can_target(false);
            touch_overlay.set_child(Some(&touch_sizer));

            let touch_wallpaper_pic = Picture::new();
            touch_wallpaper_pic.set_can_shrink(true);
            touch_wallpaper_pic.set_content_fit(gtk::ContentFit::Cover);
            touch_wallpaper_pic.set_hexpand(true);
            touch_wallpaper_pic.set_vexpand(true);
            touch_wallpaper_pic.set_halign(gtk::Align::Fill);
            touch_wallpaper_pic.set_valign(gtk::Align::Fill);
            touch_wallpaper_pic.set_visible(false);
            touch_wallpaper_pic.set_can_target(false);
            touch_overlay.add_overlay(&touch_wallpaper_pic);

            let touch_capsule = Box::new(Orientation::Horizontal, 0);
            touch_capsule.set_homogeneous(true);
            touch_capsule.set_hexpand(true);
            touch_capsule.set_vexpand(true);

            let mut touch_segments = Vec::new();
            let mut touch_native_canvases = Vec::new();
            let mut touch_labels = Vec::new();
            let mut touch_icons = Vec::new();

            for slot_idx in 0..dial_count {
                let slot_box = Box::new(Orientation::Vertical, 0);
                slot_box.add_css_class("deck-touch-slot");
                slot_box.add_css_class(&format!("deck-touch-slot-{}", slot_idx));
                slot_box.set_overflow(gtk::Overflow::Hidden);
                slot_box.set_hexpand(true);
                slot_box.set_vexpand(true);

                let slot_overlay = Overlay::new();
                slot_overlay.set_overflow(gtk::Overflow::Hidden);
                slot_overlay.set_hexpand(true);
                slot_overlay.set_vexpand(true);

                let slot_sizer = Box::new(Orientation::Vertical, 0);
                slot_sizer.set_hexpand(true);
                slot_sizer.set_vexpand(true);
                slot_sizer.set_can_target(false);

                let slot_icon = Picture::new();
                slot_icon.set_can_shrink(true);
                slot_icon.set_content_fit(gtk::ContentFit::Contain);
                slot_icon.set_hexpand(true);
                slot_icon.set_vexpand(true);
                slot_icon.set_halign(gtk::Align::Fill);
                slot_icon.set_valign(gtk::Align::Fill);
                slot_icon.set_visible(false);
                slot_icon.set_can_target(false);

                let slot_label = Label::new(None);
                slot_label.add_css_class("deck-touch-slot-label");
                slot_label.set_halign(gtk::Align::Center);
                slot_label.set_valign(gtk::Align::Center);
                slot_label.set_can_target(false);

                let slot_drop_veil = Box::new(Orientation::Vertical, 0);
                slot_drop_veil.add_css_class("deck-touch-drop-veil");
                slot_drop_veil.set_hexpand(true);
                slot_drop_veil.set_vexpand(true);
                slot_drop_veil.set_visible(false);
                slot_drop_veil.set_can_target(false);

                let slot_drop_plus = gtk::Image::from_icon_name("list-add-symbolic");
                slot_drop_plus.set_pixel_size(20);
                slot_drop_plus.add_css_class("deck-drop-plus");
                slot_drop_plus.set_halign(gtk::Align::Center);
                slot_drop_plus.set_valign(gtk::Align::Center);
                slot_drop_plus.set_hexpand(true);
                slot_drop_plus.set_vexpand(true);
                slot_drop_plus.set_can_target(false);
                slot_drop_veil.append(&slot_drop_plus);

                let slot_native_da = DrawingArea::new();
                slot_native_da.set_hexpand(true);
                slot_native_da.set_vexpand(true);
                slot_native_da.set_halign(gtk::Align::Fill);
                slot_native_da.set_valign(gtk::Align::Fill);
                slot_native_da.set_visible(false);
                slot_native_da.set_can_target(false);
                {
                    let nh_touch = self.native_host.clone();
                    let slot_name = format!("dial_{slot_idx}");
                    slot_native_da.set_draw_func(move |_area, cr, width, height| {
                        if let Some(ref host) = *nh_touch.borrow() {
                            let _ = host.render_slot_visual(&slot_name, cr, width as f64, height as f64);
                        }
                    });
                }

                slot_overlay.set_child(Some(&slot_sizer));
                slot_overlay.add_overlay(&slot_native_da);
                slot_overlay.add_overlay(&slot_icon);
                slot_overlay.add_overlay(&slot_label);
                slot_overlay.add_overlay(&slot_drop_veil);
                slot_box.append(&slot_overlay);

                // Drag Source on populated Dial Touch Segment: drag to move/swap onto another Dial or Key
                {
                    let drag_src = gtk::DragSource::new();
                    drag_src.set_actions(gtk::gdk::DragAction::MOVE);
                    drag_src.set_propagation_phase(gtk::PropagationPhase::Capture);

                    let meta_prep = self.dial_drag_meta.clone();
                    let d_prep = slot_idx;
                    drag_src.connect_prepare(move |_src, _x, _y| {
                        let has = meta_prep
                            .borrow()
                            .get(d_prep)
                            .map(|m| m.has_action)
                            .unwrap_or(false);
                        if !has {
                            return None;
                        }
                        let payload =
                            encode_slot_move(crate::ui::inspector::InspectorTarget::Dial(d_prep))?;
                        Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
                    });

                    let meta_begin = self.dial_drag_meta.clone();
                    let nh_begin = self.native_host.clone();
                    let drag_state_begin = self.dragging_source.clone();
                    let drag_caps_begin = self.active_drag_caps.clone();
                    let seg_begin = slot_box.clone();
                    let d_begin = slot_idx;
                    drag_src.connect_drag_begin(move |src, _drag| {
                        let meta = meta_begin
                            .borrow()
                            .get(d_begin)
                            .cloned()
                            .unwrap_or_default();
                        *drag_state_begin.borrow_mut() =
                            Some(crate::ui::inspector::InspectorTarget::Dial(d_begin));
                        drag_caps_begin.set(Some(meta.caps));
                        seg_begin.add_css_class("drag-source");
                        let nh_opt = nh_begin.borrow().clone();
                        let slot_name = format!("dial_{d_begin}");
                        if let Some(tex) =
                            create_slot_drag_chip_texture(&slot_name, &meta, nh_opt.as_ref())
                        {
                            let paintable: gtk::gdk::Paintable = tex.upcast();
                            src.set_icon(Some(&paintable), 24, 24);
                        }
                    });

                    let drag_state_end = self.dragging_source.clone();
                    let drag_caps_end = self.active_drag_caps.clone();
                    let seg_end = slot_box.clone();
                    drag_src.connect_drag_end(move |_src, _drag, _delete| {
                        *drag_state_end.borrow_mut() = None;
                        drag_caps_end.set(None);
                        seg_end.remove_css_class("drag-source");
                    });

                    let drag_state_cancel = self.dragging_source.clone();
                    let drag_caps_cancel = self.active_drag_caps.clone();
                    let seg_cancel = slot_box.clone();
                    drag_src.connect_drag_cancel(move |_src, _drag, _reason| {
                        *drag_state_cancel.borrow_mut() = None;
                        drag_caps_cancel.set(None);
                        seg_cancel.remove_css_class("drag-source");
                        true
                    });

                    slot_box.add_controller(drag_src);
                }

                // Drag and Drop Target for Sidebar Actions (COPY) & Canvas Move/Swap (MOVE) onto Dial Touch Segment
                let drop_target = gtk::DropTarget::new(
                    glib::Type::STRING,
                    gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
                );
                let veil_enter = slot_drop_veil.clone();
                let icon_enter = slot_drop_plus.clone();
                let dial_hov_enter = dial_drop_hovered.get(slot_idx).cloned();
                let dial_areas_enter = self.dial_draw_areas.clone();
                let drag_state_enter = self.dragging_source.clone();
                let drag_caps_enter = self.active_drag_caps.clone();
                let dial_meta_enter = self.dial_drag_meta.clone();
                let d_slot = slot_idx;
                drop_target.connect_enter(move |_target, _x, _y| {
                    let src = *drag_state_enter.borrow();
                    let dest = crate::ui::inspector::InspectorTarget::Dial(d_slot);
                    let meta_b = dial_meta_enter.borrow();
                    let dest_meta = meta_b.get(d_slot);
                    if !can_accept_drag_on_target(drag_caps_enter.get(), src, dest, dest_meta) {
                        return gtk::gdk::DragAction::empty();
                    }
                    let is_move = src.is_some();
                    if is_move {
                        let occupied = dest_meta.map(|m| m.has_action).unwrap_or(false);
                        icon_enter.set_icon_name(Some(if occupied {
                            "object-flip-horizontal-symbolic"
                        } else {
                            "go-down-symbolic"
                        }));
                    } else {
                        icon_enter.set_icon_name(Some("list-add-symbolic"));
                    }
                    veil_enter.set_visible(true);
                    if let Some(ref cell) = dial_hov_enter {
                        cell.set(true);
                    }
                    if let Some(da) = dial_areas_enter.borrow().get(d_slot) {
                        da.queue_draw();
                    }
                    if is_move {
                        gtk::gdk::DragAction::MOVE
                    } else {
                        gtk::gdk::DragAction::COPY
                    }
                });
                let drag_state_motion = self.dragging_source.clone();
                let drag_caps_motion = self.active_drag_caps.clone();
                let dial_meta_motion = self.dial_drag_meta.clone();
                let d_motion = slot_idx;
                drop_target.connect_motion(move |_target, _x, _y| {
                    let src = *drag_state_motion.borrow();
                    let dest = crate::ui::inspector::InspectorTarget::Dial(d_motion);
                    let meta_b = dial_meta_motion.borrow();
                    if !can_accept_drag_on_target(
                        drag_caps_motion.get(),
                        src,
                        dest,
                        meta_b.get(d_motion),
                    ) {
                        gtk::gdk::DragAction::empty()
                    } else if src.is_some() {
                        gtk::gdk::DragAction::MOVE
                    } else {
                        gtk::gdk::DragAction::COPY
                    }
                });
                let veil_leave = slot_drop_veil.clone();
                let dial_hov_leave = dial_drop_hovered.get(slot_idx).cloned();
                let dial_areas_leave = self.dial_draw_areas.clone();
                drop_target.connect_leave(move |_target| {
                    veil_leave.set_visible(false);
                    if let Some(ref cell) = dial_hov_leave {
                        cell.set(false);
                    }
                    if let Some(da) = dial_areas_leave.borrow().get(d_slot) {
                        da.queue_draw();
                    }
                });
                let dropped_cb = self.on_action_dropped.clone();
                let moved_cb = self.on_slot_moved.clone();
                let drag_state_drop = self.dragging_source.clone();
                let drag_caps_drop = self.active_drag_caps.clone();
                let dial_meta_drop = self.dial_drag_meta.clone();
                let veil_drop = slot_drop_veil.clone();
                let dial_hov_drop = dial_drop_hovered.get(slot_idx).cloned();
                let dial_areas_drop = self.dial_draw_areas.clone();
                drop_target.connect_drop(move |_target, value, _x, _y| {
                    veil_drop.set_visible(false);
                    if let Some(ref cell) = dial_hov_drop {
                        cell.set(false);
                    }
                    if let Some(da) = dial_areas_drop.borrow().get(d_slot) {
                        da.queue_draw();
                    }
                    let cur_caps = drag_caps_drop.get();
                    drag_caps_drop.set(None);
                    if let Ok(json_str) = value.get::<String>() {
                        let dest = crate::ui::inspector::InspectorTarget::Dial(d_slot);
                        if let Some(from_target) = decode_slot_move(&json_str) {
                            *drag_state_drop.borrow_mut() = None;
                            let meta_b = dial_meta_drop.borrow();
                            if !can_accept_drag_on_target(
                                cur_caps,
                                Some(from_target),
                                dest,
                                meta_b.get(d_slot),
                            ) {
                                return false;
                            }
                            drop(meta_b);
                            if let Some(ref cb) = *moved_cb.borrow() {
                                cb(from_target, dest);
                                return true;
                            }
                            return false;
                        }
                        if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                            if !action.caps.supports(crate::native_sdk::SlotSurface::Dial) {
                                return false;
                            }
                            if let Some(ref cb) = *dropped_cb.borrow() {
                                cb(dest, action);
                                return true;
                            }
                        }
                    }
                    false
                });
                slot_box.add_controller(drop_target);

                touch_capsule.append(&slot_box);
                touch_segments.push(slot_box);
                touch_native_canvases.push(slot_native_da);
                touch_labels.push(slot_label);
                touch_icons.push(slot_icon);
                touch_drop_veils.push(slot_drop_veil);
                touch_drop_icons.push(slot_drop_plus);
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
            *self.touch_native_canvases.borrow_mut() = touch_native_canvases;
            *self.touch_labels.borrow_mut() = touch_labels;
            *self.touch_icons.borrow_mut() = touch_icons;
            *self.touch_capsule.borrow_mut() = Some(touch_capsule);
            *self.touch_overlay.borrow_mut() = Some(touch_overlay);
            *self.touch_sizer.borrow_mut() = Some(touch_sizer);
            *self.touch_wallpaper_pic.borrow_mut() = Some(touch_wallpaper_pic);
        }

        // 2b. Stream Deck Plus (4 dials), Stream Deck Plus XL (6 dials), Stream Deck Studio (2 dials):
        // Evenly distributed full-width homogeneous row directly aligned with the touchscreen cards above!
        if kind.has_dials() {
            let dial_count = kind.dial_count();
            let dial_row = if kind.has_touchscreen() {
                (rows + 1) as i32
            } else {
                rows as i32
            };
            let dial_size = (btn_size as f64 * 0.72).clamp(48.0, 68.0) as i32;

            let dials_row_box = Box::new(Orientation::Horizontal, 0);
            dials_row_box.set_homogeneous(true);
            dials_row_box.set_halign(gtk::Align::Center);
            dials_row_box.set_valign(gtk::Align::Center);
            dials_row_box.set_hexpand(false);
            dials_row_box.set_vexpand(false);
            dials_row_box.set_margin_top(6);
            dials_row_box.set_size_request(strip_width, -1);

            let mut dial_draw_areas = Vec::new();
            let mut dial_angles = Vec::new();

            for dial_idx in 0..dial_count {
                let dial_wrapper = Box::new(Orientation::Vertical, 0);
                dial_wrapper.add_css_class("deck-dial-wrapper");
                dial_wrapper.set_hexpand(true);
                dial_wrapper.set_halign(gtk::Align::Fill);
                dial_wrapper.set_valign(gtk::Align::Center);
                dial_wrapper.set_size_request(-1, dial_size + 12);

                let da = DrawingArea::new();
                da.set_size_request(dial_size, dial_size);
                da.set_content_width(dial_size);
                da.set_content_height(dial_size);
                da.set_halign(gtk::Align::Center);
                da.set_valign(gtk::Align::Center);
                da.set_cursor_from_name(Some("grab"));

                let angle_cell = Rc::new(Cell::new(0.0f64));
                dial_angles.push(angle_cell.clone());

                let is_hovered = Rc::new(Cell::new(false));
                let is_drop_hov = dial_drop_hovered
                    .get(dial_idx)
                    .cloned()
                    .unwrap_or_else(|| Rc::new(Cell::new(false)));
                let is_pressed_cell = dial_pressed_states
                    .get(dial_idx)
                    .cloned()
                    .unwrap_or_else(|| Rc::new(Cell::new(false)));

                // Drag and Drop Target for Sidebar Actions (COPY) & Canvas Move/Swap (MOVE) onto Rotary Dial
                let dial_drop = gtk::DropTarget::new(
                    glib::Type::STRING,
                    gtk::gdk::DragAction::COPY | gtk::gdk::DragAction::MOVE,
                );
                let drop_hov_enter = is_drop_hov.clone();
                let da_drop_enter = da.clone();
                let touch_veil_enter = touch_drop_veils.get(dial_idx).cloned();
                let touch_icon_enter = touch_drop_icons.get(dial_idx).cloned();
                let drag_state_enter = self.dragging_source.clone();
                let drag_caps_enter = self.active_drag_caps.clone();
                let dial_meta_enter = self.dial_drag_meta.clone();
                let d_enter = dial_idx;
                dial_drop.connect_enter(move |_target, _x, _y| {
                    let src = *drag_state_enter.borrow();
                    let dest = crate::ui::inspector::InspectorTarget::Dial(d_enter);
                    let meta_b = dial_meta_enter.borrow();
                    let dest_meta = meta_b.get(d_enter);
                    if !can_accept_drag_on_target(drag_caps_enter.get(), src, dest, dest_meta) {
                        return gtk::gdk::DragAction::empty();
                    }
                    let is_move = src.is_some();
                    if let Some(ref ic) = touch_icon_enter {
                        if is_move {
                            let occupied = dest_meta.map(|m| m.has_action).unwrap_or(false);
                            ic.set_icon_name(Some(if occupied {
                                "object-flip-horizontal-symbolic"
                            } else {
                                "go-down-symbolic"
                            }));
                        } else {
                            ic.set_icon_name(Some("list-add-symbolic"));
                        }
                    }
                    drop_hov_enter.set(true);
                    da_drop_enter.queue_draw();
                    if let Some(ref veil) = touch_veil_enter {
                        veil.set_visible(true);
                    }
                    if is_move {
                        gtk::gdk::DragAction::MOVE
                    } else {
                        gtk::gdk::DragAction::COPY
                    }
                });
                let drag_state_dial_motion = self.dragging_source.clone();
                let drag_caps_dial_motion = self.active_drag_caps.clone();
                let dial_meta_dial_motion = self.dial_drag_meta.clone();
                let d_dial_motion = dial_idx;
                dial_drop.connect_motion(move |_target, _x, _y| {
                    let src = *drag_state_dial_motion.borrow();
                    let dest = crate::ui::inspector::InspectorTarget::Dial(d_dial_motion);
                    let meta_b = dial_meta_dial_motion.borrow();
                    if !can_accept_drag_on_target(
                        drag_caps_dial_motion.get(),
                        src,
                        dest,
                        meta_b.get(d_dial_motion),
                    ) {
                        gtk::gdk::DragAction::empty()
                    } else if src.is_some() {
                        gtk::gdk::DragAction::MOVE
                    } else {
                        gtk::gdk::DragAction::COPY
                    }
                });
                let drop_hov_leave = is_drop_hov.clone();
                let da_drop_leave = da.clone();
                let touch_veil_leave = touch_drop_veils.get(dial_idx).cloned();
                dial_drop.connect_leave(move |_target| {
                    drop_hov_leave.set(false);
                    da_drop_leave.queue_draw();
                    if let Some(ref veil) = touch_veil_leave {
                        veil.set_visible(false);
                    }
                });
                let dropped_cb_dial = self.on_action_dropped.clone();
                let moved_cb_dial = self.on_slot_moved.clone();
                let drag_state_dial_drop = self.dragging_source.clone();
                let drag_caps_dial_drop = self.active_drag_caps.clone();
                let dial_meta_dial_drop = self.dial_drag_meta.clone();
                let drop_hov_done = is_drop_hov.clone();
                let da_drop_done = da.clone();
                let touch_veil_done = touch_drop_veils.get(dial_idx).cloned();
                let d_num = dial_idx;
                dial_drop.connect_drop(move |_target, value, _x, _y| {
                    drop_hov_done.set(false);
                    da_drop_done.queue_draw();
                    if let Some(ref veil) = touch_veil_done {
                        veil.set_visible(false);
                    }
                    let cur_caps = drag_caps_dial_drop.get();
                    drag_caps_dial_drop.set(None);
                    if let Ok(json_str) = value.get::<String>() {
                        let dest = crate::ui::inspector::InspectorTarget::Dial(d_num);
                        if let Some(from_target) = decode_slot_move(&json_str) {
                            *drag_state_dial_drop.borrow_mut() = None;
                            let meta_b = dial_meta_dial_drop.borrow();
                            if !can_accept_drag_on_target(
                                cur_caps,
                                Some(from_target),
                                dest,
                                meta_b.get(d_num),
                            ) {
                                return false;
                            }
                            drop(meta_b);
                            if let Some(ref cb) = *moved_cb_dial.borrow() {
                                cb(from_target, dest);
                                return true;
                            }
                            return false;
                        }
                        if let Ok(action) = serde_json::from_str::<crate::ui::sidebar::SelectedAction>(&json_str) {
                            if !action.caps.supports(crate::native_sdk::SlotSurface::Dial) {
                                return false;
                            }
                            if let Some(ref cb) = *dropped_cb_dial.borrow() {
                                cb(dest, action);
                                return true;
                            }
                        }
                    }
                    false
                });
                dial_wrapper.add_controller(dial_drop);

                let selected_dial_ref = self.selected_dial_idx.clone();
                let _angle_draw = angle_cell.clone();
                let is_hovered_draw = is_hovered.clone();
                let is_drop_draw = is_drop_hov.clone();
                let is_pressed_draw = is_pressed_cell.clone();
                let d_idx = dial_idx;

                da.set_draw_func(move |_area, cr, width, height| {
                    let w = width as f64;
                    let h = height as f64;
                    let cx = w / 2.0;
                    let cy = h / 2.0;
                    let base_r = (w.min(h) / 2.0 - 5.0).max(10.0);
                    let is_sel = selected_dial_ref.borrow().map_or(false, |sel| sel == d_idx);
                    let is_hov = is_hovered_draw.get();
                    let is_drop = is_drop_draw.get();
                    let is_pressed = is_pressed_draw.get();
                    let r = if is_pressed { base_r * 0.90 } else { base_r };

                    // 1. Outer circular glow ring (selection, drop hover, or physical press)
                    if is_drop {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.48); // Bright #9146ff circular drop halo
                        cr.set_line_width(7.0);
                        cr.arc(cx, cy, r + 2.0, 0.0, 2.0 * std::f64::consts::PI);
                        let _ = cr.stroke();
                    } else if is_pressed {
                        cr.set_source_rgba(0.69, 0.45, 1.0, 0.55); // Tactile press glow
                        cr.set_line_width(5.5);
                        cr.arc(cx, cy, r + 2.0, 0.0, 2.0 * std::f64::consts::PI);
                        let _ = cr.stroke();
                    } else if is_sel {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.40); // #9146ff glow
                        cr.set_line_width(6.0);
                        cr.arc(cx, cy, r + 2.0, 0.0, 2.0 * std::f64::consts::PI);
                        let _ = cr.stroke();
                    }

                    // 2. Knob circular body matching Midnight Dark buttons (#191920)
                    let (fill_r, fill_g, fill_b) = if is_drop {
                        (0.184, 0.125, 0.286) // Soft translucent purple tint inside circle
                    } else if is_pressed {
                        (0.075, 0.075, 0.105) // Depressed darker center
                    } else if is_hov && !is_sel {
                        (0.1294, 0.1294, 0.1725) // #21212c on hover
                    } else {
                        (0.0980, 0.0980, 0.1255) // #191920 Midnight Dark
                    };
                    cr.set_source_rgb(fill_r, fill_g, fill_b);
                    cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.fill();

                    // Inner push-in depth ring when pressed
                    if is_pressed {
                        cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
                        cr.set_line_width(3.0);
                        cr.arc(cx, cy, (r - 2.0).max(4.0), 0.0, 2.0 * std::f64::consts::PI);
                        let _ = cr.stroke();
                    }

                    // 3. Knob circular rim
                    if is_drop || is_pressed {
                        cr.set_source_rgba(0.69, 0.45, 1.0, 1.0); // #b072ff crisp rim
                        cr.set_line_width(2.5);
                    } else if is_sel {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.95); // #9146ff solid rim
                        cr.set_line_width(2.0);
                    } else if is_hov {
                        cr.set_source_rgba(0.57, 0.27, 1.0, 0.45);
                        cr.set_line_width(2.0);
                    } else {
                        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
                        cr.set_line_width(2.0);
                    }
                    cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.stroke();

                    // 4. Clean centered '+' drop glyph inside circular dial on drag-over
                    if is_drop {
                        let arm = (r * 0.34).clamp(6.0, 10.0);
                        cr.set_source_rgba(0.95, 0.93, 1.0, 0.96);
                        cr.set_line_width(2.2);
                        cr.set_line_cap(cairo::LineCap::Round);
                        cr.move_to(cx - arm, cy);
                        cr.line_to(cx + arm, cy);
                        cr.move_to(cx, cy - arm);
                        cr.line_to(cx, cy + arm);
                        let _ = cr.stroke();
                    }
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

                // Click Controller (Single-click selects Dial; Double-click triggers Dial Press!)
                let click = gtk::GestureClick::new();
                let dial_selected_click = self.selected_dial_idx.clone();
                let on_click_ref = self.on_dial_clicked.clone();
                let on_press_ref = self.on_dial_pressed.clone();
                let da_click = da.clone();
                click.connect_pressed(move |_g, n_press, _x, _y| {
                    *dial_selected_click.borrow_mut() = Some(d_idx);
                    da_click.queue_draw();
                    if let Some(ref cb) = *on_click_ref.borrow() {
                        cb(d_idx);
                    }
                    if n_press == 2 {
                        if let Some(ref pcb) = *on_press_ref.borrow() {
                            pcb(d_idx);
                        }
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
                dials_row_box.append(&dial_wrapper);
                dial_draw_areas.push(da);
            }
            grid.attach(&dials_row_box, 0, dial_row, cols as i32, 1);
            *self.dials_row_box.borrow_mut() = Some(dials_row_box);
            *self.dial_draw_areas.borrow_mut() = dial_draw_areas;
            *self.dial_angles.borrow_mut() = dial_angles;
        }

        // 2c. Stream Deck Neo: Info Bar LCD + Left/Right Touch Points
        if kind.has_info_screen() {
            let neo_bar_box = Box::new(Orientation::Horizontal, 16);
            neo_bar_box.set_halign(gtk::Align::Center);
            neo_bar_box.set_valign(gtk::Align::Center);
            neo_bar_box.set_margin_top(8);

            let left_dot = Label::new(Some("◀"));
            left_dot.add_css_class("dim-label");

            let info_capsule = Box::new(Orientation::Horizontal, 0);
            info_capsule.add_css_class("deck-touchscreen-capsule");
            info_capsule.set_size_request(180, 38);
            info_capsule.set_halign(gtk::Align::Center);
            info_capsule.set_valign(gtk::Align::Center);

            let info_lbl = Label::new(Some("Page 1 / 1"));
            info_lbl.add_css_class("deck-touch-slot-label");
            info_lbl.set_hexpand(true);
            info_lbl.set_halign(gtk::Align::Center);
            info_lbl.set_valign(gtk::Align::Center);
            info_capsule.append(&info_lbl);

            let right_dot = Label::new(Some("▶"));
            right_dot.add_css_class("dim-label");

            neo_bar_box.append(&left_dot);
            neo_bar_box.append(&info_capsule);
            neo_bar_box.append(&right_dot);
            grid.attach(&neo_bar_box, 0, rows as i32, cols as i32, 1);
            *self.neo_info_label.borrow_mut() = Some(info_lbl);
        }

        self.hardware_area.append(&grid);
        *self.grid.borrow_mut() = Some(grid);
        *self.key_buttons.borrow_mut() = key_buttons;
        *self.key_sizers.borrow_mut() = key_sizers;
        *self.key_native_canvases.borrow_mut() = key_native_canvases;
        *self.key_label_canvases.borrow_mut() = key_label_canvases;
        *self.key_label_states.borrow_mut() = key_label_states;
        *self.key_icons.borrow_mut() = key_icons;
        *self.key_overlays.borrow_mut() = key_overlays;
        *self.dial_drop_hovered.borrow_mut() = dial_drop_hovered;
        *self.dial_pressed_states.borrow_mut() = dial_pressed_states;
    }

    pub fn set_dial_rotate_callback<F: Fn(usize, i32) + 'static>(&self, f: F) {
        *self.on_dial_rotate.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_dial_click_callback<F: Fn(usize) + 'static>(&self, f: F) {
        *self.on_dial_clicked.borrow_mut() = Some(std::boxed::Box::new(f));
    }

    pub fn set_dial_press_callback<F: Fn(usize) + 'static>(&self, f: F) {
        *self.on_dial_pressed.borrow_mut() = Some(std::boxed::Box::new(f));
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
        let (rows, _cols) = kind.grid_dimensions();
        let row_count = if kind.has_touchscreen() && kind.has_dials() {
            rows as f64 + 1.8
        } else if kind.has_dials() || kind.has_info_screen() {
            rows as f64 + 1.0
        } else {
            rows as f64 + 0.6
        };

        let (min_sz, max_sz) = match kind {
            DeviceKind::StreamDeckStudio => (38, 56),
            DeviceKind::StreamDeckPlusXL => (52, 78),
            DeviceKind::StreamDeckXL => (58, 88),
            _ => (74, 115),
        };

        let target_size = (((available_h - 110) as f64 / row_count) as i32).clamp(min_sz, max_sz);
        if target_size == self.current_btn_size.get() {
            return;
        }
        self.current_btn_size.set(target_size);

        let target_h = if kind == DeviceKind::StreamDeckStudio {
            (target_size as f64 * 1.45) as i32
        } else {
            target_size
        };

        let spacing = match kind {
            DeviceKind::StreamDeckStudio => (target_size as f64 * 0.12).clamp(4.0, 8.0) as u32,
            DeviceKind::StreamDeckPlusXL => (target_size as f64 * 0.13).clamp(6.0, 12.0) as u32,
            _ => (target_size as f64 * 0.14).clamp(8.0, 16.0) as u32,
        };
        if let Some(ref g) = *self.grid.borrow() {
            g.set_row_spacing(spacing);
            g.set_column_spacing(spacing);
        }

        let inner_btn_w = (target_size - 4).max(20);
        let inner_btn_h = (target_h - 4).max(20);
        for btn in self.key_buttons.borrow().iter() {
            btn.set_size_request(target_size, target_h);
        }
        for sizer in self.key_sizers.borrow().iter() {
            sizer.set_size_request(inner_btn_w, inner_btn_h);
        }
        for da in self.key_label_canvases.borrow().iter() {
            da.queue_draw();
        }

        let (strip_w, strip_h, _, _) = touch_strip_dimensions(kind, target_size, spacing);
        let inner_w = (strip_w - 4).max(40);
        let inner_h = (strip_h - 4).max(36);
        if let Some(ref to) = *self.touch_overlay.borrow() {
            to.set_size_request(strip_w, strip_h);
        }
        if let Some(ref ts) = *self.touch_sizer.borrow() {
            ts.set_size_request(inner_w, inner_h);
        }
        if let Some(ref db) = *self.dials_row_box.borrow() {
            db.set_size_request(strip_w, -1);
        }

        let dial_size = (target_size as f64 * 0.72).clamp(48.0, 76.0) as i32;
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

    pub fn set_key_pressed(&self, index: usize, is_pressed: bool) {
        const MIN_VISUAL_PRESS_MS: u64 = 95;
        let keys = self.key_buttons.borrow();
        let Some(btn) = keys.get(index).cloned() else {
            return;
        };
        drop(keys);

        if is_pressed {
            if let Some(old) = self.key_release_timers.borrow_mut().remove(&index) {
                old.remove();
            }
            self.key_pressed_since.borrow_mut().insert(index, std::time::Instant::now());
            btn.add_css_class("pressed");
        } else {
            let elapsed_ms = self
                .key_pressed_since
                .borrow()
                .get(&index)
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(MIN_VISUAL_PRESS_MS);

            if elapsed_ms >= MIN_VISUAL_PRESS_MS {
                if let Some(old) = self.key_release_timers.borrow_mut().remove(&index) {
                    old.remove();
                }
                btn.remove_css_class("pressed");
            } else {
                let remaining = MIN_VISUAL_PRESS_MS - elapsed_ms;
                let timers = self.key_release_timers.clone();
                let btn_rel = btn.clone();
                if let Some(old) = timers.borrow_mut().remove(&index) {
                    old.remove();
                }
                let timers_inner = timers.clone();
                let src = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(remaining),
                    move || {
                        timers_inner.borrow_mut().remove(&index);
                        btn_rel.remove_css_class("pressed");
                    },
                );
                timers.borrow_mut().insert(index, src);
            }
        }
    }

    pub fn pulse_key_press(&self, index: usize, duration_ms: u64) {
        self.set_key_pressed(index, true);
        let keys = self.key_buttons.borrow();
        let Some(btn) = keys.get(index).cloned() else {
            return;
        };
        drop(keys);
        let timers = self.key_release_timers.clone();
        if let Some(old) = timers.borrow_mut().remove(&index) {
            old.remove();
        }
        let timers_inner = timers.clone();
        let src = glib::timeout_add_local_once(
            std::time::Duration::from_millis(duration_ms.max(90)),
            move || {
                timers_inner.borrow_mut().remove(&index);
                btn.remove_css_class("pressed");
            },
        );
        timers.borrow_mut().insert(index, src);
    }

    pub fn set_dial_pressed(&self, dial_idx: usize, is_pressed: bool) {
        const MIN_VISUAL_PRESS_MS: u64 = 95;
        let state_opt = self.dial_pressed_states.borrow().get(dial_idx).cloned();
        let da_opt = self.dial_draw_areas.borrow().get(dial_idx).cloned();
        let (Some(state), Some(da)) = (state_opt, da_opt) else {
            return;
        };

        if is_pressed {
            if let Some(old) = self.dial_release_timers.borrow_mut().remove(&dial_idx) {
                old.remove();
            }
            self.dial_pressed_since.borrow_mut().insert(dial_idx, std::time::Instant::now());
            state.set(true);
            da.queue_draw();
        } else {
            let elapsed_ms = self
                .dial_pressed_since
                .borrow()
                .get(&dial_idx)
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(MIN_VISUAL_PRESS_MS);

            if elapsed_ms >= MIN_VISUAL_PRESS_MS {
                if let Some(old) = self.dial_release_timers.borrow_mut().remove(&dial_idx) {
                    old.remove();
                }
                state.set(false);
                da.queue_draw();
            } else {
                let remaining = MIN_VISUAL_PRESS_MS - elapsed_ms;
                let timers = self.dial_release_timers.clone();
                if let Some(old) = timers.borrow_mut().remove(&dial_idx) {
                    old.remove();
                }
                let timers_inner = timers.clone();
                let src = glib::timeout_add_local_once(
                    std::time::Duration::from_millis(remaining),
                    move || {
                        timers_inner.borrow_mut().remove(&dial_idx);
                        state.set(false);
                        da.queue_draw();
                    },
                );
                timers.borrow_mut().insert(dial_idx, src);
            }
        }
    }

    pub fn pulse_dial_press(&self, dial_idx: usize, duration_ms: u64) {
        self.set_dial_pressed(dial_idx, true);
        let state_opt = self.dial_pressed_states.borrow().get(dial_idx).cloned();
        let da_opt = self.dial_draw_areas.borrow().get(dial_idx).cloned();
        let (Some(state), Some(da)) = (state_opt, da_opt) else {
            return;
        };
        let timers = self.dial_release_timers.clone();
        if let Some(old) = timers.borrow_mut().remove(&dial_idx) {
            old.remove();
        }
        let timers_inner = timers.clone();
        let src = glib::timeout_add_local_once(
            std::time::Duration::from_millis(duration_ms.max(90)),
            move || {
                timers_inner.borrow_mut().remove(&dial_idx);
                state.set(false);
                da.queue_draw();
            },
        );
        timers.borrow_mut().insert(dial_idx, src);
    }

    /// Highlights Dial `dial_idx` cleanly (without drawing a selection box over the touch strip,
    /// matching StreamController's clean dial-only highlight)
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

        // Keep touch strip segments unhighlighted so the dial card renders cleanly edge-to-edge
        for seg in self.touch_segments.borrow().iter() {
            seg.remove_css_class("selected");
        }

        // Highlight Rotary Dial ring only
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

        // Update Folder Breadcrumb Bar when inside a Folder
        if profile.is_in_folder() {
            self.folder_breadcrumb_label.set_text(&profile.folder_breadcrumb());
            self.folder_bar_box.set_visible(true);
            self.page_pill_box.set_visible(false);
        } else {
            self.folder_bar_box.set_visible(false);
            self.page_pill_box.set_visible(true);
        }

        if let Some(ref neo_lbl) = *self.neo_info_label.borrow() {
            neo_lbl.set_text(&format!("Page {} / {}", cur_idx + 1, total_pages.max(1)));
        }

        let mut css = String::new();
        if let Some(page) = profile.current_page() {
            let keys = self.key_buttons.borrow();
            let native_canvases = self.key_native_canvases.borrow();
            let label_canvases = self.key_label_canvases.borrow();
            let label_states = self.key_label_states.borrow();
            let icons = self.key_icons.borrow();
            let native_host_opt = self.native_host.borrow().clone();
            let mut key_meta = self.key_drag_meta.borrow_mut();
            if key_meta.len() != keys.len() {
                key_meta.resize(keys.len(), SlotDragMeta::default());
            }

            // 1. Primary LCD Keys
            for (idx, _btn) in keys.iter().enumerate() {
                if let Some(action) = page.keys.get(&idx) {
                    let app = action.active_appearance();
                    let is_native_meta = native_host_opt
                        .as_ref()
                        .map(|h| h.is_native_plugin(&action.plugin_id))
                        .unwrap_or(false);
                    let action_caps = native_host_opt
                        .as_ref()
                        .map(|h| h.action_caps(&action.plugin_id, &action.action_id))
                        .unwrap_or_default();
                    key_meta[idx] = SlotDragMeta {
                        has_action: true,
                        icon_path: app.icon_path.clone(),
                        icon_name: action.parameters.get("icon_name").cloned(),
                        is_native: is_native_meta,
                        caps: action_caps,
                    };

                    let (new_top, new_center, new_bottom) = if app.has_plugin_labels() {
                        (
                            app.top_label.clone().unwrap_or_default(),
                            app.center_label.clone().unwrap_or_default(),
                            app.bottom_label.clone().unwrap_or_default(),
                        )
                    } else {
                        let title = if !app.show_title || app.title.is_empty() {
                            String::new()
                        } else {
                            app.title.clone()
                        };
                        match app.vertical_align {
                            TextVerticalAlign::Top => (title, String::new(), String::new()),
                            TextVerticalAlign::Center => (String::new(), title, String::new()),
                            TextVerticalAlign::Bottom => (String::new(), String::new(), title),
                        }
                    };

                    if let Some(st_rc) = label_states.get(idx) {
                        let mut st = st_rc.borrow_mut();
                        let text_changed = st.top != new_top
                            || st.center != new_center
                            || st.bottom != new_bottom
                            || st.font_family != app.font_family
                            || st.font_size != app.font_size;
                        st.top = new_top;
                        st.center = new_center;
                        st.bottom = new_bottom;
                        st.font_family = app.font_family.clone();
                        st.font_size = app.font_size;
                        st.text_color = app.text_color.clone();
                        if text_changed {
                            st.marquee_start = std::time::Instant::now();
                        }
                    }
                    if let Some(da) = label_canvases.get(idx) {
                        da.queue_draw();
                    }

                    if let Some(src) = self.active_ui_animations.borrow_mut().remove(&idx) {
                        src.remove();
                    }

                    // Check if this key is backed by a Ballista-native Rust action with custom Cairo artwork
                    let has_custom_path = app
                        .icon_path
                        .as_deref()
                        .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
                        .is_some();
                    let is_native_action = native_host_opt
                        .as_ref()
                        .map(|h| h.is_native_plugin(&action.plugin_id) && h.has_slot(&format!("key_{idx}")))
                        .unwrap_or(false);
                    let show_native_canvas = is_native_action && !has_custom_path;

                    if let Some(nda) = native_canvases.get(idx) {
                        nda.set_visible(show_native_canvas);
                        if show_native_canvas {
                            nda.queue_draw();
                        }
                    }

                    if let Some(icon) = icons.get(idx) {
                        if show_native_canvas {
                            icon.set_visible(false);
                        } else {
                        let btn_sz = (self.current_btn_size.get() - 4).max(20) as f64;
                        let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
                        let icon_sz = ((btn_sz * scale_pct) as i32).max(12);
                        let pad = if app.icon_scale < 100 {
                            ((btn_sz * (1.0 - scale_pct)) / 2.0).round() as i32
                        } else {
                            0
                        };
                        icon.set_margin_start(pad);
                        icon.set_margin_end(pad);
                        icon.set_margin_top(pad);
                        icon.set_margin_bottom(pad);
                        icon.set_size_request(-1, -1);

                        let mut icon_path = app.icon_path.as_deref();
                        let icon_name = action.parameters.get("icon_name").map(|s| s.as_str())
                            .or_else(|| match action.action_id.as_str() {
                                "system.hotkey" => Some("input-keyboard-symbolic"),
                                "system.url" => Some("web-browser-symbolic"),
                                "system.terminal" => Some("utilities-terminal-symbolic"),
                                "app_launcher.launch" => Some("system-run"),
                                _ => None,
                            });

                        // For App Launcher actions: ignore generic placeholder icons if an app has been configured
                        if action.action_id.contains("AppLauncher") || action.action_id == "app_launcher.launch" {
                            if action.parameters.contains_key("desktop_id") || action.parameters.contains_key("icon_name") {
                                if let Some(p) = icon_path {
                                    if p.ends_with("action_launch.png") || p.ends_with("default_app.png") {
                                        icon_path = action.parameters.get("icon_path").map(|s| s.as_str());
                                    }
                                }
                            }
                        }

                        if let Some(path) = icon_path {
                            if path.to_lowercase().ends_with(".gif") && std::path::Path::new(path).exists() {
                                // 1. Immediately show first frame with zero UI hitch
                                if let Some(texture) = create_rounded_texture(path, icon_sz, icon_sz, 11.0, true) {
                                    icon.set_paintable(Some(&texture));
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
                                icon.set_visible(true);
                            } else {
                                icon.set_visible(false);
                            }
                        } else if let Some(paintable) = lookup_icon_paintable(None, icon_name, icon_sz, 11.0) {
                            icon.set_paintable(Some(&paintable));
                            icon.set_visible(true);
                        } else {
                            icon.set_visible(false);
                        }
                        }
                    }

                    let bg = app.bg_color.as_deref().unwrap_or("#191920");
                    css.push_str(&format!(
                        "button.deck-key.deck-key-slot-{} {{ background-color: {}; background-image: none; }}\n",
                        idx, bg
                    ));
                } else {
                    key_meta[idx] = SlotDragMeta::default();
                    if let Some(nda) = native_canvases.get(idx) {
                        nda.set_visible(false);
                    }
                    if let Some(st_rc) = label_states.get(idx) {
                        let mut st = st_rc.borrow_mut();
                        st.top.clear();
                        st.center.clear();
                        st.bottom.clear();
                        st.needs_scroll.set(false);
                    }
                    if let Some(da) = label_canvases.get(idx) {
                        da.queue_draw();
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
            drop(key_meta);
            let touch_labels = self.touch_labels.borrow();
            let touch_icons = self.touch_icons.borrow();
            let touch_segs = self.touch_segments.borrow();
            let touch_native_canvases = self.touch_native_canvases.borrow();
            let mut dial_meta = self.dial_drag_meta.borrow_mut();
            if dial_meta.len() != touch_segs.len() {
                dial_meta.resize(touch_segs.len(), SlotDragMeta::default());
            }
            for (d_idx, slot_m) in dial_meta.iter_mut().enumerate() {
                if let Some(action) = page.dial_actions.get(&d_idx) {
                    let app = action.active_appearance();
                    let is_native_meta = native_host_opt
                        .as_ref()
                        .map(|h| h.is_native_plugin(&action.plugin_id))
                        .unwrap_or(false);
                    let action_caps = native_host_opt
                        .as_ref()
                        .map(|h| h.action_caps(&action.plugin_id, &action.action_id))
                        .unwrap_or_default();
                    *slot_m = SlotDragMeta {
                        has_action: true,
                        icon_path: app.icon_path.clone(),
                        icon_name: action.parameters.get("icon_name").cloned(),
                        is_native: is_native_meta,
                        caps: action_caps,
                    };
                } else {
                    *slot_m = SlotDragMeta::default();
                }
            }
            drop(dial_meta);

            let has_panoramic = page.touchscreen_action.as_ref().map_or(false, |act| {
                let app = act.active_appearance();
                !app.title.is_empty()
                    || app.bg_color.is_some()
                    || app.icon_path.is_some()
            });

            // Update Touch Strip Wallpaper if configured
            if let Some(ref pic) = *self.touch_wallpaper_pic.borrow() {
                let wallpaper_path = page.touchscreen_action.as_ref().and_then(|a| a.active_appearance().icon_path.as_deref());
                if let Some(path) = wallpaper_path {
                    if std::path::Path::new(path).exists() {
                        let kind = *self.current_kind.borrow();
                        let btn_size = self.current_btn_size.get();
                        let spacing = match kind {
                            DeviceKind::StreamDeckStudio => (btn_size as f64 * 0.12).clamp(4.0, 8.0) as u32,
                            DeviceKind::StreamDeckPlusXL => (btn_size as f64 * 0.13).clamp(6.0, 12.0) as u32,
                            _ => (btn_size as f64 * 0.14).clamp(8.0, 16.0) as u32,
                        };
                        let (strip_w, strip_h, _, _) = touch_strip_dimensions(kind, btn_size, spacing);
                        let inner_w = (strip_w - 4).max(40);
                        let inner_h = (strip_h - 4).max(36);

                        if path.to_lowercase().ends_with(".gif") {
                            pic.set_filename(Some(path));
                        } else if let Some(texture) = create_rounded_texture(path, inner_w, inner_h, 11.0, false) {
                            pic.set_paintable(Some(&texture));
                        } else if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, inner_w, inner_h, false) {
                            let texture = gtk::gdk::Texture::for_pixbuf(&pb);
                            pic.set_paintable(Some(&texture));
                        } else {
                            pic.set_filename(Some(path));
                        }
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
                    let app = act.active_appearance();
                    let bg = app.bg_color.as_deref().unwrap_or("transparent");
                    css.push_str(&format!(
                        ".deck-touchscreen-capsule {{ background-color: {}; }}\n",
                        bg
                    ));
                    if let Some(label) = touch_labels.first() {
                        label.set_text(&app.title);
                        label.set_visible(!app.title.is_empty());
                    }
                    for label in touch_labels.iter().skip(1) {
                        label.set_text("");
                        label.set_visible(false);
                    }
                    for icon in touch_icons.iter() {
                        icon.set_visible(false);
                    }
                    for nda in touch_native_canvases.iter() {
                        nda.set_visible(false);
                    }
                }
            } else {
                let kind = *self.current_kind.borrow();
                let btn_size = self.current_btn_size.get();
                let spacing = match kind {
                    DeviceKind::StreamDeckStudio => (btn_size as f64 * 0.12).clamp(4.0, 8.0) as u32,
                    DeviceKind::StreamDeckPlusXL => (btn_size as f64 * 0.13).clamp(6.0, 12.0) as u32,
                    _ => (btn_size as f64 * 0.14).clamp(8.0, 16.0) as u32,
                };
                let (_strip_w, _strip_h, slot_w, slot_h) = touch_strip_dimensions(kind, btn_size, spacing);

                for (idx, _seg) in touch_segs.iter().enumerate() {
                    if let Some(action) = page.dial_actions.get(&idx) {
                        let app = action.active_appearance();
                        let display_title = app.combined_display_summary();

                        let has_custom_path = app
                            .icon_path
                            .as_deref()
                            .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
                            .is_some();
                        let show_native_touch = native_host_opt
                            .as_ref()
                            .map(|h| h.is_native_plugin(&action.plugin_id) && h.has_slot(&format!("dial_{idx}")))
                            .unwrap_or(false)
                            && !has_custom_path;

                        if let Some(nda) = touch_native_canvases.get(idx) {
                            nda.set_visible(show_native_touch);
                            if show_native_touch {
                                nda.queue_draw();
                            }
                        }

                        if let Some(label) = touch_labels.get(idx) {
                            if show_native_touch {
                                label.set_visible(false);
                            } else {
                                label.set_text(&display_title);
                                label.set_visible(!display_title.is_empty());
                                match app.vertical_align {
                                    TextVerticalAlign::Top => label.set_valign(gtk::Align::Start),
                                    TextVerticalAlign::Center => label.set_valign(gtk::Align::Center),
                                    TextVerticalAlign::Bottom => label.set_valign(gtk::Align::End),
                                }
                            }
                        }

                        if let Some(icon) = touch_icons.get(idx) {
                            if show_native_touch {
                                icon.set_visible(false);
                            } else {
                            let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
                            let target_w = ((slot_w as f64 * scale_pct).round() as i32).max(24);
                            let target_h = ((slot_h as f64 * scale_pct).round() as i32).max(16);

                            let icon_path = app.icon_path.as_deref();
                            let icon_name = action.parameters.get("icon_name").map(|s| s.as_str())
                                .or_else(|| match action.action_id.as_str() {
                                    "system.hotkey" => Some("input-keyboard-symbolic"),
                                    "system.url" => Some("web-browser-symbolic"),
                                    "system.terminal" => Some("utilities-terminal-symbolic"),
                                    "app_launcher.launch" => Some("system-run"),
                                    _ => None,
                                });

                            if let Some(paintable) = lookup_rect_paintable(icon_path, icon_name, target_w, target_h, 0.0) {
                                let aspect = paintable.intrinsic_aspect_ratio();
                                let is_dial_card = (1.80..=2.20).contains(&aspect);
                                icon.set_paintable(Some(&paintable));
                                if app.icon_scale < 100 {
                                    icon.set_content_fit(gtk::ContentFit::Contain);
                                    icon.set_halign(gtk::Align::Center);
                                    icon.set_valign(gtk::Align::Center);
                                    icon.set_size_request(target_w, target_h);
                                } else {
                                    // True 2:1 dial cards fill edge-to-edge (eliminating sub-pixel rounding gaps);
                                    // square icons preserve their natural aspect ratio cleanly.
                                    icon.set_content_fit(if is_dial_card {
                                        gtk::ContentFit::Fill
                                    } else {
                                        gtk::ContentFit::Contain
                                    });
                                    icon.set_halign(gtk::Align::Fill);
                                    icon.set_valign(gtk::Align::Fill);
                                    icon.set_size_request(-1, -1);
                                }
                                icon.set_visible(true);
                            } else {
                                icon.set_visible(false);
                            }
                            }
                        }

                        let bg = app.bg_color.as_deref().unwrap_or("transparent");
                        let fg = &app.text_color;
                        let font = &app.font_family;
                        let size = app.font_size;
                        css.push_str(&format!(
                            ".deck-touch-slot.deck-touch-slot-{} {{ background-color: {}; }}\n\
                             .deck-touch-slot.deck-touch-slot-{} label {{ color: {}; font-family: '{}'; font-size: {}pt; font-weight: bold; }}\n",
                            idx, bg, idx, fg, font, size
                        ));
                    } else {
                        if let Some(nda) = touch_native_canvases.get(idx) {
                            nda.set_visible(false);
                        }
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
