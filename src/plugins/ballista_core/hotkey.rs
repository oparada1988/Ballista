use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand, SlotSurface,
};
use crate::plugins::ballista_core::system_metrics::{draw_cairo_text, draw_pill, parse_hex};
use serde_json::json;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::process::Command;
use std::time::{Duration, Instant};

const HOTKEY_MODES: &[&str] = &["Hotkey", "Hotkey Toggle", "Type Text"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JogFlash {
    None,
    Ccw,
    Press,
    Cw,
}

pub struct HotkeyAction {
    surface: SlotSurface,
    mode_idx: usize,
    combo_a: String,
    combo_b: String,
    text_snippet: String,
    press_enter: bool,
    toggle_state: usize,
    // Dial Jog Shortcut bindings
    dial_ccw: String,
    dial_press: String,
    dial_cw: String,
    accent_color: String,
    jog_flash: JogFlash,
    jog_flash_until: Option<Instant>,
}

impl HotkeyAction {
    pub fn new() -> Self {
        Self {
            surface: SlotSurface::Key,
            mode_idx: 0,
            combo_a: String::new(),
            combo_b: String::new(),
            text_snippet: String::new(),
            press_enter: false,
            toggle_state: 0,
            dial_ccw: "Ctrl+Z".to_string(),
            dial_press: "Space".to_string(),
            dial_cw: "Ctrl+Y".to_string(),
            accent_color: "#9146ff".to_string(),
            jog_flash: JogFlash::None,
            jog_flash_until: None,
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.surface = ctx.surface;
        self.mode_idx = ctx.get_usize("mode", 0).min(HOTKEY_MODES.len() - 1);
        self.combo_a = ctx.get_str("combo_a", "");
        self.combo_b = ctx.get_str("combo_b", "");
        self.text_snippet = ctx.get_str("text", "");
        self.press_enter = ctx.get_bool("press_enter", false);
        self.dial_ccw = ctx.get_str("dial_ccw", "Ctrl+Z");
        self.dial_press = ctx.get_str("dial_press", "Space");
        self.dial_cw = ctx.get_str("dial_cw", "Ctrl+Y");
        self.accent_color = ctx.get_str("accent_color", "#9146ff");
    }

    fn active_jog_flash(&self) -> JogFlash {
        if let Some(until) = self.jog_flash_until {
            if Instant::now() < until {
                return self.jog_flash;
            }
        }
        JogFlash::None
    }

    fn trigger_jog(&mut self, which: JogFlash, combo: String, ctx: &mut ActionContext) {
        self.jog_flash = which;
        self.jog_flash_until = Some(Instant::now() + Duration::from_millis(380));
        ctx.mark_dirty();
        if !combo.trim().is_empty() {
            std::thread::spawn(move || {
                fire_hotkey(&combo);
            });
        }
    }

    fn render_dial_jog_card(&self, cr: &cairo::Context, w: f64, h: f64) {
        let (ar, ag, ab) = parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0));
        let flash = self.active_jog_flash();

        draw_cairo_text(
            cr,
            "HOTKEY JOG",
            8.0,
            h * 0.08,
            (h * 0.11).clamp(8.0, 11.0),
            (ar, ag, ab),
            true,
            pango::Alignment::Center,
            w - 16.0,
        );

        let pad_x = 7.0;
        let gap = 5.0;
        let cap_w = ((w - pad_x * 2.0 - gap * 2.0) / 3.0).max(20.0);
        let cap_h = (h * 0.56).clamp(32.0, 60.0);
        let cap_y = h * 0.28;

        let slots = [
            (JogFlash::Ccw, "◀ CCW", self.dial_ccw.as_str()),
            (JogFlash::Press, "● PRESS", self.dial_press.as_str()),
            (JogFlash::Cw, "CW ▶", self.dial_cw.as_str()),
        ];

        for (i, (kind, sub, key_txt)) in slots.iter().enumerate() {
            let x = pad_x + i as f64 * (cap_w + gap);
            let lit = flash == *kind;
            draw_pill(cr, x, cap_y, cap_w, cap_h, 7.0);
            cr.set_source_rgba(ar, ag, ab, if lit { 0.85 } else { 0.14 });
            let _ = cr.fill_preserve();
            cr.set_source_rgba(ar, ag, ab, if lit { 1.0 } else { 0.58 });
            cr.set_line_width(if lit { 1.8 } else { 1.2 });
            let _ = cr.stroke();

            draw_cairo_text(
                cr,
                sub,
                x + 2.0,
                cap_y + cap_h * 0.12,
                (h * 0.088).clamp(7.0, 9.5),
                if lit { (1.0, 1.0, 1.0) } else { (0.72, 0.72, 0.84) },
                true,
                pango::Alignment::Center,
                cap_w - 4.0,
            );

            let label = if key_txt.trim().is_empty() { "—" } else { key_txt.trim() };
            let k_sz = if label.len() > 7 {
                (h * 0.105).clamp(7.5, 10.5)
            } else {
                (h * 0.125).clamp(8.5, 13.0)
            };
            draw_cairo_text(
                cr,
                label,
                x + 2.0,
                cap_y + cap_h * 0.46,
                k_sz,
                (1.0, 1.0, 1.0),
                true,
                pango::Alignment::Center,
                cap_w - 4.0,
            );
        }
    }
}

