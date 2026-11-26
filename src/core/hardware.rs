use crate::core::animation::{apply_hardware_transform, encode_hardware_image, GifAnimation};
use crate::core::branding::branding_frames_for;
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
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Computes horizontal marquee scroll offset (px) and cycle distance (px) for overflowing single-line text.
/// Holds briefly at the beginning (1.0s) so the start is always legible, then smoothly scrolls left
/// and loops seamlessly with a clean gap.
pub fn marquee_offset(elapsed_sec: f64, text_w: f64, clip_w: f64) -> (f64, f64) {
    const PAUSE_START_SEC: f64 = 1.0;
    const LOOP_GAP_PX: f64 = 28.0;
    let speed_px_s = 32.0 * (clip_w / 88.0).clamp(0.75, 1.35);
    let cycle_dist = text_w + LOOP_GAP_PX;
    let scroll_dur = cycle_dist / speed_px_s;
    let total_cycle = PAUSE_START_SEC + scroll_dur;
    let t = elapsed_sec.max(0.0) % total_cycle;
    let offset = if t < PAUSE_START_SEC {
        0.0
    } else {
        (t - PAUSE_START_SEC) * speed_px_s
    };
    (offset, cycle_dist)
}

/// Renders a strictly single-line button label (Top, Center, or Bottom) onto a Cairo context with
/// proportional pixel sizing (`min_dim / 86.0`), crisp dark outline, and smooth horizontal
/// marquee scrolling whenever the text spans wider than the button's inner width.
/// Returns `true` if the text overflowed and requires marquee scrolling.
pub fn draw_single_line_key_label(
    cr: &Context,
    w: f64,
    h: f64,
    text: &str,
    align: TextVerticalAlign,
    font_family: &str,
    font_size: u32,
    text_color_hex: &str,
    elapsed_sec: f64,
) -> bool {
    let clean: String = text
        .replace('\r', "")
        .split('\n')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if clean.is_empty() {
        return false;
    }

    let min_dim = w.min(h).max(24.0);
    let px_size = ((font_size.max(6) as f64) * (min_dim / 86.0)).max(8.0);

    let layout = pc::create_layout(cr);
    let mut font_desc = pango::FontDescription::new();
    font_desc.set_family(if font_family.is_empty() {
        "Cantarell"
    } else {
        font_family
    });
    font_desc.set_absolute_size(px_size * pango::SCALE as f64);
    font_desc.set_weight(pango::Weight::Bold);
    layout.set_font_description(Some(&font_desc));
    layout.set_single_paragraph_mode(true);
    layout.set_text(&clean);

    let (_, logical) = layout.pixel_extents();
    let text_w = logical.width() as f64;
    let text_h = logical.height() as f64;

    let pad_x = 4.0_f64;
    let pad_y = 4.0_f64;
    let clip_w = (w - pad_x * 2.0).max(8.0);
    let text_y = match align {
        TextVerticalAlign::Top => pad_y,
        TextVerticalAlign::Center => ((h - text_h) / 2.0).max(2.0),
        TextVerticalAlign::Bottom => (h - text_h - pad_y).max(2.0),
    };

    let (tr, tg, tb) = parse_hex_color(text_color_hex).unwrap_or((1.0, 1.0, 1.0));
    let draw_at = |x: f64| {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.90);
        for (dx, dy) in [
            (-1.0, 0.0),
            (1.0, 0.0),
            (0.0, -1.0),
            (0.0, 1.0),
            (1.0, 1.0),
        ] {
            cr.move_to(x + dx, text_y + dy);
            pc::show_layout(cr, &layout);
        }
        cr.set_source_rgb(tr, tg, tb);
        cr.move_to(x, text_y);
        pc::show_layout(cr, &layout);
    };

    let _ = cr.save();
    cr.rectangle(pad_x, (text_y - 2.0).max(0.0), clip_w, text_h + 4.0);
    cr.clip();

    let overflows = text_w > clip_w + 0.5;
    if !overflows {
        let draw_x = pad_x + (clip_w - text_w) / 2.0;
        draw_at(draw_x);
    } else {
        let (offset, cycle_dist) = marquee_offset(elapsed_sec, text_w, clip_w);
        let x1 = pad_x - offset;
        draw_at(x1);
        let x2 = x1 + cycle_dist;
        if x2 < pad_x + clip_w {
            draw_at(x2);
        }
    }

    let _ = cr.restore();
    overflows
}

/// Draws all active single-line label zones (top, center, bottom or single title) for a KeyAppearance.
/// Returns `true` if any zone overflows horizontally and requires marquee scrolling.
fn draw_key_appearance_labels(
    cr: &Context,
    w: f64,
    h: f64,
    app: &KeyAppearance,
    elapsed_sec: f64,
) -> bool {
    let mut overflows = false;
    if app.has_plugin_labels() {
        if let Some(ref t) = app.top_label {
            overflows |= draw_single_line_key_label(
                cr,
                w,
                h,
                t,
                TextVerticalAlign::Top,
                &app.font_family,
                app.font_size,
                &app.text_color,
                elapsed_sec,
            );
        }
        if let Some(ref c) = app.center_label {
            overflows |= draw_single_line_key_label(
                cr,
                w,
                h,
                c,
                TextVerticalAlign::Center,
                &app.font_family,
                app.font_size,
                &app.text_color,
                elapsed_sec,
            );
        }
        if let Some(ref b) = app.bottom_label {
            overflows |= draw_single_line_key_label(
                cr,
                w,
                h,
                b,
                TextVerticalAlign::Bottom,
                &app.font_family,
                app.font_size,
                &app.text_color,
                elapsed_sec,
            );
        }
    } else if app.show_title && !app.title.is_empty() {
        overflows |= draw_single_line_key_label(
            cr,
            w,
            h,
            &app.title,
            app.vertical_align,
            &app.font_family,
            app.font_size,
            &app.text_color,
            elapsed_sec,
        );
    }
    overflows
}

#[derive(Debug, Clone)]
pub struct ConnectedDevice {
    pub kind: DeviceKind,
    pub name: String,
    pub dev_path: PathBuf,
    pub vid: u16,
    pub pid: u16,
    pub serial: Option<String>,
}

impl ConnectedDevice {
    /// Stable identifier used for persisting per-device active profiles and brightness across reboots.
    pub fn stable_key(&self) -> String {
        self.serial
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{:?}_{:04x}", self.kind, self.pid))
    }
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
    DeviceLost { dev_path: PathBuf },
}

