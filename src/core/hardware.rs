use crate::core::GifAnimation;
use crate::models::{DeviceKind, KeyAppearance, Profile, TextVerticalAlign};
use cairo::{Context, Format, ImageSurface};
use gtk::gdk::prelude::GdkCairoContextExt;
use image::{ImageBuffer, Rgb};
use pangocairo::functions as pc;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ConnectedDevice {
    pub kind: DeviceKind,
    pub name: String,
    pub dev_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum HardwareEvent {
    KeyDown { key_index: usize },
    KeyUp { key_index: usize },
    DialRotate { dial_index: usize, delta: i32 },
    DialPress { dial_index: usize },
    DialRelease { dial_index: usize },
    TouchTap { slot: usize, x: u16, y: u16 },
    TouchSwipe { is_right: bool },
}

pub struct HardwareManager {
    running: Arc<AtomicBool>,
    write_lock: Arc<Mutex<()>>,
    active_animations: Arc<Mutex<HashMap<String, (String, Arc<AtomicBool>)>>>,
}

fn parse_hex_color(hex: &str) -> Option<(f64, f64, f64)> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0))
    } else {
        None
    }
}

fn load_icon(icon_path: Option<&str>, icon_name: Option<&str>, max_size: i32) -> Option<gdk_pixbuf::Pixbuf> {
    if let Some(path) = icon_path {
        if Path::new(path).exists() {
            if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, max_size, max_size, true) {
                return Some(pb);
            }
        }
    }
    if let Some(name) = icon_name {
        if Path::new(name).exists() {
            if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(name, max_size, max_size, true) {
                return Some(pb);
            }
        }
        let candidates = [
            format!("/usr/share/pixmaps/{name}.png"),
            format!("/usr/share/pixmaps/{name}.svg"),
            format!("/usr/share/icons/hicolor/48x48/apps/{name}.png"),
            format!("/usr/share/icons/hicolor/scalable/apps/{name}.svg"),
            format!("/usr/share/icons/hicolor/128x128/apps/{name}.png"),
            format!("/usr/share/icons/hicolor/256x256/apps/{name}.png"),
        ];
        for cand in candidates {
            if Path::new(&cand).exists() {
                if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(&cand, max_size, max_size, true) {
                    return Some(pb);
                }
            }
        }
        // Fallback to default_app.png if available
        let fallback_app = "/home/oscar/.local/share/ballista/plugins/com_oparada_AppLauncher/assets/default_app.png";
        if Path::new(fallback_app).exists() {
            if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(fallback_app, max_size, max_size, true) {
                return Some(pb);
            }
        }
    }
    None
}

impl HardwareManager {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
            write_lock: Arc::new(Mutex::new(())),
            active_animations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Scan /sys/class/hidraw for connected Elgato Stream Decks
    pub fn scan_devices(&self) -> Vec<ConnectedDevice> {
        let mut devices = Vec::new();

        if let Ok(entries) = fs::read_dir("/sys/class/hidraw") {
            for entry in entries.flatten() {
                let uevent_path = entry.path().join("device").join("uevent");
                if let Ok(content) = fs::read_to_string(&uevent_path) {
                    if content.to_uppercase().contains("0FD9") {
                        let dev_node = PathBuf::from("/dev").join(entry.file_name());

                        // Match specific Elgato PIDs
                        if content.contains("008F") || content.contains("008f") || content.contains("006C") || content.contains("006c") {
                            devices.push(ConnectedDevice {
                                kind: DeviceKind::StreamDeckXL,
                                name: "Stream Deck XL".into(),
                                dev_path: dev_node,
                            });
                        } else if content.contains("0084") {
                            devices.push(ConnectedDevice {
                                kind: DeviceKind::StreamDeckPlus,
                                name: "Stream Deck +".into(),
                                dev_path: dev_node,
                            });
                        } else if content.contains("006D") || content.contains("006d") {
                            devices.push(ConnectedDevice {
                                kind: DeviceKind::StreamDeckMK2,
                                name: "Stream Deck MK.2".into(),
                                dev_path: dev_node,
                            });
                        } else if content.contains("0063") {
                            devices.push(ConnectedDevice {
                                kind: DeviceKind::StreamDeckMini,
                                name: "Stream Deck Mini".into(),
                                dev_path: dev_node,
                            });
                        }
                    }
                }
            }
        }

        devices.sort_by(|a, b| a.name.cmp(&b.name));
        devices
    }