impl BallistaAction for HotkeyAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        match self.mode_idx {
            0 => {
                let c = self.combo_a.clone();
                std::thread::spawn(move || {
                    fire_hotkey(&c);
                });
            }
            1 => {
                let c = if self.toggle_state == 0 {
                    self.combo_a.clone()
                } else {
                    self.combo_b.clone()
                };
                self.toggle_state = 1 - self.toggle_state;
                ctx.mark_dirty();
                std::thread::spawn(move || {
                    fire_hotkey(&c);
                });
            }
            _ => {
                let txt = self.text_snippet.clone();
                let enter = self.press_enter;
                std::thread::spawn(move || {
                    fire_type_text(&txt, enter);
                });
            }
        }
    }

    fn on_dial_rotate(&mut self, ctx: &mut ActionContext, delta: i32) {
        if delta < 0 {
            let c = self.dial_ccw.clone();
            self.trigger_jog(JogFlash::Ccw, c, ctx);
        } else if delta > 0 {
            let c = self.dial_cw.clone();
            self.trigger_jog(JogFlash::Cw, c, ctx);
        }
    }

    fn on_dial_press(&mut self, ctx: &mut ActionContext) {
        let c = self.dial_press.clone();
        self.trigger_jog(JogFlash::Press, c, ctx);
    }

    fn on_touch_tap(&mut self, ctx: &mut ActionContext, norm_x: f64, _norm_y: f64) {
        if norm_x < 0.33 {
            let c = self.dial_ccw.clone();
            self.trigger_jog(JogFlash::Ccw, c, ctx);
        } else if norm_x > 0.67 {
            let c = self.dial_cw.clone();
            self.trigger_jog(JogFlash::Cw, c, ctx);
        } else {
            let c = self.dial_press.clone();
            self.trigger_jog(JogFlash::Press, c, ctx);
        }
    }

    fn tick_interval(&self) -> Option<Duration> {
        if self.jog_flash_until.is_some() {
            Some(Duration::from_millis(200))
        } else {
            None
        }
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        if let Some(until) = self.jog_flash_until {
            if Instant::now() >= until {
                self.jog_flash = JogFlash::None;
                self.jog_flash_until = None;
                ctx.mark_dirty();
            }
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        if ctx.surface == SlotSurface::Dial {
            let mut r0 = make_row(0, "EntryRow", "Rotate CCW (Left)");
            r0.text = ctx.get_str("dial_ccw", "Ctrl+Z");

            let mut r1 = make_row(1, "EntryRow", "Dial Press");
            r1.text = ctx.get_str("dial_press", "Space");

            let mut r2 = make_row(2, "EntryRow", "Rotate CW (Right)");
            r2.text = ctx.get_str("dial_cw", "Ctrl+Y");

            let mut r3 = make_row(3, "ColorRow", "Accent Color");
            r3.color = ctx.get_str("accent_color", "#9146ff");

            return vec![r0, r1, r2, r3];
        }

        let mode = ctx.get_usize("mode", 0).min(HOTKEY_MODES.len() - 1);

        let mut r0 = make_row(0, "ComboRow", "Mode");
        r0.options = HOTKEY_MODES.iter().map(|s| s.to_string()).collect();
        r0.selected = mode;

        let mut r1 = make_row(1, "EntryRow", "Hotkey (e.g. Ctrl+Shift+M)");
        r1.text = ctx.get_str("combo_a", "");
        r1.visible = mode != 2;

        let mut r2 = make_row(2, "EntryRow", "Second Hotkey (State 2)");
        r2.text = ctx.get_str("combo_b", "");
        r2.visible = mode == 1;

        let mut r3 = make_row(3, "EntryRow", "Text to Type");
        r3.text = ctx.get_str("text", "");
        r3.visible = mode == 2;

        let mut r4 = make_row(4, "SwitchRow", "Press Enter After");
        r4.active = ctx.get_bool("press_enter", false);
        r4.visible = mode == 2;

        vec![r0, r1, r2, r3, r4]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        if ctx.surface == SlotSurface::Dial {
            match row_idx {
                0 => {
                    if let Some(t) = change.text {
                        ctx.set_setting("dial_ccw", json!(t));
                    }
                }
                1 => {
                    if let Some(t) = change.text {
                        ctx.set_setting("dial_press", json!(t));
                    }
                }
                2 => {
                    if let Some(t) = change.text {
                        ctx.set_setting("dial_cw", json!(t));
                    }
                }
                3 => {
                    if let Some(c) = change.color {
                        ctx.set_setting("accent_color", json!(c));
                    }
                }
                _ => {}
            }
        } else {
            match row_idx {
                0 => {
                    if let Some(s) = change.selected {
                        ctx.set_setting("mode", json!(s));
                        ctx.push(HostCommand::RefreshConfigRows);
                    }
                }
                1 => {
                    if let Some(t) = change.text {
                        ctx.set_setting("combo_a", json!(t));
                    }
                }
                2 => {
                    if let Some(t) = change.text {
                        ctx.set_setting("combo_b", json!(t));
                    }
                }
                3 => {
                    if let Some(t) = change.text {
                        ctx.set_setting("text", json!(t));
                    }
                }
                4 => {
                    if let Some(a) = change.active {
                        ctx.set_setting("press_enter", json!(a));
                    }
                }
                _ => {}
            }
        }
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let surf = if w > h * 1.35 {
            SlotSurface::Dial
        } else {
            self.surface
        };
        self.render_visual_for_surface(cr, w, h, surf)
    }

    fn render_visual_for_surface(
        &self,
        cr: &cairo::Context,
        w: f64,
        h: f64,
        surface: SlotSurface,
    ) -> bool {
        if surface == SlotSurface::Dial || w > h * 1.35 {
            self.render_dial_jog_card(cr, w, h);
            return true;
        }

        let min_d = w.min(h).max(24.0);

        // Draw crisp rounded keycap outline
        let kw = w * 0.72;
        let kh = h * 0.44;
        let kx = (w - kw) / 2.0;
        let ky = (h - kh) / 2.0 - h * 0.06;
        let r = (min_d * 0.08).clamp(4.0, 9.0);

        cr.new_sub_path();
        cr.arc(kx + kw - r, ky + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        cr.arc(kx + kw - r, ky + kh - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        cr.arc(kx + r, ky + kh - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        cr.arc(kx + r, ky + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
        cr.close_path();
        cr.set_source_rgba(0.569, 0.275, 1.0, if self.mode_idx == 1 && self.toggle_state == 1 { 0.32 } else { 0.14 });
        let _ = cr.fill_preserve();
        cr.set_source_rgba(0.659, 0.400, 1.0, 0.85);
        cr.set_line_width(1.6);
        let _ = cr.stroke();

        let cap_text = match self.mode_idx {
            2 => "Aa",
            1 => {
                let active = if self.toggle_state == 0 { &self.combo_a } else { &self.combo_b };
                if active.is_empty() { "A / B" } else { active.as_str() }
            }
            _ => {
                if self.combo_a.is_empty() { "⌘" } else { self.combo_a.as_str() }
            }
        };

        let sz = if cap_text.len() > 6 {
            (min_d * 0.125).clamp(8.5, 13.0)
        } else {
            (min_d * 0.17).clamp(10.0, 19.0)
        };
        draw_cairo_text(
            cr,
            cap_text,
            kx + 3.0,
            ky + kh / 2.0 - sz * 0.62,
            sz,
            (0.96, 0.96, 1.0),
            true,
            pango::Alignment::Center,
            kw - 6.0,
        );
        true
    }
}

/// Fires a key combination (e.g. "Ctrl+Shift+M") natively via Linux uinput,
/// with graceful fallbacks to `ydotool`, `wtype`, or `xdotool`.
pub fn fire_hotkey(combo: &str) {
    let trimmed = combo.trim();
    if trimmed.is_empty() {
        return;
    }
    let keycodes: Vec<u16> = trimmed
        .split('+')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .filter_map(parse_linux_keycode)
        .collect();

    if !keycodes.is_empty() && send_uinput_chord(&keycodes) {
        return;
    }

    // Fallback 1: ydotool
    if !keycodes.is_empty() {
        let mut args: Vec<String> = vec!["key".into()];
        for &kc in &keycodes {
            args.push(format!("{kc}:1"));
        }
        for &kc in keycodes.iter().rev() {
            args.push(format!("{kc}:0"));
        }
        if Command::new("ydotool").args(&args).status().map(|s| s.success()).unwrap_or(false) {
            return;
        }
    }

    // Fallback 2: xdotool (X11 / XWayland)
    let xdo_combo = trimmed
        .split('+')
        .map(|p| match p.trim().to_lowercase().as_str() {
            "ctrl" | "control" => "ctrl".to_string(),
            "shift" => "shift".to_string(),
            "alt" => "alt".to_string(),
            "super" | "meta" | "win" | "cmd" => "super".to_string(),
            "enter" | "return" => "Return".to_string(),
            "esc" | "escape" => "Escape".to_string(),
            "space" => "space".to_string(),
            "tab" => "Tab".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+");
    if Command::new("xdotool").args(["key", "--clearmodifiers", &xdo_combo]).status().map(|s| s.success()).unwrap_or(false) {
        return;
    }

    tracing::warn!(
        "Hotkey '{}' could not inject input: /dev/uinput not writable and neither ydotool nor xdotool succeeded",
        trimmed
    );
}

/// Types a text snippet natively via uinput (ASCII) with fallback to `wtype`, `ydotool`, or `xdotool`.
pub fn fire_type_text(text: &str, press_enter: bool) {
    if text.is_empty() && !press_enter {
        return;
    }
    if send_uinput_text(text, press_enter) {
        return;
    }
    // Fallback 1: wtype (Wayland)
    let mut w = Command::new("wtype");
    if !text.is_empty() {
        w.arg(text);
    }
    if press_enter {
        w.args(["-k", "Return"]);
    }
    if w.status().map(|s| s.success()).unwrap_or(false) {
        return;
    }
    // Fallback 2: ydotool
    if !text.is_empty() {
        let _ = Command::new("ydotool").args(["type", "--", text]).status();
    }
    if press_enter {
        let _ = Command::new("ydotool").args(["key", "28:1", "28:0"]).status();
    }
    // Fallback 3: xdotool
    let _ = Command::new("xdotool").args(["type", "--clearmodifiers", "--", text]).status();
    if press_enter {
        let _ = Command::new("xdotool").args(["key", "Return"]).status();
    }
}

// ===================== Pure-Rust Linux /dev/uinput Virtual Keyboard =====================

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const SYN_REPORT: u16 = 0x00;

const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;
const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;

#[repr(C)]
struct InputEvent {
    tv_sec: libc::time_t,
    tv_usec: libc::suseconds_t,
    type_: u16,
    code: u16,
    value: i32,
}

#[repr(C)]
struct UinputUserDev {
    name: [u8; 80],
    id_bustype: u16,
    id_vendor: u16,
    id_product: u16,
    id_version: u16,
    ff_effects_max: u32,
    absmax: [i32; 64],
    absmin: [i32; 64],
    absfuzz: [i32; 64],
    absflat: [i32; 64],
}

fn emit_ev(file: &mut std::fs::File, type_: u16, code: u16, value: i32) {
    let ev = InputEvent {
        tv_sec: 0,
        tv_usec: 0,
        type_,
        code,
        value,
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (&ev as *const InputEvent) as *const u8,
            std::mem::size_of::<InputEvent>(),
        )
    };
    let _ = file.write_all(bytes);
}

fn open_uinput_keyboard(keys: &[u16]) -> Option<std::fs::File> {
    let mut file = OpenOptions::new().write(true).open("/dev/uinput").ok()?;
    let fd = file.as_raw_fd();
    unsafe {
        if libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as libc::c_int) < 0 {
            return None;
        }
        if libc::ioctl(fd, UI_SET_EVBIT, EV_SYN as libc::c_int) < 0 {
            return None;
        }
        for &k in keys {
            libc::ioctl(fd, UI_SET_KEYBIT, k as libc::c_int);
        }
        let mut dev: UinputUserDev = std::mem::zeroed();
        let name = b"Ballista Virtual Keyboard";
        dev.name[..name.len()].copy_from_slice(name);
        dev.id_bustype = 0x03; // BUS_USB
        dev.id_vendor = 0x1234;
        dev.id_product = 0x5678;
        dev.id_version = 1;
        let bytes = std::slice::from_raw_parts(
            (&dev as *const UinputUserDev) as *const u8,
            std::mem::size_of::<UinputUserDev>(),
        );
        file.write_all(bytes).ok()?;
        if libc::ioctl(fd, UI_DEV_CREATE) < 0 {
            return None;
        }
    }
    // Give udev/compositor a brief window to attach the virtual input node
    std::thread::sleep(Duration::from_millis(60));
    Some(file)
}

fn send_uinput_chord(keys: &[u16]) -> bool {
    let Some(mut file) = open_uinput_keyboard(keys) else {
        return false;
    };
    for &k in keys {
        emit_ev(&mut file, EV_KEY, k, 1);
        emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        std::thread::sleep(Duration::from_millis(8));
    }
    std::thread::sleep(Duration::from_millis(25));
    for &k in keys.iter().rev() {
        emit_ev(&mut file, EV_KEY, k, 0);
        emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        std::thread::sleep(Duration::from_millis(8));
    }
    std::thread::sleep(Duration::from_millis(25));
    unsafe {
        libc::ioctl(file.as_raw_fd(), UI_DEV_DESTROY);
    }
    true
}

fn send_uinput_text(text: &str, press_enter: bool) -> bool {
    let mut seq: Vec<(bool, u16)> = Vec::new();
    let mut all_keys: Vec<u16> = vec![42, 28]; // KEY_LEFTSHIFT, KEY_ENTER
    for ch in text.chars() {
        let Some((shift, kc)) = ascii_to_keycode(ch) else {
            return false; // Non-ASCII -> let wtype/xdotool handle it
        };
        seq.push((shift, kc));
        if !all_keys.contains(&kc) {
            all_keys.push(kc);
        }
    }
    let Some(mut file) = open_uinput_keyboard(&all_keys) else {
        return false;
    };
    for (shift, kc) in seq {
        if shift {
            emit_ev(&mut file, EV_KEY, 42, 1);
            emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        }
        emit_ev(&mut file, EV_KEY, kc, 1);
        emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        std::thread::sleep(Duration::from_millis(6));
        emit_ev(&mut file, EV_KEY, kc, 0);
        emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        if shift {
            emit_ev(&mut file, EV_KEY, 42, 0);
            emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        }
        std::thread::sleep(Duration::from_millis(6));
    }
    if press_enter {
        emit_ev(&mut file, EV_KEY, 28, 1);
        emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
        std::thread::sleep(Duration::from_millis(12));
        emit_ev(&mut file, EV_KEY, 28, 0);
        emit_ev(&mut file, EV_SYN, SYN_REPORT, 0);
    }
    std::thread::sleep(Duration::from_millis(25));
    unsafe {
        libc::ioctl(file.as_raw_fd(), UI_DEV_DESTROY);
    }
    true
}

fn parse_linux_keycode(token: &str) -> Option<u16> {
    match token.to_lowercase().as_str() {
        "ctrl" | "control" | "lctrl" => Some(29),
        "rctrl" => Some(97),
        "shift" | "lshift" => Some(42),
        "rshift" => Some(54),
        "alt" | "lalt" => Some(56),
        "ralt" | "altgr" => Some(100),
        "super" | "meta" | "win" | "cmd" => Some(125),
        "esc" | "escape" => Some(1),
        "1" => Some(2), "2" => Some(3), "3" => Some(4), "4" => Some(5), "5" => Some(6),
        "6" => Some(7), "7" => Some(8), "8" => Some(9), "9" => Some(10), "0" => Some(11),
        "-" | "minus" => Some(12), "=" | "equal" => Some(13), "backspace" | "bksp" => Some(14),
        "tab" => Some(15),
        "q" => Some(16), "w" => Some(17), "e" => Some(18), "r" => Some(19), "t" => Some(20),
        "y" => Some(21), "u" => Some(22), "i" => Some(23), "o" => Some(24), "p" => Some(25),
        "[" => Some(26), "]" => Some(27), "enter" | "return" => Some(28),
        "a" => Some(30), "s" => Some(31), "d" => Some(32), "f" => Some(33), "g" => Some(34),
        "h" => Some(35), "j" => Some(36), "k" => Some(37), "l" => Some(38),
        ";" => Some(39), "'" => Some(40), "`" => Some(41), "\\" => Some(43),
        "z" => Some(44), "x" => Some(45), "c" => Some(46), "v" => Some(47), "b" => Some(48),
        "n" => Some(49), "m" => Some(50), "," => Some(51), "." => Some(52), "/" => Some(53),
        "space" => Some(57),
        "f1" => Some(59), "f2" => Some(60), "f3" => Some(61), "f4" => Some(62),
        "f5" => Some(63), "f6" => Some(64), "f7" => Some(65), "f8" => Some(66),
        "f9" => Some(67), "f10" => Some(68), "f11" => Some(87), "f12" => Some(88),
        "f13" => Some(183), "f14" => Some(184), "f15" => Some(185), "f16" => Some(186),
        "f17" => Some(187), "f18" => Some(188), "f19" => Some(189), "f20" => Some(190),
        "home" => Some(102), "up" => Some(103), "pageup" | "pgup" => Some(104),
        "left" => Some(105), "right" => Some(106), "end" => Some(107),
        "down" => Some(108), "pagedown" | "pgdn" => Some(109),
        "insert" | "ins" => Some(110), "delete" | "del" => Some(111),
        "mute" | "xf86audiomute" => Some(113),
        "volumedown" | "xf86audiolowervolume" => Some(114),
        "volumeup" | "xf86audioraisevolume" => Some(115),
        "playpause" | "xf86audioplay" => Some(164),
        "next" | "xf86audionext" => Some(163),
        "prev" | "previous" | "xf86audioprev" => Some(165),
        _ => None,
    }
}

fn ascii_to_keycode(ch: char) -> Option<(bool, u16)> {
    match ch {
        'a'..='z' => Some((false, parse_linux_keycode(&ch.to_string())?)),
        'A'..='Z' => Some((true, parse_linux_keycode(&ch.to_ascii_lowercase().to_string())?)),
        '0'..='9' => Some((false, parse_linux_keycode(&ch.to_string())?)),
        ' ' => Some((false, 57)),
        '\n' => Some((false, 28)),
        '\t' => Some((false, 15)),
        '-' => Some((false, 12)), '_' => Some((true, 12)),
        '=' => Some((false, 13)), '+' => Some((true, 13)),
        '[' => Some((false, 26)), '{' => Some((true, 26)),
        ']' => Some((false, 27)), '}' => Some((true, 27)),
        ';' => Some((false, 39)), ':' => Some((true, 39)),
        '\'' => Some((false, 40)), '"' => Some((true, 40)),
        '`' => Some((false, 41)), '~' => Some((true, 41)),
        '\\' => Some((false, 43)), '|' => Some((true, 43)),
        ',' => Some((false, 51)), '<' => Some((true, 51)),
        '.' => Some((false, 52)), '>' => Some((true, 52)),
        '/' => Some((false, 53)), '?' => Some((true, 53)),
        '!' => Some((true, 2)), '@' => Some((true, 3)), '#' => Some((true, 4)),
        '$' => Some((true, 5)), '%' => Some((true, 6)), '^' => Some((true, 7)),
        '&' => Some((true, 8)), '*' => Some((true, 9)), '(' => Some((true, 10)),
        ')' => Some((true, 11)),
        _ => None,
    }
}