#[derive(Clone)]
pub struct HardwareManager {
    hid_api: Arc<Mutex<Option<HidApi>>>,
    open_handles: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<HidDevice>>>>>,
    device_kinds: Arc<Mutex<HashMap<PathBuf, DeviceKind>>>,
    listener_stops: Arc<Mutex<HashMap<PathBuf, Arc<AtomicBool>>>>,
    active_animations: Arc<Mutex<HashMap<String, (String, Arc<AtomicBool>)>>>,
    native_host: Arc<Mutex<Option<crate::native_sdk::NativePluginHost>>>,
    suspended: Arc<AtomicBool>,
    branding_locked: Arc<AtomicBool>,
    branding_painted: Arc<Mutex<HashMap<PathBuf, bool>>>,
    current_brightness: Arc<Mutex<HashMap<PathBuf, u8>>>,
    transition_gen: Arc<AtomicU64>,
}

/// Sleeps up to `duration` in short slices, waking immediately if `stop_flag` becomes true.
fn sleep_interruptible(stop_flag: &AtomicBool, duration: Duration) {
    let deadline = std::time::Instant::now() + duration;
    while !stop_flag.load(Ordering::SeqCst) {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        let step = (deadline - now).min(Duration::from_millis(10));
        thread::sleep(step);
    }
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
            listener_stops: Arc::new(Mutex::new(HashMap::new())),
            active_animations: Arc::new(Mutex::new(HashMap::new())),
            native_host: Arc::new(Mutex::new(None)),
            suspended: Arc::new(AtomicBool::new(false)),
            branding_locked: Arc::new(AtomicBool::new(false)),
            branding_painted: Arc::new(Mutex::new(HashMap::new())),
            current_brightness: Arc::new(Mutex::new(HashMap::new())),
            transition_gen: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::SeqCst)
    }

    pub fn is_branding_locked(&self) -> bool {
        self.branding_locked.load(Ordering::SeqCst)
    }

    pub fn set_branding_locked(&self, locked: bool) {
        self.branding_locked.store(locked, Ordering::SeqCst);
    }

    pub fn set_native_host(&self, host: crate::native_sdk::NativePluginHost) {
        *self.native_host.lock().unwrap() = Some(host);
    }

    fn get_native_host(&self) -> Option<crate::native_sdk::NativePluginHost> {
        self.native_host.lock().unwrap().clone()
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
        if self.is_suspended() {
            return Err("Hardware transport is suspended".to_string());
        }
        {
            let handles = self.open_handles.lock().unwrap();
            if let Some(existing) = handles.get(dev_path) {
                return Ok(existing.clone());
            }
        }

        let path_str = dev_path.to_string_lossy();
        let c_path = CString::new(path_str.as_bytes())
            .map_err(|e| format!("Invalid HID device path '{}': {e}", path_str))?;

        // Serialize concurrent opens on the same USB interface via `hid_api` and re-check
        // `open_handles` while holding it so startup threads (e.g. fade-in + input listener)
        // never race to call `api.open_path()` twice on the same device.
        let api_guard = self.hid_api.lock().unwrap();
        if self.is_suspended() {
            return Err("Hardware transport is suspended".to_string());
        }
        {
            let handles = self.open_handles.lock().unwrap();
            if let Some(existing) = handles.get(dev_path) {
                return Ok(existing.clone());
            }
        }
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

    /// Stops all active animation/marquee threads without dropping device handles.
    pub fn stop_all_animations(&self) {
        let mut anims = self.active_animations.lock().unwrap();
        for (_, (_, flag)) in anims.drain() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Stops all active device listeners without closing handles.
    pub fn stop_all_listeners(&self) {
        let mut stops = self.listener_stops.lock().unwrap();
        for (_, flag) in stops.drain() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    /// Writes the Ballista branding frames (centered 2x2 logo + black surround keys +
    /// touch strip / Neo info bar) directly to an already-open `HidDevice`.
    pub fn write_branding_direct(dev: &HidDevice, kind: DeviceKind) -> Result<(), String> {
        if !kind.is_visual() {
            return Ok(());
        }
        let frames = branding_frames_for(kind);
        let total_keys = kind.total_keys();
        for key in 0..total_keys {
            let bytes = frames.logo_keys.get(&key).unwrap_or(&frames.black_key);
            if !bytes.is_empty() {
                Self::write_key_image_direct(dev, kind, key, bytes)?;
            }
        }
        if let (Some((tw, th)), Some(ref jpeg)) = (kind.touchscreen_size(), frames.touchscreen_jpeg) {
            Self::write_touchscreen_image_direct(dev, kind, 0, 0, tw as u16, th as u16, jpeg)?;
        }
        if kind.has_info_screen() {
            if let Some(ref jpeg) = frames.neo_info_bar_jpeg {
                Self::write_neo_screen_image_direct(dev, jpeg)?;
            }
        }
        Ok(())
    }

    /// Smoothly fades hardware backlight brightness between `from` and `to` using a cosine
    /// ease-in-out curve (~60Hz). Returns `false` if cancelled by a newer `transition_gen`.
    fn fade_brightness_interruptible(
        dev_arc: &Arc<Mutex<HidDevice>>,
        dev_path: &Path,
        kind: DeviceKind,
        from: u8,
        to: u8,
        duration_ms: u64,
        steps: u32,
        cur_bright_map: &Arc<Mutex<HashMap<PathBuf, u8>>>,
        gen_guard: Option<(&Arc<AtomicU64>, u64)>,
    ) -> bool {
        if !kind.is_visual() {
            return true;
        }
        let from = from.min(100);
        let to = to.min(100);
        if from == to || steps == 0 || duration_ms == 0 {
            if let Some((gen_arc, expected)) = gen_guard {
                if gen_arc.load(Ordering::SeqCst) != expected {
                    return false;
                }
            }
            if let Ok(dev) = dev_arc.lock() {
                let _ = Self::send_brightness_direct(&dev, kind, to);
            }
            cur_bright_map
                .lock()
                .unwrap()
                .insert(dev_path.to_path_buf(), to);
            return true;
        }

        let step_ms = (duration_ms / steps as u64).max(10);
        let mut last_sent: Option<u8> = None;

        for i in 1..=steps {
            if let Some((gen_arc, expected)) = gen_guard {
                if gen_arc.load(Ordering::SeqCst) != expected {
                    return false;
                }
            }
            let t = i as f64 / steps as f64;
            let eased = 0.5 - 0.5 * (std::f64::consts::PI * t).cos();
            let val = (from as f64 + (to as f64 - from as f64) * eased)
                .round()
                .clamp(0.0, 100.0) as u8;
            if last_sent != Some(val) {
                if let Ok(dev) = dev_arc.lock() {
                    let _ = Self::send_brightness_direct(&dev, kind, val);
                }
                cur_bright_map
                    .lock()
                    .unwrap()
                    .insert(dev_path.to_path_buf(), val);
                last_sent = Some(val);
            }
            if i < steps {
                thread::sleep(Duration::from_millis(step_ms));
            }
        }
        true
    }

    /// Safely stops all listeners/animations, drains open `HidDevice` handles while waiting
    /// for worker threads to release cloned `Arc` references (so `hid_close` completes before
    /// `hid_exit`), optionally runs the Ballista branding transition (and Option A fade to 0%)
    /// in parallel across all connected decks, and tears down `HidApi`.
    fn teardown_transport_with_transition(
        &self,
        show_branding: bool,
        fade_to_zero_after_hold: bool,
        default_brightness: u8,
    ) {
        self.transition_gen.fetch_add(1, Ordering::SeqCst);
        self.stop_all_listeners();
        self.stop_all_animations();

        let drained: Vec<(PathBuf, Arc<Mutex<HidDevice>>)> = {
            let mut handles = self.open_handles.lock().unwrap();
            handles.drain().collect()
        };

        // Wait briefly for listener/animation/fade threads to exit and drop their cloned
        // Arc<Mutex<HidDevice>> handles before we run the final power transition and close USB.
        let wait_deadline = std::time::Instant::now() + Duration::from_millis(250);
        while drained.iter().any(|(_, arc)| Arc::strong_count(arc) > 1)
            && std::time::Instant::now() < wait_deadline
        {
            thread::sleep(Duration::from_millis(5));
        }

        if show_branding && !drained.is_empty() {
            let kinds = self.device_kinds.lock().unwrap().clone();
            let painted_snap = self.branding_painted.lock().unwrap().clone();
            let bright_snap = self.current_brightness.lock().unwrap().clone();
            let bright_map = self.current_brightness.clone();

            thread::scope(|s| {
                for (path, dev_arc) in &drained {
                    let kind = kinds
                        .get(path)
                        .copied()
                        .unwrap_or(DeviceKind::StreamDeckMK2);
                    if !kind.is_visual() {
                        continue;
                    }
                    let already_branded = painted_snap.get(path).copied().unwrap_or(false);
                    let cur_b = bright_snap.get(path).copied().unwrap_or(default_brightness);
                    let bm = bright_map.clone();
                    let p = path.clone();

                    s.spawn(move || {
                        if already_branded {
                            // Branding is already on screen (e.g. suspending/shutting down from Lock Screen)
                            if fade_to_zero_after_hold && cur_b > 0 {
                                Self::fade_brightness_interruptible(
                                    dev_arc, &p, kind, cur_b, 0, 320, 18, &bm, None,
                                );
                            } else if fade_to_zero_after_hold {
                                if let Ok(dev) = dev_arc.lock() {
                                    let _ = Self::send_brightness_direct(&dev, kind, 0);
                                }
                            }
                            return;
                        }

                        if cur_b > 0 {
                            // 1. Fade Action UI out cleanly to 0%
                            Self::fade_brightness_interruptible(
                                dev_arc, &p, kind, cur_b, 0, 240, 14, &bm, None,
                            );
                        } else if let Ok(dev) = dev_arc.lock() {
                            let _ = Self::send_brightness_direct(&dev, kind, 0);
                        }

                        // 2. Paint Ballista 2x2 logo + touch strip while dark (zero tearing)
                        if let Ok(dev) = dev_arc.lock() {
                            if let Err(e) = Self::write_branding_direct(&dev, kind) {
                                tracing::warn!(
                                    "Failed to write Ballista branding to {} ({}): {e}",
                                    kind.name(),
                                    p.display()
                                );
                            }
                        }

                        if cur_b == 0 {
                            // Manually slept deck: branding is now safely in framebuffer, stay dark
                            return;
                        }

                        // 3. Fade Ballista branding smoothly in
                        let target_b = cur_b.max(35);
                        Self::fade_brightness_interruptible(
                            dev_arc, &p, kind, 0, target_b, 260, 15, &bm, None,
                        );

                        if fade_to_zero_after_hold {
                            // 4. Option A: Hold Ballista branding briefly, then fade cleanly to 0% black
                            thread::sleep(Duration::from_millis(550));
                            Self::fade_brightness_interruptible(
                                dev_arc, &p, kind, target_b, 0, 320, 18, &bm, None,
                            );
                        }
                    });
                }
            });
        }

        // Explicitly close all HidDevice handles BEFORE dropping HidApi (hid_exit / libusb_exit).
        drop(drained);
        self.device_kinds.lock().unwrap().clear();
        self.branding_painted.lock().unwrap().clear();
        self.current_brightness.lock().unwrap().clear();
        *self.hid_api.lock().unwrap() = None;
    }

    /// Prepares hardware for system suspend (Option A): fades Action UI into Ballista branding,
    /// holds briefly, fades backlight to 0%, stops listeners & animations, and releases USB HID.
    pub fn prepare_for_suspend(&self) {
        tracing::info!("Preparing Elgato Stream Deck hardware for system suspend (branding fade -> 0%)...");
        self.suspended.store(true, Ordering::SeqCst);
        self.branding_locked.store(true, Ordering::SeqCst);
        self.teardown_transport_with_transition(true, true, 75);
        tracing::info!("Stream Deck suspend branding transition complete; USB HID transport released");
    }

    /// Prepares hardware for shutdown/reboot (Option A: `fade_to_zero_after_hold = true`) or
    /// logout/restart (`fade_to_zero_after_hold = false`, leaving Ballista branding lit for GDM).
    pub fn prepare_for_shutdown(&self, fade_to_zero_after_hold: bool) {
        tracing::info!(
            "Running Stream Deck exit branding transition (fade_to_zero={fade_to_zero_after_hold})..."
        );
        self.suspended.store(true, Ordering::SeqCst);
        self.branding_locked.store(true, Ordering::SeqCst);
        self.teardown_transport_with_transition(true, fade_to_zero_after_hold, 75);
        tracing::info!("Stream Deck exit branding transition complete; USB HID transport released");
    }

    /// Full USB HID transport reset (used on system wake, Tray Rescan, and I/O error recovery):
    /// tears down stale handles/animations and initializes a fresh `HidApi` (libusb) context.
    pub fn reset_transport(&self) {
        self.suspended.store(true, Ordering::SeqCst);
        self.teardown_transport_with_transition(false, false, 0);
        let fresh_api = match HidApi::new() {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::error!("Failed to re-initialize libusb HIDAPI transport: {e}");
                None
            }
        };
        *self.hid_api.lock().unwrap() = fresh_api;
        self.suspended.store(false, Ordering::SeqCst);
    }

    /// Stops all background listeners/animations and releases USB device handles cleanly.
    pub fn shutdown(&self) {
        self.suspended.store(true, Ordering::SeqCst);
        self.teardown_transport_with_transition(false, false, 0);
    }

    /// Transitions all connected visual decks in parallel from their current display (e.g.
    /// boot branding or lock screen branding) into the live Action UI with a smooth fade-out ->
    /// frame swap -> fade-in crossfade.
    pub fn fade_in_all_devices_async(
        &self,
        targets: Vec<(PathBuf, String, DeviceKind, Profile, u8)>,
        assumed_start_brightness: Option<u8>,
    ) {
        self.branding_locked.store(false, Ordering::SeqCst);
        let my_gen = self.transition_gen.fetch_add(1, Ordering::SeqCst) + 1;

        for (dev_path, device_key, kind, profile, brightness) in targets {
            let hw = self.clone();
            thread::spawn(move || {
                if hw.is_suspended() || hw.is_branding_locked() {
                    return;
                }
                if !kind.is_visual() {
                    return;
                }
                let t0 = std::time::Instant::now();
                let dev_arc = match hw.get_or_open_device(&dev_path) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("Cannot open {} for fade-in sync: {e}", dev_path.display());
                        return;
                    }
                };

                let start_b = hw
                    .current_brightness
                    .lock()
                    .unwrap()
                    .get(&dev_path)
                    .copied()
                    .or(assumed_start_brightness)
                    .unwrap_or(0);

                // 1. Fade out existing branding (if lit) to 0% so frame swap is completely tear-free
                if start_b > 0 {
                    if !Self::fade_brightness_interruptible(
                        &dev_arc,
                        &dev_path,
                        kind,
                        start_b,
                        0,
                        220,
                        13,
                        &hw.current_brightness,
                        Some((&hw.transition_gen, my_gen)),
                    ) {
                        return;
                    }
                } else {
                    if let Ok(dev) = dev_arc.lock() {
                        let _ = Self::send_brightness_direct(&dev, kind, 0);
                    }
                    hw.current_brightness
                        .lock()
                        .unwrap()
                        .insert(dev_path.clone(), 0);
                }

                if hw.transition_gen.load(Ordering::SeqCst) != my_gen
                    || hw.is_suspended()
                    || hw.is_branding_locked()
                {
                    return;
                }

                // 2. Render and upload the complete Action UI while dark at 0%
                hw.sync_hardware_page(&dev_path, &device_key, kind, &profile);

                if hw.transition_gen.load(Ordering::SeqCst) != my_gen
                    || hw.is_suspended()
                    || hw.is_branding_locked()
                {
                    return;
                }

                // 3. Smoothly fade Action UI up to the deck's saved brightness
                Self::fade_brightness_interruptible(
                    &dev_arc,
                    &dev_path,
                    kind,
                    0,
                    brightness,
                    320,
                    18,
                    &hw.current_brightness,
                    Some((&hw.transition_gen, my_gen)),
                );

                tracing::info!(
                    "Fade-in hardware sync complete for {} ({}) in {:?}",
                    kind.name(),
                    dev_path.display(),
                    t0.elapsed()
                );
            });
        }
    }

    /// Transitions all connected visual decks in parallel to the Ballista branding screen
    /// (used when locking the screen or waking from suspend onto the lock screen).
    pub fn fade_to_branding_all_async(
        &self,
        targets: Vec<(PathBuf, DeviceKind, u8)>,
        fade_from_zero: bool,
    ) {
        self.branding_locked.store(true, Ordering::SeqCst);
        let my_gen = self.transition_gen.fetch_add(1, Ordering::SeqCst) + 1;
        self.stop_all_animations();

        for (dev_path, kind, target_brightness) in targets {
            let hw = self.clone();
            thread::spawn(move || {
                if hw.is_suspended() || !kind.is_visual() {
                    return;
                }
                let dev_arc = match hw.get_or_open_device(&dev_path) {
                    Ok(d) => d,
                    Err(e) => {
                        tracing::warn!("Cannot open {} for lock branding: {e}", dev_path.display());
                        return;
                    }
                };

                let start_b = if fade_from_zero {
                    0
                } else {
                    hw.current_brightness
                        .lock()
                        .unwrap()
                        .get(&dev_path)
                        .copied()
                        .unwrap_or(target_brightness)
                };

                // 1. Fade Action UI down to 0%
                if start_b > 0 {
                    if !Self::fade_brightness_interruptible(
                        &dev_arc,
                        &dev_path,
                        kind,
                        start_b,
                        0,
                        240,
                        14,
                        &hw.current_brightness,
                        Some((&hw.transition_gen, my_gen)),
                    ) {
                        return;
                    }
                } else {
                    if let Ok(dev) = dev_arc.lock() {
                        let _ = Self::send_brightness_direct(&dev, kind, 0);
                    }
                    hw.current_brightness
                        .lock()
                        .unwrap()
                        .insert(dev_path.clone(), 0);
                }

                if hw.transition_gen.load(Ordering::SeqCst) != my_gen || hw.is_suspended() {
                    return;
                }

                // 2. Paint Ballista 2x2 logo + touch strip while dark
                let write_ok = if let Ok(dev) = dev_arc.lock() {
                    match Self::write_branding_direct(&dev, kind) {
                        Ok(()) => true,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to render Ballista lock branding on {}: {e}",
                                dev_path.display()
                            );
                            false
                        }
                    }
                } else {
                    false
                };
                if write_ok {
                    hw.branding_painted
                        .lock()
                        .unwrap()
                        .insert(dev_path.clone(), true);
                }

                if hw.transition_gen.load(Ordering::SeqCst) != my_gen || hw.is_suspended() {
                    return;
                }

                // 3. Fade Ballista branding smoothly up to target_brightness
                if target_brightness > 0 {
                    Self::fade_brightness_interruptible(
                        &dev_arc,
                        &dev_path,
                        kind,
                        0,
                        target_brightness,
                        300,
                        16,
                        &hw.current_brightness,
                        Some((&hw.transition_gen, my_gen)),
                    );
                }
            });
        }
    }

    /// Standalone headless boot-splash runner (requires no GTK/Wayland/D-Bus).
    /// Retries USB scan briefly during early kernel boot, paints the Ballista 2x2 logo +
    /// touch strip onto every connected Stream Deck in parallel, smoothly fades up to
    /// `target_brightness`, and optionally holds and fades back to 0% (for testing Option A).
    pub fn run_boot_splash(
        target_brightness: u8,
        animate_fade_in: bool,
        hold_then_fade_off: bool,
    ) -> usize {
        let hw = Self::new();
        let mut devices = hw.scan_devices();
        for attempt in 1..=3 {
            if !devices.is_empty() {
                break;
            }
            tracing::info!(
                "No Elgato Stream Decks enumerated yet (boot retry {attempt}/3)..."
            );
            thread::sleep(Duration::from_millis(450));
            devices = hw.scan_devices();
        }
        if devices.is_empty() {
            tracing::info!("Boot splash: no Elgato Stream Deck devices found");
            return 0;
        }

        let total_detected = devices.len();
        let painted = AtomicUsize::new(0);
        thread::scope(|s| {
            for dev_info in &devices {
                if !dev_info.kind.is_visual() {
                    continue;
                }
                let hw_ref = &hw;
                let painted_ref = &painted;
                s.spawn(move || {
                    let dev_arc = match hw_ref.get_or_open_device(&dev_info.dev_path) {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(
                                "Boot splash could not open {} ({}): {e}",
                                dev_info.name,
                                dev_info.dev_path.display()
                            );
                            return;
                        }
                    };
                    if let Ok(dev) = dev_arc.lock() {
                        let _ = Self::send_brightness_direct(&dev, dev_info.kind, 0);
                        if let Err(e) = Self::write_branding_direct(&dev, dev_info.kind) {
                            tracing::warn!(
                                "Boot splash write failed on {} ({}): {e}",
                                dev_info.name,
                                dev_info.dev_path.display()
                            );
                            return;
                        }
                    }
                    if target_brightness > 0 {
                        if animate_fade_in {
                            Self::fade_brightness_interruptible(
                                &dev_arc,
                                &dev_info.dev_path,
                                dev_info.kind,
                                0,
                                target_brightness,
                                320,
                                18,
                                &hw_ref.current_brightness,
                                None,
                            );
                        } else if let Ok(dev) = dev_arc.lock() {
                            let _ = Self::send_brightness_direct(
                                &dev,
                                dev_info.kind,
                                target_brightness,
                            );
                        }
                    }
                    if hold_then_fade_off && target_brightness > 0 {
                        thread::sleep(Duration::from_millis(600));
                        Self::fade_brightness_interruptible(
                            &dev_arc,
                            &dev_info.dev_path,
                            dev_info.kind,
                            target_brightness,
                            0,
                            320,
                            18,
                            &hw_ref.current_brightness,
                            None,
                        );
                    }
                    painted_ref.fetch_add(1, Ordering::SeqCst);
                });
            }
        });

        hw.shutdown();
        let painted_count = painted.load(Ordering::SeqCst);
        tracing::info!(
            "Ballista boot splash completed for {painted_count}/{total_detected} device(s)"
        );
        painted_count
    }

    /// Phase-1 Fast Boot: renders and pushes initial saved brightness & active pages to ALL
    /// connected decks simultaneously on parallel threads (<250ms total even with multiple decks).
    pub fn sync_all_devices_async(&self, targets: Vec<(PathBuf, String, DeviceKind, Profile, u8)>) {
        self.fade_in_all_devices_async(targets, Some(75));
    }

    fn send_brightness_direct(dev: &HidDevice, kind: DeviceKind, brightness: u8) -> Result<(), String> {
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

    /// Send hardware brightness feature report (0-100%) using the exact Gen1 or Gen2 protocol
    pub fn set_brightness(&self, dev_path: &Path, brightness: u8) -> Result<(), String> {
        if self.is_branding_locked() {
            return Ok(());
        }
        let kind = self.lookup_kind(dev_path);
        if !kind.is_visual() {
            return Ok(());
        }

        let dev_arc = self.get_or_open_device(dev_path)?;
        let dev = dev_arc.lock().unwrap();
        let res = Self::send_brightness_direct(&dev, kind, brightness);
        if res.is_ok() {
            self.current_brightness
                .lock()
                .unwrap()
                .insert(dev_path.to_path_buf(), brightness.min(100));
        }
        res
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

    /// Pre-renders the static key background + icon (or native Cairo vector visual) layer
    /// so marquee frames never reload icon assets from disk on each scroll tick.
    fn render_key_base_layer(
        device_key: &str,
        kind: DeviceKind,
        key_idx: usize,
        appearance: Option<&KeyAppearance>,
        icon_name: Option<&str>,
        is_active: bool,
        native_host: Option<&crate::native_sdk::NativePluginHost>,
    ) -> Option<(u32, u32, i32, Vec<u8>)> {
        let (w, h) = kind.key_image_size()?;
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

            let custom_icon_override = appearance
                .and_then(|a| a.icon_path.as_deref())
                .filter(|p| !p.is_empty() && Path::new(p).exists())
                .is_some();

            let painted_native = if !custom_icon_override {
                if let Some(host) = native_host {
                    host.render_slot_visual(device_key, &format!("key_{key_idx}"), &cr, w as f64, h as f64)
                } else {
                    false
                }
            } else {
                false
            };

            if !painted_native {
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
            }
        }

        drop(cr);
        let stride = surface.stride();
        let data = surface.take_data().ok()?.to_vec();
        Some((w, h, stride, data))
    }

    /// Renders single-line (and optionally scrolling marquee) labels onto a pre-rendered base buffer
    /// and encodes the final frame for the target Stream Deck hardware.
    fn composite_key_frame(
        kind: DeviceKind,
        w: u32,
        h: u32,
        stride: i32,
        base_data: &[u8],
        appearance: Option<&KeyAppearance>,
        is_active: bool,
        elapsed_sec: f64,
    ) -> (Vec<u8>, bool) {
        let mut frame_buf = base_data.to_vec();
        let mut overflows = false;

        if is_active {
            if let Some(app) = appearance {
                match ImageSurface::create_for_data(
                    frame_buf,
                    Format::Rgb24,
                    w as i32,
                    h as i32,
                    stride,
                ) {
                    Ok(surface) => {
                        if let Ok(cr) = Context::new(&surface) {
                            overflows =
                                draw_key_appearance_labels(&cr, w as f64, h as f64, app, elapsed_sec);
                        }
                        frame_buf = surface
                            .take_data()
                            .map(|d| d.to_vec())
                            .unwrap_or_else(|_| base_data.to_vec());
                    }
                    Err(_) => {
                        frame_buf = base_data.to_vec();
                    }
                }
            }
        }

        let stride_u = stride as usize;
        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let offset = y as usize * stride_u + x as usize * 4;
                let b_val = frame_buf[offset];
                let g_val = frame_buf[offset + 1];
                let r_val = frame_buf[offset + 2];
                img.put_pixel(x, y, Rgb([r_val, g_val, b_val]));
            }
        }

        let (rot_ccw, flip_x, flip_y) = kind.key_image_transform();
        let transformed = apply_hardware_transform(img, rot_ccw, flip_x, flip_y);
        (
            encode_hardware_image(&transformed, kind.key_image_format(), 95),
            overflows,
        )
    }

    /// Renders a key image buffer formatted natively (dimensions, rotation, flip, BMP or JPEG) for the hardware
    pub fn render_key_image(
        &self,
        device_key: &str,
        kind: DeviceKind,
        key_idx: usize,
        appearance: Option<&KeyAppearance>,
        icon_name: Option<&str>,
        is_active: bool,
    ) -> Vec<u8> {
        let host = self.get_native_host();
        let Some((w, h, stride, base_data)) = Self::render_key_base_layer(
            device_key,
            kind,
            key_idx,
            appearance,
            icon_name,
            is_active,
            host.as_ref(),
        ) else {
            return Vec::new();
        };
        let (encoded, _) = Self::composite_key_frame(
            kind,
            w,
            h,
            stride,
            &base_data,
            appearance,
            is_active,
            0.0,
        );
        encoded
    }

    /// Renders the touchscreen strip JPEG for Stream Deck + (800x100) or Stream Deck + XL (1200x100 -> rotated 90° CCW)
    pub fn render_touchscreen_jpeg(&self, device_key: &str, kind: DeviceKind, profile: &Profile) -> Vec<u8> {
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
                let native_host = self.get_native_host();
                for i in 0..dial_count {
                    if let Some(action) = page.dial_actions.get(&i) {
                        let slot_x = (i as f64) * slot_w;
                        let app = action.active_appearance();
                        let summary_title = app.combined_display_summary();
                        let title = summary_title.as_str();

                        let has_custom_path = app
                            .icon_path
                            .as_deref()
                            .filter(|p| !p.is_empty() && Path::new(p).exists())
                            .is_some();

                        if let Some(ref bg_hex) = app.bg_color {
                            if let Some((bg_r, bg_g, bg_b)) = parse_hex_color(bg_hex) {
                                cr.set_source_rgb(bg_r, bg_g, bg_b);
                                cr.rectangle(slot_x, 0.0, slot_w, height as f64);
                                let _ = cr.fill();
                            }
                        }

                        if !has_custom_path {
                            if let Some(ref host) = native_host {
                                let _ = cr.save();
                                cr.rectangle(slot_x, 0.0, slot_w, height as f64);
                                cr.clip();
                                cr.translate(slot_x, 0.0);
                                let painted = host.render_slot_visual(
                                    device_key,
                                    &format!("dial_{i}"),
                                    &cr,
                                    slot_w,
                                    height as f64,
                                );
                                let _ = cr.restore();
                                if painted {
                                    continue;
                                }
                            }
                        }

                        let mut has_card_icon = false;
                        if let Some(ref path) = app.icon_path {
                            if Path::new(path).exists() {
                                let scale_pct = (app.icon_scale.clamp(20, 100) as f64) / 100.0;
                                let is_svg = path.to_lowercase().ends_with(".svg");

                                let loaded_pb: Option<(gdk_pixbuf::Pixbuf, bool)> = if is_svg {
                                    // Probe natural aspect via unscaled load, then rasterize cleanly at target size
                                    let aspect = gdk_pixbuf::Pixbuf::from_file(path)
                                        .map(|p| p.width().max(1) as f64 / p.height().max(1) as f64)
                                        .unwrap_or(1.0);
                                    let is_wide = aspect > 1.35;
                                    let (tw, th) = if is_wide {
                                        (
                                            (slot_w * scale_pct).round().max(16.0) as i32,
                                            (height as f64 * scale_pct).round().max(16.0) as i32,
                                        )
                                    } else {
                                        let base = if title.is_empty() { height as f64 - 12.0 } else { height as f64 - 36.0 };
                                        let sq = (base * scale_pct).round().max(16.0) as i32;
                                        (sq, sq)
                                    };
                                    gdk_pixbuf::Pixbuf::from_file_at_scale(path, tw, th, true)
                                        .ok()
                                        .map(|pb| (pb, is_wide))
                                } else if let Ok(raw_pb) = gdk_pixbuf::Pixbuf::from_file(path) {
                                    let src_w = raw_pb.width().max(1) as f64;
                                    let src_h = raw_pb.height().max(1) as f64;
                                    let aspect = src_w / src_h;
                                    let is_wide = aspect > 1.35;

                                    // Wide dial cards (200x100 on Stream Deck + & Stream Deck + XL) fill
                                    // their full slot_w x height LCD zone; square icons stay cleanly proportioned.
                                    let (max_w, max_h) = if is_wide {
                                        (
                                            (slot_w * scale_pct).max(16.0),
                                            (height as f64 * scale_pct).max(16.0),
                                        )
                                    } else {
                                        let base = if title.is_empty() { height as f64 - 12.0 } else { height as f64 - 36.0 };
                                        let sq = (base * scale_pct).max(16.0);
                                        (sq, sq)
                                    };

                                    let fit_scale = (max_w / src_w).min(max_h / src_h);
                                    let dst_w = (src_w * fit_scale).round().max(1.0) as i32;
                                    let dst_h = (src_h * fit_scale).round().max(1.0) as i32;

                                    let scaled = if dst_w == raw_pb.width() && dst_h == raw_pb.height() {
                                        Some(raw_pb)
                                    } else {
                                        raw_pb.scale_simple(dst_w, dst_h, gdk_pixbuf::InterpType::Hyper)
                                    };
                                    scaled.map(|pb| (pb, is_wide))
                                } else {
                                    None
                                };

                                if let Some((pixbuf, is_wide_card)) = loaded_pb {
                                    has_card_icon = true;
                                    let pw = pixbuf.width() as f64;
                                    let ph = pixbuf.height() as f64;
                                    let px = slot_x + (slot_w - pw) / 2.0;
                                    let py = if is_wide_card || title.is_empty() {
                                        (height as f64 - ph) / 2.0
                                    } else {
                                        8.0
                                    };
                                    cr.set_source_pixbuf(&pixbuf, px, py);
                                    let _ = cr.paint();
                                }
                            }
                        }

                        let draw_dial_label = |text: &str, y_pos: f64| {
                            let clean: String = text
                                .replace('\r', "")
                                .split('\n')
                                .map(|s| s.trim())
                                .filter(|s| !s.is_empty())
                                .collect::<Vec<_>>()
                                .join(" ");
                            if clean.is_empty() {
                                return;
                            }
                            let layout = pc::create_layout(&cr);
                            let mut font_desc = pango::FontDescription::new();
                            font_desc.set_family(&app.font_family);
                            font_desc.set_absolute_size((app.font_size.max(8) as f64) * pango::SCALE as f64);
                            font_desc.set_weight(pango::Weight::Bold);
                            layout.set_font_description(Some(&font_desc));
                            layout.set_single_paragraph_mode(true);
                            layout.set_ellipsize(pango::EllipsizeMode::End);
                            layout.set_text(&clean);
                            layout.set_alignment(pango::Alignment::Center);
                            layout.set_width(((slot_w - 8.0).max(16.0) as i32) * pango::SCALE);

                            cr.set_source_rgba(0.0, 0.0, 0.0, 0.85);
                            cr.move_to(slot_x + 4.0 + 1.0, y_pos + 1.0);
                            pc::show_layout(&cr, &layout);

                            let (fg_r, fg_g, fg_b) =
                                parse_hex_color(&app.text_color).unwrap_or((1.0, 1.0, 1.0));
                            cr.set_source_rgb(fg_r, fg_g, fg_b);
                            cr.move_to(slot_x + 4.0, y_pos);
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
    pub fn sync_hardware_page(&self, dev_path: &Path, device_key: &str, kind: DeviceKind, profile: &Profile) {
        if !kind.is_visual() || self.is_suspended() || self.is_branding_locked() {
            return;
        }
        self.branding_painted
            .lock()
            .unwrap()
            .insert(dev_path.to_path_buf(), false);

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
                                    sleep_interruptible(&stop_flag, Duration::from_millis(delay_ms as u64));
                                }
                            }
                        });
                        continue;
                    } else {
                        continue;
                    }
                }

                let native_host = self.get_native_host();
                let Some((kw, kh, kstride, base_data)) = Self::render_key_base_layer(
                    device_key,
                    kind,
                    key,
                    appearance,
                    icon_name,
                    is_active,
                    native_host.as_ref(),
                ) else {
                    if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                        stop_flag.store(true, Ordering::SeqCst);
                    }
                    drop(anim_lock);
                    continue;
                };

                let (encoded_0, overflows) = Self::composite_key_frame(
                    kind,
                    kw,
                    kh,
                    kstride,
                    &base_data,
                    appearance,
                    is_active,
                    0.0,
                );

                if overflows {
                    if let Some(app) = appearance {
                        let marquee_sig = format!(
                            "marquee:{:?}:{:?}:{:?}:{}:{}:{}:{}:{}:{:?}:{:?}:{}",
                            app.top_label,
                            app.center_label,
                            app.bottom_label,
                            app.show_title,
                            app.title,
                            app.font_family,
                            app.font_size,
                            app.text_color,
                            app.bg_color,
                            app.icon_path,
                            app.icon_scale
                        );
                        let already_running = anim_lock
                            .get(&key_anim_id)
                            .map(|(sig, _)| sig == &marquee_sig)
                            .unwrap_or(false);
                        if already_running {
                            drop(anim_lock);
                            continue;
                        }
                        if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                            stop_flag.store(true, Ordering::SeqCst);
                        }
                        let stop_flag = Arc::new(AtomicBool::new(false));
                        anim_lock.insert(key_anim_id.clone(), (marquee_sig, stop_flag.clone()));
                        drop(anim_lock);

                        // Immediately push initial static frame (t=0.0) so key renders with zero delay
                        {
                            let dev = dev_arc.lock().unwrap();
                            let _ = Self::write_key_image_direct(&dev, kind, key, &encoded_0);
                        }

                        // Lightweight marquee ticker (~12 FPS) reusing the pre-rendered icon/bg base buffer
                        let dev_arc_anim = dev_arc.clone();
                        let app_owned = app.clone();
                        thread::spawn(move || {
                            let started = std::time::Instant::now();
                            while !stop_flag.load(Ordering::SeqCst) {
                                sleep_interruptible(&stop_flag, Duration::from_millis(85));
                                if stop_flag.load(Ordering::SeqCst) {
                                    break;
                                }
                                let elapsed = started.elapsed().as_secs_f64();
                                let (frame_bytes, _) = Self::composite_key_frame(
                                    kind,
                                    kw,
                                    kh,
                                    kstride,
                                    &base_data,
                                    Some(&app_owned),
                                    true,
                                    elapsed,
                                );
                                let dev = dev_arc_anim.lock().unwrap();
                                let _ = Self::write_key_image_direct(&dev, kind, key, &frame_bytes);
                            }
                        });
                        continue;
                    }
                }

                if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                drop(anim_lock);

                let res = {
                    let dev = dev_arc.lock().unwrap();
                    Self::write_key_image_direct(&dev, kind, key, &encoded_0)
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
                                sleep_interruptible(&stop_flag, Duration::from_millis(delay_ms as u64));
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

            let touch_jpeg = self.render_touchscreen_jpeg(device_key, kind, profile);
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

    /// Lightweight single-slot hardware push for live 1Hz native action ticks (Clock, System Metrics, Media)
    /// so ticking a single key never re-renders or re-uploads all 32 keys over USB.
    pub fn sync_single_slot(&self, dev_path: &Path, device_key: &str, kind: DeviceKind, profile: &Profile, slot: &str) {
        if !kind.is_visual() || self.is_suspended() || self.is_branding_locked() {
            return;
        }
        let Ok(dev_arc) = self.get_or_open_device(dev_path) else {
            return;
        };

        if let Some(key_str) = slot.strip_prefix("key_") {
            if let Ok(key) = key_str.parse::<usize>() {
                if key >= kind.total_keys() {
                    return;
                }
                let page = match profile.current_page() {
                    Some(p) => p,
                    None => return,
                };
                let action = page.keys.get(&key);
                let appearance = action.map(|a| a.active_appearance());
                let icon_name = action
                    .and_then(|a| a.parameters.get("icon_name"))
                    .map(|s| s.as_str());
                let is_active = action.is_some();
                let native_host = self.get_native_host();

                let Some((kw, kh, kstride, base_data)) = Self::render_key_base_layer(
                    device_key,
                    kind,
                    key,
                    appearance,
                    icon_name,
                    is_active,
                    native_host.as_ref(),
                ) else {
                    return;
                };
                let (encoded_0, overflows) = Self::composite_key_frame(
                    kind,
                    kw,
                    kh,
                    kstride,
                    &base_data,
                    appearance,
                    is_active,
                    0.0,
                );

                let key_anim_id = format!("key_{}_{}", dev_path.display(), key);
                let mut anim_lock = self.active_animations.lock().unwrap();
                if overflows {
                    if let Some(app) = appearance {
                        let marquee_sig = format!(
                            "marquee:{:?}:{:?}:{:?}:{}:{}:{}:{}:{}:{:?}:{:?}:{}",
                            app.top_label,
                            app.center_label,
                            app.bottom_label,
                            app.show_title,
                            app.title,
                            app.font_family,
                            app.font_size,
                            app.text_color,
                            app.bg_color,
                            app.icon_path,
                            app.icon_scale
                        );
                        let already_running = anim_lock
                            .get(&key_anim_id)
                            .map(|(sig, _)| sig == &marquee_sig)
                            .unwrap_or(false);
                        if already_running {
                            return;
                        }
                        if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                            stop_flag.store(true, Ordering::SeqCst);
                        }
                        let stop_flag = Arc::new(AtomicBool::new(false));
                        anim_lock.insert(key_anim_id.clone(), (marquee_sig, stop_flag.clone()));
                        drop(anim_lock);

                        {
                            let dev = dev_arc.lock().unwrap();
                            let _ = Self::write_key_image_direct(&dev, kind, key, &encoded_0);
                        }
                        let dev_arc_anim = dev_arc.clone();
                        let app_owned = app.clone();
                        thread::spawn(move || {
                            let started = std::time::Instant::now();
                            while !stop_flag.load(Ordering::SeqCst) {
                                sleep_interruptible(&stop_flag, Duration::from_millis(85));
                                if stop_flag.load(Ordering::SeqCst) {
                                    break;
                                }
                                let elapsed = started.elapsed().as_secs_f64();
                                let (frame_bytes, _) = Self::composite_key_frame(
                                    kind,
                                    kw,
                                    kh,
                                    kstride,
                                    &base_data,
                                    Some(&app_owned),
                                    true,
                                    elapsed,
                                );
                                let dev = dev_arc_anim.lock().unwrap();
                                let _ = Self::write_key_image_direct(&dev, kind, key, &frame_bytes);
                            }
                        });
                        return;
                    }
                }
                if let Some((_, stop_flag)) = anim_lock.remove(&key_anim_id) {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                drop(anim_lock);

                let dev = dev_arc.lock().unwrap();
                if let Err(e) = Self::write_key_image_direct(&dev, kind, key, &encoded_0) {
                    tracing::warn!(
                        "Failed to update key {} on {}: {}",
                        key,
                        dev_path.display(),
                        e
                    );
                }
            }
        } else if slot.starts_with("dial_") {
            if let Some((tw, th)) = kind.touchscreen_size() {
                let touch_jpeg = self.render_touchscreen_jpeg(device_key, kind, profile);
                let dev = dev_arc.lock().unwrap();
                if let Err(e) = Self::write_touchscreen_image_direct(
                    &dev,
                    kind,
                    0,
                    0,
                    tw as u16,
                    th as u16,
                    &touch_jpeg,
                ) {
                    tracing::warn!(
                        "Failed to update touchscreen on {}: {}",
                        dev_path.display(),
                        e
                    );
                }
            }
        }
    }

    /// Spawn background listener thread polling USB HID input reports for a connected Stream Deck.
    /// Supports simultaneous listeners across ALL connected decks (only replaces an existing listener
    /// for the same `dev_path`).
    pub fn start_listener<F>(&self, dev_path: PathBuf, kind: DeviceKind, callback: F)
    where
        F: Fn(HardwareEvent) + Send + 'static,
    {
        // Stop any previous listener for THIS specific device only (other decks remain active!)
        if let Some(old_stop) = self.listener_stops.lock().unwrap().remove(&dev_path) {
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
        let suspended_flag = self.suspended.clone();
        self.listener_stops
            .lock()
            .unwrap()
            .insert(dev_path.clone(), stop_flag.clone());

        thread::spawn(move || {
            let key_count = kind.total_keys();
            let dial_count = kind.dial_count();
            let touch_key_count = kind.touch_key_count();

            let mut prev_keys = vec![false; key_count];
            let mut prev_touch_keys = vec![false; touch_key_count];
            let mut prev_dial_press = vec![false; dial_count];
            let mut buf = [0u8; 128];
            let mut consecutive_errors: u32 = 0;

            tracing::info!(
                "Started USB HID listener for {} ({}) [keys={}, dials={}]",
                kind.name(),
                dev_path.display(),
                key_count,
                dial_count
            );

            while !stop_flag.load(Ordering::SeqCst) && !suspended_flag.load(Ordering::SeqCst) {
                let read_res = {
                    let dev = dev_arc.lock().unwrap();
                    dev.read(&mut buf)
                };

                match read_res {
                    Ok(0) => {
                        consecutive_errors = 0;
                        // Sleep OUTSIDE the device mutex so writes & brightness updates acquire immediately
                        sleep_interruptible(&stop_flag, Duration::from_millis(5));
                    }
                    Ok(n) if n >= 2 && buf[0] == 0x01 => {
                        consecutive_errors = 0;
                        match kind {
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
                    }
                    }
                    Ok(_) => {
                        consecutive_errors = 0;
                    }
                    Err(e) => {
                        if stop_flag.load(Ordering::SeqCst) || suspended_flag.load(Ordering::SeqCst) {
                            break;
                        }
                        consecutive_errors += 1;
                        if consecutive_errors == 1 || consecutive_errors % 5 == 0 {
                            tracing::warn!(
                                "HID read error #{} on {}: {e}",
                                consecutive_errors,
                                dev_path.display()
                            );
                        }
                        if consecutive_errors >= 10 {
                            tracing::warn!(
                                "USB HID transport lost on {} after {} consecutive errors — requesting automatic recovery",
                                dev_path.display(),
                                consecutive_errors
                            );
                            callback(HardwareEvent::DeviceLost {
                                dev_path: dev_path.clone(),
                            });
                            break;
                        }
                        sleep_interruptible(&stop_flag, Duration::from_millis(50));
                    }
                }
            }
        });
    }
}