    /// Send brightness feature report (0-100%) to Stream Deck hardware
    pub fn set_brightness(&self, dev_path: &Path, brightness: u8) -> Result<(), String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dev_path)
            .map_err(|e| format!("Cannot open device {}: {e}", dev_path.display()))?;

        let mut buf = [0u8; 32];
        buf[0] = 0x03; // Feature Report ID
        buf[1] = 0x08; // Set brightness command
        buf[2] = brightness.min(100);

        let fd = file.as_raw_fd();
        let request = 0xC0000000u64 | ((buf.len() as u64) << 16) | (0x48 << 8) | 0x06;

        let res = unsafe { libc::ioctl(fd, request, buf.as_mut_ptr()) };
        if res < 0 {
            Err(format!("ioctl failed with code {res}"))
        } else {
            Ok(())
        }
    }

    pub fn write_key_image_direct(dev_path: &Path, key: usize, jpeg_data: &[u8]) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(dev_path)
            .map_err(|e| format!("Cannot open device {}: {e}", dev_path.display()))?;

        const REPORT_LEN: usize = 1024;
        const PAYLOAD_LEN: usize = 1016;

        let mut bytes_remaining = jpeg_data.len();
        let mut page_number: u16 = 0;

        while bytes_remaining > 0 {
            let this_length = std::cmp::min(bytes_remaining, PAYLOAD_LEN);
            let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };

            let mut packet = [0u8; REPORT_LEN];
            packet[0] = 0x02; // Output Report ID
            packet[1] = 0x07; // Set key image command
            packet[2] = (key & 0xFF) as u8;
            packet[3] = is_last;
            packet[4] = (this_length & 0xFF) as u8;
            packet[5] = ((this_length >> 8) & 0xFF) as u8;
            packet[6] = (page_number & 0xFF) as u8;
            packet[7] = ((page_number >> 8) & 0xFF) as u8;

            let bytes_sent = page_number as usize * PAYLOAD_LEN;
            packet[8..8 + this_length].copy_from_slice(&jpeg_data[bytes_sent..bytes_sent + this_length]);

            file.write_all(&packet)
                .map_err(|e| format!("Failed to write key image packet: {e}"))?;

            page_number += 1;
            bytes_remaining -= this_length;
        }

        Ok(())
    }

    /// Write a rendered JPEG image buffer to a specific physical key on the Stream Deck
    pub fn write_key_image(&self, dev_path: &Path, key: usize, jpeg_data: &[u8]) -> Result<(), String> {
        let _lock = self.write_lock.lock().unwrap();
        Self::write_key_image_direct(dev_path, key, jpeg_data)
    }

    pub fn write_touchscreen_image_direct(
        dev_path: &Path,
        x_pos: u16,
        y_pos: u16,
        width: u16,
        height: u16,
        jpeg_data: &[u8],
    ) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .open(dev_path)
            .map_err(|e| format!("Cannot open device {}: {e}", dev_path.display()))?;

        const REPORT_LEN: usize = 1024;
        const HEADER_LEN: usize = 16;
        const PAYLOAD_LEN: usize = REPORT_LEN - HEADER_LEN; // 1008 bytes

        let mut bytes_remaining = jpeg_data.len();
        let mut page_number: u16 = 0;

        while bytes_remaining > 0 {
            let this_length = std::cmp::min(bytes_remaining, PAYLOAD_LEN);
            let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };

            let mut packet = [0u8; REPORT_LEN];
            packet[0] = 0x02; // Output Report ID
            packet[1] = 0x0c; // Set touchscreen image command
            packet[2] = (x_pos & 0xFF) as u8;
            packet[3] = ((x_pos >> 8) & 0xFF) as u8;
            packet[4] = (y_pos & 0xFF) as u8;
            packet[5] = ((y_pos >> 8) & 0xFF) as u8;
            packet[6] = (width & 0xFF) as u8;
            packet[7] = ((width >> 8) & 0xFF) as u8;
            packet[8] = (height & 0xFF) as u8;
            packet[9] = ((height >> 8) & 0xFF) as u8;
            packet[10] = is_last;
            packet[11] = (page_number & 0xFF) as u8;
            packet[12] = ((page_number >> 8) & 0xFF) as u8;
            packet[13] = (this_length & 0xFF) as u8;
            packet[14] = ((this_length >> 8) & 0xFF) as u8;
            packet[15] = 0x00;

            let bytes_sent = page_number as usize * PAYLOAD_LEN;
            packet[16..16 + this_length].copy_from_slice(&jpeg_data[bytes_sent..bytes_sent + this_length]);

            file.write_all(&packet)
                .map_err(|e| format!("Failed to write touchscreen packet: {e}"))?;

            page_number += 1;
            bytes_remaining -= this_length;
        }

        Ok(())
    }

    /// Write a rendered JPEG image buffer to the Stream Deck + Touchscreen strip (800x100)
    pub fn write_touchscreen_image(
        &self,
        dev_path: &Path,
        x_pos: u16,
        y_pos: u16,
        width: u16,
        height: u16,
        jpeg_data: &[u8],
    ) -> Result<(), String> {
        let _lock = self.write_lock.lock().unwrap();
        Self::write_touchscreen_image_direct(dev_path, x_pos, y_pos, width, height, jpeg_data)
    }

    /// Renders a key image buffer to JPEG formatted for the hardware with background, icon, and title
    pub fn render_key_jpeg(
        &self,
        kind: DeviceKind,
        _key_idx: usize,
        appearance: Option<&KeyAppearance>,
        icon_name: Option<&str>,
        is_active: bool,
    ) -> Vec<u8> {
        let (dim, rotate_180) = match kind {
            DeviceKind::StreamDeckXL => (96u32, true),
            DeviceKind::StreamDeckPlus => (120u32, false),
            DeviceKind::StreamDeckPlusXL => (112u32, false),
            DeviceKind::StreamDeckMini => (80u32, false),
            _ => (72u32, true),
        };

        let surface = ImageSurface::create(Format::Rgb24, dim as i32, dim as i32)
            .expect("Failed to create Cairo image surface");
        let cr = Context::new(&surface).expect("Failed to create Cairo context");

        // Pure Black Hardware Contract:
        // Empty / unassigned keys are rendered 100% pure black with NO border and NO text
        if !is_active {
            cr.set_source_rgb(0.0, 0.0, 0.0);
            let _ = cr.paint();
        } else {
            // 1. Active Key Background
            let custom_bg = appearance.and_then(|a| a.bg_color.as_deref()).and_then(parse_hex_color);
            let (r, g, b) = custom_bg.unwrap_or((0.0, 0.0, 0.0)); // Deep black default for active keys unless custom color picked

            cr.set_source_rgb(r, g, b);
            let _ = cr.paint();

            // 2. Subtle 1px inner border only if custom color was chosen
            if custom_bg.is_some() {
                cr.set_source_rgb(
                    (r + 0.15).min(1.0),
                    (g + 0.15).min(1.0),
                    (b + 0.15).min(1.0),
                );
                cr.set_line_width(2.0);
                cr.rectangle(1.0, 1.0, (dim - 2) as f64, (dim - 2) as f64);
                let _ = cr.stroke();
            }

            // 3. Icon rendering (full-bleed at 100% scale completely covering key, scaled down when < 100%)
            let icon_path = appearance.and_then(|a| a.icon_path.as_deref());
            let scale_pct = appearance.map(|a| a.icon_scale).unwrap_or(100).clamp(20, 100) as f64 / 100.0;
            let icon_max = ((dim as f64 * scale_pct) as i32).max(12);

            if let Some(pixbuf) = load_icon(icon_path, icon_name, icon_max) {
                let icon_w = pixbuf.width() as f64;
                let icon_h = pixbuf.height() as f64;
                let icon_x = ((dim as f64 - icon_w) / 2.0).max(0.0);
                let icon_y = ((dim as f64 - icon_h) / 2.0).max(0.0);

                cr.set_source_pixbuf(&pixbuf, icon_x, icon_y);
                let _ = cr.paint();
            }

            // 4. Title Text rendering as an OVERLAY directly on top of the image
            if let Some(app) = appearance {
                if app.show_title && !app.title.is_empty() {
                    let layout = pc::create_layout(&cr);
                    let scale_factor = dim as f64 / 96.0;
                    let target_size = ((app.font_size as f64 * scale_factor).max(8.0) * pango::SCALE as f64) as i32;

                    let mut font_desc = pango::FontDescription::new();
                    font_desc.set_family(&app.font_family);
                    font_desc.set_size(target_size);
                    font_desc.set_weight(pango::Weight::Bold);
                    layout.set_font_description(Some(&font_desc));

                    layout.set_text(&app.title);
                    layout.set_alignment(pango::Alignment::Center);
                    layout.set_width(((dim - 8) as i32) * pango::SCALE);
                    layout.set_wrap(pango::WrapMode::WordChar);

                    let (_, extents) = layout.pixel_extents();
                    let text_h = extents.height() as f64;

                    let text_y = match app.vertical_align {
                        TextVerticalAlign::Top => 4.0,
                        TextVerticalAlign::Center => ((dim as f64 - text_h) / 2.0).max(2.0),
                        TextVerticalAlign::Bottom => (dim as f64 - text_h - 4.0).max(2.0),
                    };

                    // Draw subtle text drop shadow for legibility over images
                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.85);
                    cr.move_to(5.0, text_y + 1.0);
                    pc::show_layout(&cr, &layout);

                    let (tr, tg, tb) = parse_hex_color(&app.text_color).unwrap_or((1.0, 1.0, 1.0));
                    cr.set_source_rgb(tr, tg, tb);
                    cr.move_to(4.0, text_y);
                    pc::show_layout(&cr, &layout);
                }
            }
        }

        // 5. Convert Cairo surface to RGB ImageBuffer
        drop(cr);
        let stride = surface.stride() as usize;
        let surface_data = surface.take_data().expect("Failed to read surface data");

        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(dim, dim);
        for y in 0..dim {
            for x in 0..dim {
                let offset = y as usize * stride + x as usize * 4;
                let b_val = surface_data[offset];
                let g_val = surface_data[offset + 1];
                let r_val = surface_data[offset + 2];
                img.put_pixel(x, y, Rgb([r_val, g_val, b_val]));
            }
        }

        // 6. Rotate 180 degrees if required by physical hardware
        if rotate_180 {
            img = image::imageops::rotate180(&img);
        }

        let mut jpeg_buf = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 95);
        let _ = encoder.encode_image(&img);

        jpeg_buf
    }

    /// Renders the 800x100 touchscreen strip for the Stream Deck +
    /// Renders the 800x100 touchscreen strip for the Stream Deck +
    /// Pure black by default; renders graphics for active dial cards or panoramic strip
    pub fn render_touchscreen_jpeg(&self, profile: &Profile) -> Vec<u8> {
        let width = 800u32;
        let height = 100u32;

        let surface = ImageSurface::create(Format::Rgb24, width as i32, height as i32)
            .expect("Failed to create Touchscreen Cairo surface");
        let cr = Context::new(&surface).expect("Failed to create Touchscreen Cairo context");

        // Pure black 100% hardware LCD background
        cr.set_source_rgb(0.0, 0.0, 0.0);
        let _ = cr.paint();

        if let Some(page) = profile.current_page() {
            // Check if there is an active full panoramic touchscreen action
            let has_panoramic_content = page.touchscreen_action.as_ref().map_or(false, |act| {
                let app = act.active_appearance();
                !app.title.is_empty()
                    || app.bg_color.is_some()
                    || app.icon_path.is_some()
                    || (act.action_id != "custom" && act.action_id != "touchscreen" && !act.action_name.is_empty())
            });

            if has_panoramic_content {
                if let Some(ref action) = page.touchscreen_action {
                    let app = action.active_appearance();
                    let title = if !app.show_title || app.title.is_empty() {
                        ""
                    } else {
                        &app.title
                    };

                    // Only fill background if user explicitly chose a background color
                    if let Some(ref bg_hex) = app.bg_color {
                        if let Some((bg_r, bg_g, bg_b)) = parse_hex_color(bg_hex) {
                            cr.set_source_rgb(bg_r, bg_g, bg_b);
                            cr.rectangle(0.0, 0.0, 800.0, 100.0);
                            let _ = cr.fill();
                        }
                    }

                    // Render Touch Strip Wallpaper
                    if let Some(ref icon_path) = app.icon_path {
                        if Path::new(icon_path).exists() {
                            let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
                            let target_w = (800.0 * scale_pct) as i32;
                            let target_h = (100.0 * scale_pct) as i32;
                            if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(icon_path, target_w, target_h, false) {
                                let pw = pixbuf.width() as f64;
                                let ph = pixbuf.height() as f64;
                                let px = (800.0 - pw) / 2.0;
                                let py = (100.0 - ph) / 2.0;
                                cr.set_source_pixbuf(&pixbuf, px, py);
                                let _ = cr.paint();
                            }
                        }
                    }

                    if !title.is_empty() {
                        let layout = pc::create_layout(&cr);
                        let mut font_desc = pango::FontDescription::new();
                        font_desc.set_family(&app.font_family);
                        font_desc.set_size(15 * pango::SCALE);
                        font_desc.set_weight(pango::Weight::Bold);
                        layout.set_font_description(Some(&font_desc));
                        layout.set_text(title);
                        layout.set_alignment(pango::Alignment::Center);
                        layout.set_width(800 * pango::SCALE);

                        let (fg_r, fg_g, fg_b) = parse_hex_color(&app.text_color).unwrap_or((1.0, 1.0, 1.0));
                        cr.set_source_rgb(fg_r, fg_g, fg_b);
                        cr.move_to(0.0, 36.0);
                        pc::show_layout(&cr, &layout);
                    }
                }
            } else {
                // Render individual 200x100 dial cards (Cleanly on pure black, NO artificial borders or boxes)
                for i in 0..4 {
                    if let Some(action) = page.dial_actions.get(&i) {
                        let slot_x = (i * 200) as f64;
                        let title = if !action.appearance.show_title || action.appearance.title.is_empty() {
                            ""
                        } else {
                            &action.appearance.title
                        };

                        // Only fill background if user explicitly chose a background color!
                        if let Some(ref bg_hex) = action.appearance.bg_color {
                            if let Some((bg_r, bg_g, bg_b)) = parse_hex_color(bg_hex) {
                                cr.set_source_rgb(bg_r, bg_g, bg_b);
                                cr.rectangle(slot_x, 0.0, 200.0, 100.0);
                                let _ = cr.fill();
                            }
                        }

                        // Dial Card Icon (with responsive scaling)
                        let mut has_card_icon = false;
                        if let Some(ref path) = action.appearance.icon_path {
                            if Path::new(path).exists() {
                                let scale_pct = (action.appearance.icon_scale.clamp(20, 100) as f64) / 100.0;
                                let base_sz = if title.is_empty() { 70.0 } else { 48.0 };
                                let icon_sz = ((base_sz * scale_pct) as i32).max(12);
                                if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(path, icon_sz, icon_sz, true) {
                                    has_card_icon = true;
                                    let pw = pixbuf.width() as f64;
                                    let ph = pixbuf.height() as f64;
                                    let px = slot_x + (200.0 - pw) / 2.0;
                                    let py = if title.is_empty() { (100.0 - ph) / 2.0 } else { 10.0 };
                                    cr.set_source_pixbuf(&pixbuf, px, py);
                                    let _ = cr.paint();
                                }
                            }
                        }

                        // Dial Title Header
                        if !title.is_empty() {
                            let layout = pc::create_layout(&cr);
                            let mut font_desc = pango::FontDescription::new();
                            font_desc.set_family(&action.appearance.font_family);
                            font_desc.set_size(action.appearance.font_size as i32 * pango::SCALE);
                            font_desc.set_weight(pango::Weight::Bold);
                            layout.set_font_description(Some(&font_desc));
                            layout.set_text(title);
                            layout.set_alignment(pango::Alignment::Center);
                            layout.set_width(200 * pango::SCALE);

                            let (fg_r, fg_g, fg_b) = parse_hex_color(&action.appearance.text_color).unwrap_or((1.0, 1.0, 1.0));
                            cr.set_source_rgb(fg_r, fg_g, fg_b);
                            let title_y = match action.appearance.vertical_align {
                                crate::models::TextVerticalAlign::Top => 12.0,
                                crate::models::TextVerticalAlign::Center => if has_card_icon { 44.0 } else { 36.0 },
                                crate::models::TextVerticalAlign::Bottom => 64.0,
                            };
                            cr.move_to(slot_x, title_y);
                            pc::show_layout(&cr, &layout);
                        }
                    }
                }
            }
        }

        drop(cr);
        let stride = surface.stride() as usize;
        let surface_data = surface.take_data().expect("Failed to read touchscreen surface");

        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let offset = y as usize * stride + x as usize * 4;
                let b_val = surface_data[offset];
                let g_val = surface_data[offset + 1];
                let r_val = surface_data[offset + 2];
                img.put_pixel(x, y, Rgb([r_val, g_val, b_val]));
            }
        }

        let mut jpeg_buf = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, 95);
        let _ = encoder.encode_image(&img);

        jpeg_buf
    }

    /// Syncs an entire profile page to physical hardware LCD keys and touchscreen
    pub fn sync_hardware_page(&self, dev_path: &Path, kind: DeviceKind, profile: &Profile) {
        let total_keys = kind.total_keys();
        if let Some(page) = profile.current_page() {
            for key in 0..total_keys {
                let action = page.keys.get(&key);
                let appearance = action.map(|a| a.active_appearance());
                let icon_name = action.and_then(|a| a.parameters.get("icon_name")).map(|s| s.as_str());
                let is_active = action.is_some();

                let key_anim_id = format!("key_{}_{}", dev_path.display(), key);
                let mut anim_lock = self.active_animations.lock().unwrap();

                // Check if this key has an active GIF
                let gif_path_opt = appearance.and_then(|a| a.icon_path.as_deref()).filter(|p| {
                    p.to_lowercase().ends_with(".gif") && Path::new(p).exists()
                });

                if let Some(path) = gif_path_opt {
                    let already_running = anim_lock.get(&key_anim_id).map(|(p, _)| p == path).unwrap_or(false);
                    if !already_running {
                        if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                            stop_flag.store(true, Ordering::SeqCst);
                        }
                        let stop_flag = Arc::new(AtomicBool::new(false));
                        anim_lock.insert(key_anim_id.clone(), (path.to_string(), stop_flag.clone()));
                        drop(anim_lock);

                        let (dim, rotate_180) = match kind {
                            DeviceKind::StreamDeckXL => (96u32, true),
                            DeviceKind::StreamDeckPlus => (120u32, false),
                            DeviceKind::StreamDeckPlusXL => (112u32, false),
                            DeviceKind::StreamDeckMini => (80u32, false),
                            _ => (72u32, true),
                        };

                        let dev_path_buf = dev_path.to_path_buf();
                        let path_str = path.to_string();
                        let write_lock_clone = self.write_lock.clone();

                        thread::spawn(move || {
                            if let Some(anim) = GifAnimation::load_key_gif(&path_str, dim, rotate_180) {
                                let mut frame_idx = 0;
                                while !stop_flag.load(Ordering::SeqCst) {
                                    let (ref jpeg, delay_ms) = anim.frames[frame_idx];
                                    {
                                        let _guard = write_lock_clone.lock().unwrap();
                                        let _ = Self::write_key_image_direct(&dev_path_buf, key, jpeg);
                                    }
                                    frame_idx = (frame_idx + 1) % anim.frames.len();
                                    thread::sleep(Duration::from_millis(delay_ms as u64));
                                }
                            }
                        });
                        continue;
                    } else {
                        // Already running this GIF animation
                        continue;
                    }
                }

                // If not a GIF, stop any running animation for this key
                if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                drop(anim_lock);

                let jpeg = self.render_key_jpeg(kind, key, appearance, icon_name, is_active);
                if let Err(e) = self.write_key_image(dev_path, key, &jpeg) {
                    tracing::error!("Failed to write key {} image to {}: {}", key, dev_path.display(), e);
                } else if is_active {
                    tracing::info!("Successfully wrote key {} (active) image to {}", key, dev_path.display());
                }
            }
        }

        // Stream Deck + also has an 800x100 touchscreen strip
        if kind == DeviceKind::StreamDeckPlus {
            let touch_anim_id = format!("touch_{}", dev_path.display());
            let mut anim_lock = self.active_animations.lock().unwrap();

            let touch_gif_path = profile.current_page()
                .and_then(|p| p.touchscreen_action.as_ref())
                .and_then(|a| a.active_appearance().icon_path.as_deref())
                .filter(|p| p.to_lowercase().ends_with(".gif") && Path::new(p).exists());

            if let Some(path) = touch_gif_path {
                let already_running = anim_lock.get(&touch_anim_id).map(|(p, _)| p == path).unwrap_or(false);
                if !already_running {
                    if let Some((_, stop_flag)) = anim_lock.remove(&touch_anim_id) {
                        stop_flag.store(true, Ordering::SeqCst);
                    }
                    let stop_flag = Arc::new(AtomicBool::new(false));
                    anim_lock.insert(touch_anim_id.clone(), (path.to_string(), stop_flag.clone()));
                    drop(anim_lock);

                    let dev_path_buf = dev_path.to_path_buf();
                    let path_str = path.to_string();
                    let write_lock_clone = self.write_lock.clone();

                    thread::spawn(move || {
                        if let Some(anim) = GifAnimation::load_touchscreen_gif(&path_str) {
                            let mut frame_idx = 0;
                            while !stop_flag.load(Ordering::SeqCst) {
                                let (ref jpeg, delay_ms) = anim.frames[frame_idx];
                                {
                                    let _guard = write_lock_clone.lock().unwrap();
                                    let _ = Self::write_touchscreen_image_direct(&dev_path_buf, 0, 0, 800, 100, jpeg);
                                }
                                frame_idx = (frame_idx + 1) % anim.frames.len();
                                thread::sleep(Duration::from_millis(delay_ms as u64));
                            }
                        }
                    });
                    return;
                } else {
                    return;
                }
            }

            // Stop any running touchscreen animation
            if let Some((_, stop_flag)) = anim_lock.remove(&touch_anim_id) {
                stop_flag.store(true, Ordering::SeqCst);
            }
            drop(anim_lock);

            let touch_jpeg = self.render_touchscreen_jpeg(profile);
            if let Err(e) = self.write_touchscreen_image(dev_path, 0, 0, 800, 100, &touch_jpeg) {
                tracing::error!("Failed to write touchscreen image to {}: {}", dev_path.display(), e);
            } else {
                tracing::info!("Successfully wrote touchscreen image to {}", dev_path.display());
            }
        }
    }

    /// Spawn background thread to poll USB input reports and deliver key, dial, and touch events
    pub fn start_listener<F>(&self, dev_path: PathBuf, kind: DeviceKind, callback: F)
    where
        F: Fn(HardwareEvent) + Send + 'static,
    {
        self.running.store(false, Ordering::SeqCst);
        let running = Arc::new(AtomicBool::new(true));
        self.running.store(true, Ordering::SeqCst);
        let running_thread = running.clone();

        thread::spawn(move || {
            let mut file = match OpenOptions::new().read(true).open(&dev_path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("Failed to open {} for reading: {e}", dev_path.display());
                    return;
                }
            };

            let mut prev_keys = vec![false; kind.total_keys()];
            let mut prev_dial_press = [false; 4];
            let mut buf = [0u8; 128];

            while running_thread.load(Ordering::SeqCst) {
                match file.read(&mut buf) {
                    Ok(n) if n >= 4 && buf[0] == 0x01 => {
                        match kind {
                            DeviceKind::StreamDeckXL | DeviceKind::StreamDeckMK2 | DeviceKind::StreamDeckMini => {
                                let key_count = kind.total_keys();
                                for i in 0..key_count {
                                    if 4 + i < n {
                                        let is_down = buf[4 + i] == 1;
                                        if is_down && !prev_keys[i] {
                                            callback(HardwareEvent::KeyDown { key_index: i });
                                        } else if !is_down && prev_keys[i] {
                                            callback(HardwareEvent::KeyUp { key_index: i });
                                        }
                                        prev_keys[i] = is_down;
                                    }
                                }
                            }
                            DeviceKind::StreamDeckPlus => {
                                if buf[1] == 0x00 {
                                    // 8 LCD Keys
                                    for i in 0..8 {
                                        if 4 + i < n {
                                            let is_down = buf[4 + i] == 1;
                                            if is_down && !prev_keys[i] {
                                                callback(HardwareEvent::KeyDown { key_index: i });
                                            } else if !is_down && prev_keys[i] {
                                                callback(HardwareEvent::KeyUp { key_index: i });
                                            }
                                            prev_keys[i] = is_down;
                                        }
                                    }
                                } else if buf[1] == 0x02 && n >= 9 {
                                    // Touchscreen Event: buf[4]: 1=Short, 2=Long, 3=Drag
                                    let event_type = buf[4];
                                    let x = (buf[5] as u16) | ((buf[6] as u16) << 8);
                                    let y = (buf[7] as u16) | ((buf[8] as u16) << 8);
                                    if event_type == 0x03 && n >= 13 {
                                        let start_x = (buf[9] as u16) | ((buf[10] as u16) << 8);
                                        if x > start_x + 50 {
                                            callback(HardwareEvent::TouchSwipe { is_right: true });
                                        } else if start_x > x + 50 {
                                            callback(HardwareEvent::TouchSwipe { is_right: false });
                                        }
                                    } else {
                                        let slot = ((x / 200) as usize).min(3);
                                        callback(HardwareEvent::TouchTap { slot, x, y });
                                    }
                                } else if buf[1] == 0x03 && n >= 9 {
                                    // Dial Event: buf[4]: 0x01=Turn, 0x00=Push
                                    if buf[4] == 0x01 {
                                        // Turn
                                        for i in 0..4 {
                                            let raw_val = buf[5 + i];
                                            if raw_val != 0 {
                                                let delta = if raw_val < 0x80 {
                                                    raw_val as i32
                                                } else {
                                                    -((0x100 - raw_val as u16) as i32)
                                                };
                                                callback(HardwareEvent::DialRotate { dial_index: i, delta });
                                            }
                                        }
                                    } else if buf[4] == 0x00 {
                                        // Push / Release
                                        for i in 0..4 {
                                            let is_down = buf[5 + i] == 1;
                                            if is_down && !prev_dial_press[i] {
                                                callback(HardwareEvent::DialPress { dial_index: i });
                                            } else if !is_down && prev_dial_press[i] {
                                                callback(HardwareEvent::DialRelease { dial_index: i });
                                            }
                                            prev_dial_press[i] = is_down;
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {
                        thread::sleep(std::time::Duration::from_millis(50));
                    }
                }
            }
        });
    }
}
