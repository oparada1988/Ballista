use crate::core::animation::{apply_hardware_transform, encode_hardware_image, GifAnimation};
use crate::models::{
    DeviceKind, KeyAppearance, KeyImageFormat, Profile, TextVerticalAlign, ELGATO_VENDOR_ID,
};
use cairo::{Context, Format, ImageSurface};
use gtk::gdk::prelude::GdkCairoContextExt;
use hidapi::{HidApi, HidDevice};
use image::{ImageBuffer, Rgb};
use pangocairo::functions as pc;
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
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
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
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
    hid_api: Arc<Mutex<Option<HidApi>>>,
    open_handles: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<HidDevice>>>>>,
    device_kinds: Arc<Mutex<HashMap<PathBuf, DeviceKind>>>,
    listener_stop: Arc<Mutex<Option<Arc<AtomicBool>>>>,
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

fn convert_original_key_index(key: usize) -> usize {
    let key_col = key % 5;
    (key - key_col) + (4 - key_col)
}

fn decode_dial_delta(raw_val: u8) -> i32 {
    if raw_val < 0x80 {
        raw_val as i32
    } else {
        -((0x100 - raw_val as u16) as i32)
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
        if let Some(resolved) = crate::ui::canvas::resolve_icon_path(name, max_size) {
            if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(&resolved, max_size, max_size, true) {
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
    }
    None
}

impl HardwareManager {
    pub fn new() -> Self {
        let api = match HidApi::new() {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::error!("Failed to initialize libusb HIDAPI transport: {e}");
                None
            }
        };
        Self {
            hid_api: Arc::new(Mutex::new(api)),
            open_handles: Arc::new(Mutex::new(HashMap::new())),
            device_kinds: Arc::new(Mutex::new(HashMap::new())),
            listener_stop: Arc::new(Mutex::new(None)),
            active_animations: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Enumerates all connected Elgato Stream Deck devices over USB HID (libusb backend,
    /// matching python-elgato-streamdeck LibUSBHIDAPI so devices are always found regardless
    /// of whether Linux `usbhid` kernel driver is attached or detached).
    pub fn scan_devices(&self) -> Vec<ConnectedDevice> {
        let mut devices = Vec::new();
        let mut seen_paths: HashSet<String> = HashSet::new();

        let mut api_guard = self.hid_api.lock().unwrap();
        if api_guard.is_none() {
            if let Ok(a) = HidApi::new() {
                *api_guard = Some(a);
            }
        }

        if let Some(ref mut api) = *api_guard {
            if let Err(e) = api.refresh_devices() {
                tracing::warn!("HIDAPI refresh_devices warning: {e}");
            }

            let mut kind_map = self.device_kinds.lock().unwrap();
            for info in api.device_list() {
                if info.vendor_id() != ELGATO_VENDOR_ID {
                    continue;
                }
                let pid = info.product_id();
                if let Some((kind, display_name)) = DeviceKind::from_usb_pid(pid) {
                    let path_str = info.path().to_string_lossy().to_string();
                    if !seen_paths.insert(path_str.clone()) {
                        continue;
                    }
                    let dev_path = PathBuf::from(&path_str);
                    let serial = info
                        .serial_number()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty());

                    kind_map.insert(dev_path.clone(), kind);
                    tracing::info!(
                        "Detected Elgato Stream Deck: {} (VID={:#06x}, PID={:#06x}, serial={:?}, path={})",
                        display_name,
                        ELGATO_VENDOR_ID,
                        pid,
                        serial,
                        path_str
                    );

                    devices.push(ConnectedDevice {
                        kind,
                        name: display_name.to_string(),
                        dev_path,
                        vid: ELGATO_VENDOR_ID,
                        pid,
                        serial,
                    });
                }
            }
        }

        devices.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then_with(|| a.serial.cmp(&b.serial))
                .then_with(|| a.dev_path.cmp(&b.dev_path))
        });
        devices
    }

    /// Returns a persistent shared `HidDevice` handle for `dev_path`, opening via libusb if needed.
    fn get_or_open_device(&self, dev_path: &Path) -> Result<Arc<Mutex<HidDevice>>, String> {
        {
            let handles = self.open_handles.lock().unwrap();
            if let Some(existing) = handles.get(dev_path) {
                return Ok(existing.clone());
            }
        }

        let path_str = dev_path.to_string_lossy();
        let c_path = CString::new(path_str.as_bytes())
            .map_err(|e| format!("Invalid HID device path '{}': {e}", path_str))?;

        let api_guard = self.hid_api.lock().unwrap();
        let api = api_guard
            .as_ref()
            .ok_or_else(|| "HIDAPI transport is not initialized".to_string())?;

        let dev = api
            .open_path(&c_path)
            .map_err(|e| format!("Failed to open USB HID device '{}': {e}", path_str))?;

        // Match python-elgato-streamdeck LibUSBHIDAPI: always use non-blocking reads so the
        // input polling thread never holds the device mutex while waiting for USB reports!
        if let Err(e) = dev.set_blocking_mode(false) {
            tracing::warn!("Could not set non-blocking mode on {}: {e}", path_str);
        }

        let arc = Arc::new(Mutex::new(dev));
        self.open_handles
            .lock()
            .unwrap()
            .insert(dev_path.to_path_buf(), arc.clone());
        Ok(arc)
    }

    fn lookup_kind(&self, dev_path: &Path) -> DeviceKind {
        self.device_kinds
            .lock()
            .unwrap()
            .get(dev_path)
            .copied()
            .unwrap_or(DeviceKind::StreamDeckMK2)
    }

    /// Stops all background listeners/animations and releases USB device handles cleanly.
    pub fn shutdown(&self) {
        if let Some(stop) = self.listener_stop.lock().unwrap().take() {
            stop.store(true, Ordering::SeqCst);
        }
        {
            let mut anims = self.active_animations.lock().unwrap();
            for (_, (_, flag)) in anims.drain() {
                flag.store(true, Ordering::SeqCst);
            }
        }
        self.open_handles.lock().unwrap().clear();
    }

    /// Send hardware brightness feature report (0-100%) using the exact Gen1 or Gen2 protocol
    pub fn set_brightness(&self, dev_path: &Path, brightness: u8) -> Result<(), String> {
        let kind = self.lookup_kind(dev_path);
        if !kind.is_visual() {
            return Ok(());
        }

        let dev_arc = self.get_or_open_device(dev_path)?;
        let dev = dev_arc.lock().unwrap();
        let pct = brightness.min(100);

        match kind {
            DeviceKind::StreamDeckOriginal | DeviceKind::StreamDeckMini => {
                let mut payload = [0u8; 17];
                payload[0..6].copy_from_slice(&[0x05, 0x55, 0xAA, 0xD1, 0x01, pct]);
                dev.send_feature_report(&payload)
                    .map_err(|e| format!("Gen1 set_brightness failed: {e}"))?;
            }
            _ => {
                let mut payload = [0u8; 32];
                payload[0..3].copy_from_slice(&[0x03, 0x08, pct]);
                dev.send_feature_report(&payload)
                    .map_err(|e| format!("Gen2 set_brightness failed: {e}"))?;
            }
        }

        Ok(())
    }

    /// Reset deck display / show default logo
    #[allow(dead_code)]
    pub fn reset_device(&self, dev_path: &Path, kind: DeviceKind) -> Result<(), String> {
        if !kind.is_visual() {
            return Ok(());
        }
        let dev_arc = self.get_or_open_device(dev_path)?;
        let dev = dev_arc.lock().unwrap();
        match kind {
            DeviceKind::StreamDeckOriginal | DeviceKind::StreamDeckMini => {
                let mut payload = [0u8; 17];
                payload[0..2].copy_from_slice(&[0x0B, 0x63]);
                dev.send_feature_report(&payload)
                    .map_err(|e| format!("Gen1 reset failed: {e}"))?;
            }
            _ => {
                let mut payload = [0u8; 32];
                payload[0..2].copy_from_slice(&[0x03, 0x02]);
                dev.send_feature_report(&payload)
                    .map_err(|e| format!("Gen2 reset failed: {e}"))?;
            }
        }
        Ok(())
    }

    /// Write an encoded key image (BMP for Original/Mini, JPEG for Gen2) directly to an open HidDevice
    pub fn write_key_image_direct(
        dev: &HidDevice,
        kind: DeviceKind,
        key: usize,
        image_data: &[u8],
    ) -> Result<(), String> {
        match kind {
            DeviceKind::StreamDeckPedal => Ok(()),
            DeviceKind::StreamDeckOriginal => {
                const REPORT_LEN: usize = 8191;
                let payload_len = (image_data.len() / 2).max(1);
                let hw_key = convert_original_key_index(key);
                let mut bytes_remaining = image_data.len();
                let mut page_number: usize = 0;

                while bytes_remaining > 0 {
                    let this_length = std::cmp::min(bytes_remaining, payload_len);
                    let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };
                    let bytes_sent = page_number * payload_len;

                    let mut packet = vec![0u8; REPORT_LEN];
                    packet[0] = 0x02;
                    packet[1] = 0x01;
                    packet[2] = (page_number + 1) as u8;
                    packet[3] = 0x00;
                    packet[4] = is_last;
                    packet[5] = (hw_key + 1) as u8;

                    let copy_len = this_length.min(REPORT_LEN - 16);
                    packet[16..16 + copy_len]
                        .copy_from_slice(&image_data[bytes_sent..bytes_sent + copy_len]);

                    dev.write(&packet)
                        .map_err(|e| format!("StreamDeckOriginal write_key_image failed: {e}"))?;

                    bytes_remaining -= this_length;
                    page_number += 1;
                }
                Ok(())
            }
            DeviceKind::StreamDeckMini => {
                const REPORT_LEN: usize = 1024;
                const HEADER_LEN: usize = 16;
                const PAYLOAD_LEN: usize = REPORT_LEN - HEADER_LEN; // 1008 bytes

                let mut bytes_remaining = image_data.len();
                let mut page_number: usize = 0;

                while bytes_remaining > 0 {
                    let this_length = std::cmp::min(bytes_remaining, PAYLOAD_LEN);
                    let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };
                    let bytes_sent = page_number * PAYLOAD_LEN;

                    let mut packet = [0u8; REPORT_LEN];
                    packet[0] = 0x02;
                    packet[1] = 0x01;
                    packet[2] = (page_number & 0xFF) as u8;
                    packet[3] = 0x00;
                    packet[4] = is_last;
                    packet[5] = (key + 1) as u8;

                    packet[HEADER_LEN..HEADER_LEN + this_length]
                        .copy_from_slice(&image_data[bytes_sent..bytes_sent + this_length]);

                    dev.write(&packet)
                        .map_err(|e| format!("StreamDeckMini write_key_image failed: {e}"))?;

                    bytes_remaining -= this_length;
                    page_number += 1;
                }
                Ok(())
            }
            _ => {
                // Gen2 JPEG protocol: MK.2, Original V2, XL, XL V2, Neo, Plus, Plus XL, Studio
                const REPORT_LEN: usize = 1024;
                const HEADER_LEN: usize = 8;
                const PAYLOAD_LEN: usize = REPORT_LEN - HEADER_LEN; // 1016 bytes

                let mut bytes_remaining = image_data.len();
                let mut page_number: u16 = 0;

                while bytes_remaining > 0 {
                    let this_length = std::cmp::min(bytes_remaining, PAYLOAD_LEN);
                    let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };
                    let bytes_sent = page_number as usize * PAYLOAD_LEN;

                    let mut packet = [0u8; REPORT_LEN];
                    packet[0] = 0x02; // Output Report ID
                    packet[1] = 0x07; // Set key image command
                    packet[2] = (key & 0xFF) as u8;
                    packet[3] = is_last;
                    packet[4] = (this_length & 0xFF) as u8;
                    packet[5] = ((this_length >> 8) & 0xFF) as u8;
                    packet[6] = (page_number & 0xFF) as u8;
                    packet[7] = ((page_number >> 8) & 0xFF) as u8;

                    packet[HEADER_LEN..HEADER_LEN + this_length]
                        .copy_from_slice(&image_data[bytes_sent..bytes_sent + this_length]);

                    dev.write(&packet)
                        .map_err(|e| format!("Gen2 write_key_image failed: {e}"))?;

                    page_number += 1;
                    bytes_remaining -= this_length;
                }
                Ok(())
            }
        }
    }

    pub fn write_key_image(
        &self,
        dev_path: &Path,
        kind: DeviceKind,
        key: usize,
        image_data: &[u8],
    ) -> Result<(), String> {
        let dev_arc = self.get_or_open_device(dev_path)?;
        let dev = dev_arc.lock().unwrap();
        Self::write_key_image_direct(&dev, kind, key, image_data)
    }

    /// Write a touchscreen strip JPEG to Stream Deck + (800x100) or Stream Deck + XL (1200x100 rotated 90° CCW)
    pub fn write_touchscreen_image_direct(
        dev: &HidDevice,
        kind: DeviceKind,
        x_pos: u16,
        y_pos: u16,
        width: u16,
        height: u16,
        jpeg_data: &[u8],
    ) -> Result<(), String> {
        const REPORT_LEN: usize = 1024;
        const HEADER_LEN: usize = 16;
        const PAYLOAD_LEN: usize = REPORT_LEN - HEADER_LEN; // 1008 bytes

        // Per python-elgato-streamdeck StreamDeckPlusXL.py:
        // Stream Deck + XL hardware panel is mounted rotated 90°, so x/y and width/height swap in header!
        let (int_x, int_y, int_w, int_h) = if kind == DeviceKind::StreamDeckPlusXL {
            (y_pos, x_pos, height, width)
        } else {
            (x_pos, y_pos, width, height)
        };

        let mut bytes_remaining = jpeg_data.len();
        let mut page_number: u16 = 0;

        while bytes_remaining > 0 {
            let this_length = std::cmp::min(bytes_remaining, PAYLOAD_LEN);
            let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };
            let bytes_sent = page_number as usize * PAYLOAD_LEN;

            let mut packet = [0u8; REPORT_LEN];
            packet[0] = 0x02; // Output Report ID
            packet[1] = 0x0C; // Set touchscreen image command
            packet[2] = (int_x & 0xFF) as u8;
            packet[3] = ((int_x >> 8) & 0xFF) as u8;
            packet[4] = (int_y & 0xFF) as u8;
            packet[5] = ((int_y >> 8) & 0xFF) as u8;
            packet[6] = (int_w & 0xFF) as u8;
            packet[7] = ((int_w >> 8) & 0xFF) as u8;
            packet[8] = (int_h & 0xFF) as u8;
            packet[9] = ((int_h >> 8) & 0xFF) as u8;
            packet[10] = is_last;
            packet[11] = (page_number & 0xFF) as u8;
            packet[12] = ((page_number >> 8) & 0xFF) as u8;
            packet[13] = (this_length & 0xFF) as u8;
            packet[14] = ((this_length >> 8) & 0xFF) as u8;
            packet[15] = 0x00;

            packet[HEADER_LEN..HEADER_LEN + this_length]
                .copy_from_slice(&jpeg_data[bytes_sent..bytes_sent + this_length]);

            dev.write(&packet)
                .map_err(|e| format!("write_touchscreen_image failed: {e}"))?;

            page_number += 1;
            bytes_remaining -= this_length;
        }

        Ok(())
    }

    pub fn write_touchscreen_image(
        &self,
        dev_path: &Path,
        kind: DeviceKind,
        x_pos: u16,
        y_pos: u16,
        width: u16,
        height: u16,
        jpeg_data: &[u8],
    ) -> Result<(), String> {
        let dev_arc = self.get_or_open_device(dev_path)?;
        let dev = dev_arc.lock().unwrap();
        Self::write_touchscreen_image_direct(&dev, kind, x_pos, y_pos, width, height, jpeg_data)
    }

    /// Write the 248x58 JPEG Info Bar image to Stream Deck Neo (command 0x02 0x0B)
    pub fn write_neo_screen_image_direct(dev: &HidDevice, jpeg_data: &[u8]) -> Result<(), String> {
        const REPORT_LEN: usize = 1024;
        const HEADER_LEN: usize = 8;
        const PAYLOAD_LEN: usize = REPORT_LEN - HEADER_LEN;

        let mut bytes_remaining = jpeg_data.len();
        let mut page_number: u16 = 0;

        while bytes_remaining > 0 {
            let this_length = std::cmp::min(bytes_remaining, PAYLOAD_LEN);
            let is_last: u8 = if this_length == bytes_remaining { 1 } else { 0 };
            let bytes_sent = page_number as usize * PAYLOAD_LEN;

            let mut packet = [0u8; REPORT_LEN];
            packet[0] = 0x02;
            packet[1] = 0x0B; // Neo Info Bar screen image command
            packet[2] = 0x00;
            packet[3] = is_last;
            packet[4] = (this_length & 0xFF) as u8;
            packet[5] = ((this_length >> 8) & 0xFF) as u8;
            packet[6] = (page_number & 0xFF) as u8;
            packet[7] = ((page_number >> 8) & 0xFF) as u8;

            packet[HEADER_LEN..HEADER_LEN + this_length]
                .copy_from_slice(&jpeg_data[bytes_sent..bytes_sent + this_length]);

            dev.write(&packet)
                .map_err(|e| format!("write_neo_screen_image failed: {e}"))?;

            page_number += 1;
            bytes_remaining -= this_length;
        }
        Ok(())
    }

    /// Renders a key image buffer formatted natively (dimensions, rotation, flip, BMP or JPEG) for the hardware
    pub fn render_key_image(
        &self,
        kind: DeviceKind,
        _key_idx: usize,
        appearance: Option<&KeyAppearance>,
        icon_name: Option<&str>,
        is_active: bool,
    ) -> Vec<u8> {
        let Some((w, h)) = kind.key_image_size() else {
            return Vec::new();
        };
        let min_dim = w.min(h);

        let surface = ImageSurface::create(Format::Rgb24, w as i32, h as i32)
            .expect("Failed to create Cairo image surface");
        let cr = Context::new(&surface).expect("Failed to create Cairo context");

        // Pure Black Hardware Contract:
        // Empty / unassigned keys are rendered 100% pure black with NO border and NO text
        if !is_active {
            cr.set_source_rgb(0.0, 0.0, 0.0);
            let _ = cr.paint();
        } else {
            let custom_bg = appearance
                .and_then(|a| a.bg_color.as_deref())
                .and_then(parse_hex_color);
            let (r, g, b) = custom_bg.unwrap_or((0.0, 0.0, 0.0));

            cr.set_source_rgb(r, g, b);
            let _ = cr.paint();

            if custom_bg.is_some() {
                cr.set_source_rgb(
                    (r + 0.15).min(1.0),
                    (g + 0.15).min(1.0),
                    (b + 0.15).min(1.0),
                );
                cr.set_line_width(2.0);
                cr.rectangle(1.0, 1.0, (w - 2) as f64, (h - 2) as f64);
                let _ = cr.stroke();
            }

            let mut icon_path = appearance.and_then(|a| a.icon_path.as_deref());
            if let Some(p) = icon_path {
                if (p.ends_with("action_launch.png") || p.ends_with("default_app.png"))
                    && icon_name.is_some()
                {
                    icon_path = None;
                }
            }
            let scale_pct =
                appearance.map(|a| a.icon_scale).unwrap_or(100).clamp(20, 100) as f64 / 100.0;
            let icon_max = ((min_dim as f64 * scale_pct) as i32).max(12);

            if let Some(pixbuf) = load_icon(icon_path, icon_name, icon_max) {
                let icon_w = pixbuf.width() as f64;
                let icon_h = pixbuf.height() as f64;
                let icon_x = ((w as f64 - icon_w) / 2.0).max(0.0);
                let icon_y = ((h as f64 - icon_h) / 2.0).max(0.0);

                cr.set_source_pixbuf(&pixbuf, icon_x, icon_y);
                let _ = cr.paint();
            }

            if let Some(app) = appearance {
                let draw_overlay_text = |text: &str, align: TextVerticalAlign| {
                    if text.is_empty() {
                        return;
                    }
                    let layout = pc::create_layout(&cr);
                    let scale_factor = min_dim as f64 / 96.0;
                    let target_size =
                        ((app.font_size as f64 * scale_factor).max(8.0) * pango::SCALE as f64)
                            as i32;

                    let mut font_desc = pango::FontDescription::new();
                    font_desc.set_family(&app.font_family);
                    font_desc.set_size(target_size);
                    font_desc.set_weight(pango::Weight::Bold);
                    layout.set_font_description(Some(&font_desc));

                    layout.set_text(text);
                    layout.set_alignment(pango::Alignment::Center);
                    layout.set_width(((w - 8) as i32) * pango::SCALE);
                    layout.set_wrap(pango::WrapMode::WordChar);

                    let (_, extents) = layout.pixel_extents();
                    let text_h = extents.height() as f64;

                    let text_y = match align {
                        TextVerticalAlign::Top => 4.0,
                        TextVerticalAlign::Center => ((h as f64 - text_h) / 2.0).max(2.0),
                        TextVerticalAlign::Bottom => (h as f64 - text_h - 4.0).max(2.0),
                    };

                    cr.set_source_rgba(0.0, 0.0, 0.0, 0.90);
                    for (dx, dy) in [
                        (-1.0, 0.0),
                        (1.0, 0.0),
                        (0.0, -1.0),
                        (0.0, 1.0),
                        (1.0, 1.0),
                    ] {
                        cr.move_to(4.0 + dx, text_y + dy);
                        pc::show_layout(&cr, &layout);
                    }

                    let (tr, tg, tb) =
                        parse_hex_color(&app.text_color).unwrap_or((1.0, 1.0, 1.0));
                    cr.set_source_rgb(tr, tg, tb);
                    cr.move_to(4.0, text_y);
                    pc::show_layout(&cr, &layout);
                };

                if app.has_plugin_labels() {
                    if let Some(ref t) = app.top_label {
                        draw_overlay_text(t, TextVerticalAlign::Top);
                    }
                    if let Some(ref c) = app.center_label {
                        draw_overlay_text(c, TextVerticalAlign::Center);
                    }
                    if let Some(ref b) = app.bottom_label {
                        draw_overlay_text(b, TextVerticalAlign::Bottom);
                    }
                } else if app.show_title && !app.title.is_empty() {
                    draw_overlay_text(&app.title, app.vertical_align);
                }
            }
        }

        drop(cr);
        let stride = surface.stride() as usize;
        let surface_data = surface.take_data().expect("Failed to read surface data");

        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let offset = y as usize * stride + x as usize * 4;
                let b_val = surface_data[offset];
                let g_val = surface_data[offset + 1];
                let r_val = surface_data[offset + 2];
                img.put_pixel(x, y, Rgb([r_val, g_val, b_val]));
            }
        }

        let (rot_ccw, flip_x, flip_y) = kind.key_image_transform();
        let transformed = apply_hardware_transform(img, rot_ccw, flip_x, flip_y);
        encode_hardware_image(&transformed, kind.key_image_format(), 95)
    }

    /// Renders the touchscreen strip JPEG for Stream Deck + (800x100) or Stream Deck + XL (1200x100 -> rotated 90° CCW)
    pub fn render_touchscreen_jpeg(&self, kind: DeviceKind, profile: &Profile) -> Vec<u8> {
        let (width, height) = kind.touchscreen_size().unwrap_or((800, 100));
        let dial_count = kind.dial_count().max(1);
        let slot_w = (width as f64) / (dial_count as f64);

        let surface = ImageSurface::create(Format::Rgb24, width as i32, height as i32)
            .expect("Failed to create Touchscreen Cairo surface");
        let cr = Context::new(&surface).expect("Failed to create Touchscreen Cairo context");

        cr.set_source_rgb(0.0, 0.0, 0.0);
        let _ = cr.paint();

        if let Some(page) = profile.current_page() {
            let has_panoramic_content = page.touchscreen_action.as_ref().map_or(false, |act| {
                let app = act.active_appearance();
                !app.title.is_empty()
                    || app.bg_color.is_some()
                    || app.icon_path.is_some()
                    || (act.action_id != "custom"
                        && act.action_id != "touchscreen"
                        && act.action_id != "switch_pages"
                        && act.action_id != "disabled"
                        && !act.action_name.is_empty())
            });

            if has_panoramic_content {
                if let Some(ref action) = page.touchscreen_action {
                    let app = action.active_appearance();
                    let title = if !app.show_title || app.title.is_empty() {
                        ""
                    } else {
                        &app.title
                    };

                    if let Some(ref bg_hex) = app.bg_color {
                        if let Some((bg_r, bg_g, bg_b)) = parse_hex_color(bg_hex) {
                            cr.set_source_rgb(bg_r, bg_g, bg_b);
                            cr.rectangle(0.0, 0.0, width as f64, height as f64);
                            let _ = cr.fill();
                        }
                    }

                    if let Some(ref icon_path) = app.icon_path {
                        if Path::new(icon_path).exists() {
                            let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
                            let target_w = (width as f64 * scale_pct) as i32;
                            let target_h = (height as f64 * scale_pct) as i32;
                            if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(
                                icon_path, target_w, target_h, false,
                            ) {
                                let pw = pixbuf.width() as f64;
                                let ph = pixbuf.height() as f64;
                                let px = (width as f64 - pw) / 2.0;
                                let py = (height as f64 - ph) / 2.0;
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
                        layout.set_width((width as i32) * pango::SCALE);

                        let (fg_r, fg_g, fg_b) =
                            parse_hex_color(&app.text_color).unwrap_or((1.0, 1.0, 1.0));
                        cr.set_source_rgb(fg_r, fg_g, fg_b);
                        cr.move_to(0.0, 36.0);
                        pc::show_layout(&cr, &layout);
                    }
                }
            } else {
                for i in 0..dial_count {
                    if let Some(action) = page.dial_actions.get(&i) {
                        let slot_x = (i as f64) * slot_w;
                        let app = action.active_appearance();
                        let summary_title = app.combined_display_summary();
                        let title = summary_title.as_str();

                        if let Some(ref bg_hex) = app.bg_color {
                            if let Some((bg_r, bg_g, bg_b)) = parse_hex_color(bg_hex) {
                                cr.set_source_rgb(bg_r, bg_g, bg_b);
                                cr.rectangle(slot_x, 0.0, slot_w, height as f64);
                                let _ = cr.fill();
                            }
                        }

                        let mut has_card_icon = false;
                        if let Some(ref path) = app.icon_path {
                            if Path::new(path).exists() {
                                let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
                                let base_sz = if title.is_empty() { 70.0 } else { 48.0 };
                                let icon_sz = ((base_sz * scale_pct) as i32).max(12);
                                if let Ok(pixbuf) = gdk_pixbuf::Pixbuf::from_file_at_scale(
                                    path, icon_sz, icon_sz, true,
                                ) {
                                    has_card_icon = true;
                                    let pw = pixbuf.width() as f64;
                                    let ph = pixbuf.height() as f64;
                                    let px = slot_x + (slot_w - pw) / 2.0;
                                    let py = if title.is_empty() {
                                        (height as f64 - ph) / 2.0
                                    } else {
                                        10.0
                                    };
                                    cr.set_source_pixbuf(&pixbuf, px, py);
                                    let _ = cr.paint();
                                }
                            }
                        }

                        let draw_dial_label = |text: &str, y_pos: f64| {
                            if text.is_empty() {
                                return;
                            }
                            let layout = pc::create_layout(&cr);
                            let mut font_desc = pango::FontDescription::new();
                            font_desc.set_family(&app.font_family);
                            font_desc.set_size(app.font_size as i32 * pango::SCALE);
                            font_desc.set_weight(pango::Weight::Bold);
                            layout.set_font_description(Some(&font_desc));
                            layout.set_text(text);
                            layout.set_alignment(pango::Alignment::Center);
                            layout.set_width((slot_w as i32) * pango::SCALE);

                            cr.set_source_rgba(0.0, 0.0, 0.0, 0.85);
                            cr.move_to(slot_x + 1.0, y_pos + 1.0);
                            pc::show_layout(&cr, &layout);

                            let (fg_r, fg_g, fg_b) =
                                parse_hex_color(&app.text_color).unwrap_or((1.0, 1.0, 1.0));
                            cr.set_source_rgb(fg_r, fg_g, fg_b);
                            cr.move_to(slot_x, y_pos);
                            pc::show_layout(&cr, &layout);
                        };

                        if app.has_plugin_labels() {
                            if let Some(ref t) = app.top_label {
                                draw_dial_label(t, 6.0);
                            }
                            if let Some(ref c) = app.center_label {
                                draw_dial_label(c, 38.0);
                            }
                            if let Some(ref b) = app.bottom_label {
                                draw_dial_label(b, 72.0);
                            }
                        } else if !title.is_empty() {
                            let title_y = match app.vertical_align {
                                TextVerticalAlign::Top => 12.0,
                                TextVerticalAlign::Center => {
                                    if has_card_icon {
                                        44.0
                                    } else {
                                        36.0
                                    }
                                }
                                TextVerticalAlign::Bottom => 64.0,
                            };
                            draw_dial_label(title, title_y);
                        }
                    }
                }
            }
        }

        drop(cr);
        let stride = surface.stride() as usize;
        let surface_data = surface
            .take_data()
            .expect("Failed to read touchscreen surface");

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

        let transformed =
            apply_hardware_transform(img, kind.touchscreen_rotation_ccw(), false, false);
        encode_hardware_image(&transformed, KeyImageFormat::Jpeg, 95)
    }

    /// Renders the 248x58 Info Bar LCD for Stream Deck Neo (flipped XY = 180° per StreamDeckNeo.py)
    pub fn render_neo_info_bar_jpeg(&self, profile: &Profile) -> Vec<u8> {
        let width = 248u32;
        let height = 58u32;

        let surface = ImageSurface::create(Format::Rgb24, width as i32, height as i32)
            .expect("Failed to create Neo InfoBar Cairo surface");
        let cr = Context::new(&surface).expect("Failed to create Neo InfoBar Cairo context");

        cr.set_source_rgb(0.0, 0.0, 0.0);
        let _ = cr.paint();

        let page_num = profile.current_page_idx + 1;
        let page_total = profile.pages.len().max(1);
        let label_text = format!("Page {page_num} / {page_total}");

        let layout = pc::create_layout(&cr);
        let mut font_desc = pango::FontDescription::new();
        font_desc.set_family("Cantarell");
        font_desc.set_size(14 * pango::SCALE);
        font_desc.set_weight(pango::Weight::Bold);
        layout.set_font_description(Some(&font_desc));
        layout.set_text(&label_text);
        layout.set_alignment(pango::Alignment::Center);
        layout.set_width((width as i32) * pango::SCALE);

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.move_to(0.0, 17.0);
        pc::show_layout(&cr, &layout);

        drop(cr);
        let stride = surface.stride() as usize;
        let surface_data = surface
            .take_data()
            .expect("Failed to read Neo InfoBar surface");

        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let offset = y as usize * stride + x as usize * 4;
                img.put_pixel(
                    x,
                    y,
                    Rgb([
                        surface_data[offset + 2],
                        surface_data[offset + 1],
                        surface_data[offset],
                    ]),
                );
            }
        }

        let transformed = apply_hardware_transform(img, 0, true, true);
        encode_hardware_image(&transformed, KeyImageFormat::Jpeg, 95)
    }

    /// Syncs an entire profile page to physical hardware LCD keys, touchscreen strip, and Neo info bar
    pub fn sync_hardware_page(&self, dev_path: &Path, kind: DeviceKind, profile: &Profile) {
        if !kind.is_visual() {
            return;
        }

        let dev_arc = match self.get_or_open_device(dev_path) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Cannot open {} to sync hardware page: {e}", dev_path.display());
                return;
            }
        };

        let total_keys = kind.total_keys();
        if let Some(page) = profile.current_page() {
            for key in 0..total_keys {
                let action = page.keys.get(&key);
                let appearance = action.map(|a| a.active_appearance());
                let icon_name = action
                    .and_then(|a| a.parameters.get("icon_name"))
                    .map(|s| s.as_str());
                let is_active = action.is_some();

                let key_anim_id = format!("key_{}_{}", dev_path.display(), key);
                let mut anim_lock = self.active_animations.lock().unwrap();

                let gif_path_opt = appearance
                    .and_then(|a| a.icon_path.as_deref())
                    .filter(|p| p.to_lowercase().ends_with(".gif") && Path::new(p).exists());

                if let Some(path) = gif_path_opt {
                    let already_running = anim_lock
                        .get(&key_anim_id)
                        .map(|(p, _)| p == path)
                        .unwrap_or(false);
                    if !already_running {
                        if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                            stop_flag.store(true, Ordering::SeqCst);
                        }
                        let stop_flag = Arc::new(AtomicBool::new(false));
                        anim_lock
                            .insert(key_anim_id.clone(), (path.to_string(), stop_flag.clone()));
                        drop(anim_lock);

                        let dev_arc_anim = dev_arc.clone();
                        let path_str = path.to_string();

                        thread::spawn(move || {
                            if let Some(anim) = GifAnimation::load_key_gif(&path_str, kind) {
                                let mut frame_idx = 0;
                                while !stop_flag.load(Ordering::SeqCst) {
                                    let (ref frame_bytes, delay_ms) = anim.frames[frame_idx];
                                    {
                                        let dev = dev_arc_anim.lock().unwrap();
                                        let _ = Self::write_key_image_direct(
                                            &dev,
                                            kind,
                                            key,
                                            frame_bytes,
                                        );
                                    }
                                    frame_idx = (frame_idx + 1) % anim.frames.len();
                                    thread::sleep(Duration::from_millis(delay_ms as u64));
                                }
                            }
                        });
                        continue;
                    } else {
                        continue;
                    }
                }

                if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                drop(anim_lock);

                let encoded = self.render_key_image(kind, key, appearance, icon_name, is_active);
                let res = {
                    let dev = dev_arc.lock().unwrap();
                    Self::write_key_image_direct(&dev, kind, key, &encoded)
                };
                if let Err(e) = res {
                    tracing::error!(
                        "Failed to write key {} image to {}: {}",
                        key,
                        dev_path.display(),
                        e
                    );
                }
            }
        }

        // Stream Deck + (800x100) and Stream Deck + XL (1200x100) Touchscreen strip
        if let Some((tw, th)) = kind.touchscreen_size() {
            let touch_anim_id = format!("touch_{}", dev_path.display());
            let mut anim_lock = self.active_animations.lock().unwrap();

            let touch_gif_path = profile
                .current_page()
                .and_then(|p| p.touchscreen_action.as_ref())
                .and_then(|a| a.active_appearance().icon_path.as_deref())
                .filter(|p| p.to_lowercase().ends_with(".gif") && Path::new(p).exists());

            if let Some(path) = touch_gif_path {
                let already_running = anim_lock
                    .get(&touch_anim_id)
                    .map(|(p, _)| p == path)
                    .unwrap_or(false);
                if !already_running {
                    if let Some((_, stop_flag)) = anim_lock.remove(&touch_anim_id) {
                        stop_flag.store(true, Ordering::SeqCst);
                    }
                    let stop_flag = Arc::new(AtomicBool::new(false));
                    anim_lock.insert(touch_anim_id.clone(), (path.to_string(), stop_flag.clone()));
                    drop(anim_lock);

                    let dev_arc_anim = dev_arc.clone();
                    let path_str = path.to_string();

                    thread::spawn(move || {
                        if let Some(anim) = GifAnimation::load_touchscreen_gif(&path_str, kind) {
                            let mut frame_idx = 0;
                            while !stop_flag.load(Ordering::SeqCst) {
                                let (ref jpeg, delay_ms) = anim.frames[frame_idx];
                                {
                                    let dev = dev_arc_anim.lock().unwrap();
                                    let _ = Self::write_touchscreen_image_direct(
                                        &dev,
                                        kind,
                                        0,
                                        0,
                                        tw as u16,
                                        th as u16,
                                        jpeg,
                                    );
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

            if let Some((_, stop_flag)) = anim_lock.remove(&touch_anim_id) {
                stop_flag.store(true, Ordering::SeqCst);
            }
            drop(anim_lock);

            let touch_jpeg = self.render_touchscreen_jpeg(kind, profile);
            let res = {
                let dev = dev_arc.lock().unwrap();
                Self::write_touchscreen_image_direct(
                    &dev,
                    kind,
                    0,
                    0,
                    tw as u16,
                    th as u16,
                    &touch_jpeg,
                )
            };
            if let Err(e) = res {
                tracing::error!(
                    "Failed to write touchscreen image to {}: {}",
                    dev_path.display(),
                    e
                );
            }
        }

        // Stream Deck Neo 248x58 Info Bar LCD
        if kind.has_info_screen() {
            let bar_jpeg = self.render_neo_info_bar_jpeg(profile);
            let dev = dev_arc.lock().unwrap();
            if let Err(e) = Self::write_neo_screen_image_direct(&dev, &bar_jpeg) {
                tracing::error!(
                    "Failed to write Neo info screen to {}: {}",
                    dev_path.display(),
                    e
                );
            }
        }
    }

    /// Spawn background listener thread polling USB HID input reports for any Stream Deck model
    pub fn start_listener<F>(&self, dev_path: PathBuf, kind: DeviceKind, callback: F)
    where
        F: Fn(HardwareEvent) + Send + 'static,
    {
        // Stop any existing listener thread cleanly before starting the new one
        if let Some(old_stop) = self.listener_stop.lock().unwrap().take() {
            old_stop.store(true, Ordering::SeqCst);
        }

        let dev_arc = match self.get_or_open_device(&dev_path) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(
                    "Failed to open {} for hardware input listening: {e}",
                    dev_path.display()
                );
                return;
            }
        };

        let stop_flag = Arc::new(AtomicBool::new(false));
        *self.listener_stop.lock().unwrap() = Some(stop_flag.clone());

        thread::spawn(move || {
            let key_count = kind.total_keys();
            let dial_count = kind.dial_count();
            let touch_key_count = kind.touch_key_count();

            let mut prev_keys = vec![false; key_count];
            let mut prev_touch_keys = vec![false; touch_key_count];
            let mut prev_dial_press = vec![false; dial_count];
            let mut buf = [0u8; 128];

            tracing::info!(
                "Started USB HID listener for {} ({}) [keys={}, dials={}]",
                kind.name(),
                dev_path.display(),
                key_count,
                dial_count
            );

            while !stop_flag.load(Ordering::SeqCst) {
                let read_res = {
                    let dev = dev_arc.lock().unwrap();
                    dev.read(&mut buf)
                };

                match read_res {
                    Ok(0) => {
                        // Sleep OUTSIDE the device mutex so writes & brightness updates acquire immediately
                        thread::sleep(Duration::from_millis(5));
                    }
                    Ok(n) if n >= 2 && buf[0] == 0x01 => match kind {
                        DeviceKind::StreamDeckOriginal => {
                            for i in 0..key_count {
                                let hw_idx = convert_original_key_index(i);
                                if 1 + hw_idx < n {
                                    let is_down = buf[1 + hw_idx] != 0;
                                    if is_down && !prev_keys[i] {
                                        callback(HardwareEvent::KeyDown { key_index: i });
                                    } else if !is_down && prev_keys[i] {
                                        callback(HardwareEvent::KeyUp { key_index: i });
                                    }
                                    prev_keys[i] = is_down;
                                }
                            }
                        }
                        DeviceKind::StreamDeckMini => {
                            for i in 0..key_count {
                                if 1 + i < n {
                                    let is_down = buf[1 + i] != 0;
                                    if is_down && !prev_keys[i] {
                                        callback(HardwareEvent::KeyDown { key_index: i });
                                    } else if !is_down && prev_keys[i] {
                                        callback(HardwareEvent::KeyUp { key_index: i });
                                    }
                                    prev_keys[i] = is_down;
                                }
                            }
                        }
                        DeviceKind::StreamDeckMK2
                        | DeviceKind::StreamDeckXL
                        | DeviceKind::StreamDeckPedal => {
                            for i in 0..key_count {
                                if 4 + i < n {
                                    let is_down = buf[4 + i] != 0;
                                    if is_down && !prev_keys[i] {
                                        callback(HardwareEvent::KeyDown { key_index: i });
                                    } else if !is_down && prev_keys[i] {
                                        callback(HardwareEvent::KeyUp { key_index: i });
                                    }
                                    prev_keys[i] = is_down;
                                }
                            }
                        }
                        DeviceKind::StreamDeckNeo => {
                            for i in 0..key_count {
                                if 4 + i < n {
                                    let is_down = buf[4 + i] != 0;
                                    if is_down && !prev_keys[i] {
                                        callback(HardwareEvent::KeyDown { key_index: i });
                                    } else if !is_down && prev_keys[i] {
                                        callback(HardwareEvent::KeyUp { key_index: i });
                                    }
                                    prev_keys[i] = is_down;
                                }
                            }
                            // 2 hardware capacitive touch points flanking Info Bar (left=prev page, right=next page)
                            for t in 0..touch_key_count {
                                let offset = 4 + key_count + t;
                                if offset < n {
                                    let is_down = buf[offset] != 0;
                                    if is_down && !prev_touch_keys[t] {
                                        callback(HardwareEvent::TouchSwipe { is_right: t == 0 });
                                    }
                                    prev_touch_keys[t] = is_down;
                                }
                            }
                        }
                        DeviceKind::StreamDeckPlus
                        | DeviceKind::StreamDeckPlusXL
                        | DeviceKind::StreamDeckStudio => {
                            if buf[1] == 0x00 {
                                // LCD Key Matrix Event
                                for i in 0..key_count {
                                    if 4 + i < n {
                                        let is_down = buf[4 + i] != 0;
                                        if is_down && !prev_keys[i] {
                                            callback(HardwareEvent::KeyDown { key_index: i });
                                        } else if !is_down && prev_keys[i] {
                                            callback(HardwareEvent::KeyUp { key_index: i });
                                        }
                                        prev_keys[i] = is_down;
                                    }
                                }
                            } else if buf[1] == 0x02 && n >= 10 && kind.has_touchscreen() {
                                // Touchscreen Event (matches StreamDeckPlus.py / StreamDeckPlusXL.py):
                                // buf[4]: 1=Short, 2=Long, 3=Drag
                                // buf[6..8]: x (LE u16), buf[8..10]: y (LE u16)
                                // buf[10..12]: x_out (LE u16), buf[12..14]: y_out (LE u16)
                                let event_type = buf[4];
                                let x = u16::from_le_bytes([buf[6], buf[7]]);
                                let y = u16::from_le_bytes([buf[8], buf[9]]);
                                if event_type == 0x03 && n >= 14 {
                                    let x_out = u16::from_le_bytes([buf[10], buf[11]]);
                                    if x_out > x + 50 {
                                        callback(HardwareEvent::TouchSwipe { is_right: true });
                                    } else if x > x_out + 50 {
                                        callback(HardwareEvent::TouchSwipe { is_right: false });
                                    }
                                } else if event_type == 0x01 || event_type == 0x02 {
                                    let max_slot = dial_count.saturating_sub(1);
                                    let slot = ((x / 200) as usize).min(max_slot);
                                    callback(HardwareEvent::TouchTap { slot, x, y });
                                }
                            } else if buf[1] == 0x03 && n >= 5 + dial_count {
                                // Dial Event: buf[4] == 0x01 (Turn), 0x00 (Push/Release)
                                if buf[4] == 0x01 {
                                    for i in 0..dial_count {
                                        let raw_val = buf[5 + i];
                                        if raw_val != 0 {
                                            let delta = decode_dial_delta(raw_val);
                                            callback(HardwareEvent::DialRotate {
                                                dial_index: i,
                                                delta,
                                            });
                                        }
                                    }
                                } else if buf[4] == 0x00 {
                                    for i in 0..dial_count {
                                        let is_down = buf[5 + i] != 0;
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
                    },
                    Ok(_) => {}
                    Err(e) => {
                        tracing::debug!(
                            "HID read error on {} (retrying): {e}",
                            dev_path.display()
                        );
                        thread::sleep(Duration::from_millis(50));
                    }
                }
            }
        });
    }
}
