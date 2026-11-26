use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand, SlotSurface,
};
use crate::plugins::ballista_core::system_metrics::{draw_cairo_text, draw_pill, parse_hex};
use serde_json::json;
use std::time::{Duration, Instant};

const KEY_STYLE_OPTIONS: &[&str] = &["Digital", "Analog"];
const DIAL_STYLE_OPTIONS: &[&str] = &["Split (Analog + Digital)", "Digital", "Analog"];
const ANALOG_THEMES: &[&str] = &["Midnight Dark", "Classic Light", "Minimal"];

const WEEKDAYS: &[&str] = &["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

// Quick world timezone presets cycled by rotating the dial knob
const TZ_PRESETS: &[(Option<i32>, &str)] = &[
    (None, "LOCAL"),
    (Some(0), "UTC"),
    (Some(-8), "UTC-8"),
    (Some(-5), "UTC-5"),
    (Some(1), "UTC+1"),
    (Some(2), "UTC+2"),
    (Some(5), "UTC+5"),
    (Some(8), "UTC+8"),
    (Some(9), "UTC+9"),
];

pub struct ClockAction {
    surface: SlotSurface,
    style_idx: usize,
    dial_style_idx: usize, // 0 = Split, 1 = Digital, 2 = Analog
    use_24h: bool,
    show_seconds: bool,
    show_date: bool,
    analog_theme: usize,
    use_local_tz: bool,
    utc_offset_hrs: i32,
    accent_color: String,
    last_rendered_sec: i64,
    last_rotate_tz: Option<Instant>,
}

impl ClockAction {
    pub fn new() -> Self {
        Self {
            surface: SlotSurface::Key,
            style_idx: 0,
            dial_style_idx: 0,
            use_24h: false,
            show_seconds: false,
            show_date: true,
            analog_theme: 0,
            use_local_tz: true,
            utc_offset_hrs: 0,
            accent_color: "#9146ff".to_string(),
            last_rendered_sec: -1,
            last_rotate_tz: None,
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.surface = ctx.surface;
        self.style_idx = ctx.get_usize("style", 0).min(KEY_STYLE_OPTIONS.len() - 1);
        self.dial_style_idx = ctx
            .get_usize("dial_style", 0)
            .min(DIAL_STYLE_OPTIONS.len() - 1);
        self.use_24h = ctx.get_bool("use_24h", false);
        self.show_seconds = ctx.get_bool("show_seconds", false);
        self.show_date = ctx.get_bool("show_date", true);
        self.analog_theme = ctx
            .get_usize("analog_theme", 0)
            .min(ANALOG_THEMES.len() - 1);
        self.use_local_tz = ctx.get_bool("use_local_tz", true);
        self.utc_offset_hrs = ctx.get_f64("utc_offset", 0.0).round() as i32;
        self.accent_color = ctx.get_str("accent_color", "#9146ff");
    }

    fn current_tm(&self) -> (i32, i32, i32, i32, i32, i32, i64) {
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if self.use_local_tz {
            unsafe { libc::localtime_r(&now, &mut tm) };
        } else {
            let shifted = now + (self.utc_offset_hrs as i64) * 3600;
            unsafe { libc::gmtime_r(&shifted, &mut tm) };
        }
        (
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
            tm.tm_wday,
            tm.tm_mon,
            tm.tm_mday,
            now,
        )
    }

    fn tz_badge(&self) -> String {
        if self.use_local_tz {
            "LOCAL".to_string()
        } else if self.utc_offset_hrs == 0 {
            "UTC".to_string()
        } else if self.utc_offset_hrs > 0 {
            format!("UTC+{}", self.utc_offset_hrs)
        } else {
            format!("UTC{}", self.utc_offset_hrs)
        }
    }

    fn draw_analog_face(
        &self,
        cr: &cairo::Context,
        cx: f64,
        cy: f64,
        r: f64,
        hour24: i32,
        min: i32,
        sec: i32,
        accent: (f64, f64, f64),
    ) {
        let (ar, ag, ab) = accent;
        let (face_rgb, tick_rgb, hand_rgb) = match self.analog_theme {
            1 => ((0.97, 0.97, 0.99), (0.25, 0.25, 0.30), (0.10, 0.10, 0.14)),
            2 => ((0.09, 0.09, 0.12), (0.55, 0.55, 0.65), (0.95, 0.95, 0.98)),
            _ => ((0.11, 0.11, 0.15), (0.72, 0.72, 0.82), (0.96, 0.96, 0.99)),
        };
        let d = r * 2.0;

        cr.set_source_rgb(face_rgb.0, face_rgb.1, face_rgb.2);
        cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();

        cr.set_source_rgba(ar, ag, ab, 0.55);
        cr.set_line_width((d * 0.028).clamp(1.2, 2.4));
        cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        for i in 0..12 {
            let angle = (i as f64) * (std::f64::consts::PI / 6.0) - std::f64::consts::FRAC_PI_2;
            let is_major = i % 3 == 0;
            if self.analog_theme == 2 && !is_major {
                continue;
            }
            let inner_r = if is_major { r * 0.75 } else { r * 0.85 };
            let outer_r = r * 0.92;
            cr.set_source_rgba(
                tick_rgb.0,
                tick_rgb.1,
                tick_rgb.2,
                if is_major { 0.95 } else { 0.52 },
            );
            cr.set_line_width(if is_major {
                (d * 0.032).clamp(1.5, 2.8)
            } else {
                (d * 0.016).clamp(1.0, 1.6)
            });
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(cx + angle.cos() * inner_r, cy + angle.sin() * inner_r);
            cr.line_to(cx + angle.cos() * outer_r, cy + angle.sin() * outer_r);
            let _ = cr.stroke();
        }

        // Hour hand
        let h_angle = (((hour24 % 12) as f64) + (min as f64) / 60.0)
            * (std::f64::consts::PI / 6.0)
            - std::f64::consts::FRAC_PI_2;
        cr.set_source_rgb(hand_rgb.0, hand_rgb.1, hand_rgb.2);
        cr.set_line_width((d * 0.050).clamp(2.0, 4.2));
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(cx, cy);
        cr.line_to(
            cx + h_angle.cos() * (r * 0.50),
            cy + h_angle.sin() * (r * 0.50),
        );
        let _ = cr.stroke();

        // Minute hand
        let m_angle = ((min as f64) + (sec as f64) / 60.0) * (std::f64::consts::PI / 30.0)
            - std::f64::consts::FRAC_PI_2;
        cr.set_source_rgb(hand_rgb.0, hand_rgb.1, hand_rgb.2);
        cr.set_line_width((d * 0.034).clamp(1.5, 3.0));
        cr.set_line_cap(cairo::LineCap::Round);
        cr.move_to(cx, cy);
        cr.line_to(
            cx + m_angle.cos() * (r * 0.72),
            cy + m_angle.sin() * (r * 0.72),
        );
        let _ = cr.stroke();

        // Second hand
        if self.show_seconds {
            let s_angle =
                (sec as f64) * (std::f64::consts::PI / 30.0) - std::f64::consts::FRAC_PI_2;
            cr.set_source_rgb(ar, ag, ab);
            cr.set_line_width((d * 0.018).clamp(1.0, 1.8));
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(
                cx - s_angle.cos() * (r * 0.18),
                cy - s_angle.sin() * (r * 0.18),
            );
            cr.line_to(
                cx + s_angle.cos() * (r * 0.80),
                cy + s_angle.sin() * (r * 0.80),
            );
            let _ = cr.stroke();
        }

        // Center pin
        cr.set_source_rgb(ar, ag, ab);
        cr.arc(
            cx,
            cy,
            (d * 0.044).clamp(2.0, 4.0),
            0.0,
            2.0 * std::f64::consts::PI,
        );
        let _ = cr.fill();
    }

    fn render_dial_card(&self, cr: &cairo::Context, w: f64, h: f64) {
        let (hour24, min, sec, wday, mon, mday, _) = self.current_tm();
        let accent = parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0));
        let (ar, ag, ab) = accent;

        let bg_grad = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
        bg_grad.add_color_stop_rgb(0.0, 0.118, 0.118, 0.165);
        bg_grad.add_color_stop_rgb(1.0, 0.070, 0.070, 0.100);
        let _ = cr.set_source(&bg_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        let (disp_hour, ampm) = if self.use_24h {
            (hour24, "")
        } else {
            let suffix = if hour24 >= 12 { "PM" } else { "AM" };
            let h12 = match hour24 % 12 {
                0 => 12,
                n => n,
            };
            (h12, suffix)
        };
        let time_str = if self.show_seconds {
            format!("{:02}:{:02}:{:02}", disp_hour, min, sec)
        } else {
            format!("{:02}:{:02}", disp_hour, min)
        };
        let day_str = WEEKDAYS.get(wday as usize).copied().unwrap_or("");
        let mon_str = MONTHS.get(mon as usize).copied().unwrap_or("");
        let date_str = format!("{}, {} {}", day_str, mon_str, mday);
        let tz_str = self.tz_badge();

        match self.dial_style_idx {
            1 => {
                // Full Widescreen Digital Clock
                cr.set_source_rgba(ar, ag, ab, 0.85);
                cr.rectangle(w * 0.22, 3.0, w * 0.56, 2.5);
                let _ = cr.fill();

                let time_sz = if self.show_seconds {
                    (h * 0.30).clamp(16.0, 30.0)
                } else {
                    (h * 0.38).clamp(20.0, 38.0)
                };
                let full_time = if ampm.is_empty() {
                    time_str
                } else {
                    format!("{time_str} {ampm}")
                };
                draw_cairo_text(
                    cr,
                    &full_time,
                    6.0,
                    h * 0.38 - time_sz * 0.56,
                    time_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    w - 12.0,
                );

                let foot_sz = (h * 0.13).clamp(9.0, 13.0);
                draw_cairo_text(
                    cr,
                    &format!("{}  ·  {}", date_str, tz_str),
                    6.0,
                    h - foot_sz * 1.9,
                    foot_sz,
                    (0.76, 0.76, 0.86),
                    false,
                    pango::Alignment::Center,
                    w - 12.0,
                );
            }
            2 => {
                // Analog Left + Large Day/Date/TZ Right
                let r = (h * 0.40).clamp(18.0, 42.0);
                let cx = r + 14.0;
                let cy = h / 2.0;
                self.draw_analog_face(cr, cx, cy, r, hour24, min, sec, accent);

                let rx = cx + r + 14.0;
                let rw = (w - rx - 8.0).max(30.0);
                draw_cairo_text(
                    cr,
                    day_str,
                    rx,
                    h * 0.16,
                    (h * 0.20).clamp(12.0, 20.0),
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Left,
                    rw,
                );
                draw_cairo_text(
                    cr,
                    &format!("{} {}", mon_str, mday),
                    rx,
                    h * 0.44,
                    (h * 0.165).clamp(10.0, 17.0),
                    (0.95, 0.95, 0.99),
                    true,
                    pango::Alignment::Left,
                    rw,
                );
                draw_cairo_text(
                    cr,
                    &tz_str,
                    rx,
                    h * 0.70,
                    (h * 0.115).clamp(8.5, 11.5),
                    (0.68, 0.68, 0.78),
                    true,
                    pango::Alignment::Left,
                    rw,
                );
            }
            _ => {
                // SPLIT CARD (Default Dial Flagship): Vector Analog Left + Bold Digital + Date + TZ Pill Right
                let r = (h * 0.38).clamp(16.0, 40.0);
                let cx = r + 11.0;
                let cy = h / 2.0;
                self.draw_analog_face(cr, cx, cy, r, hour24, min, sec, accent);

                let rx = cx + r + 11.0;
                let rw = (w - rx - 8.0).max(30.0);

                // Timezone pill top-right
                let pill_w = (tz_str.len() as f64 * 6.5 + 12.0).clamp(40.0, rw);
                let pill_h = (h * 0.17).clamp(13.0, 18.0);
                draw_pill(cr, rx, h * 0.10, pill_w, pill_h, pill_h / 2.0);
                cr.set_source_rgba(ar, ag, ab, 0.24);
                let _ = cr.fill();
                draw_cairo_text(
                    cr,
                    &tz_str,
                    rx,
                    h * 0.10 + (pill_h - (h * 0.105).clamp(8.0, 10.5)) * 0.38,
                    (h * 0.105).clamp(8.0, 10.5),
                    (0.90, 0.84, 1.0),
                    true,
                    pango::Alignment::Center,
                    pill_w,
                );

                // Digital time center-right
                let time_sz = if self.show_seconds {
                    (h * 0.185).clamp(11.0, 19.0)
                } else {
                    (h * 0.245).clamp(14.0, 25.0)
                };
                let full_time = if ampm.is_empty() {
                    time_str
                } else {
                    format!("{time_str} {ampm}")
                };
                draw_cairo_text(
                    cr,
                    &full_time,
                    rx,
                    h * 0.35,
                    time_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Left,
                    rw,
                );

                // Date bottom-right
                if self.show_date {
                    draw_cairo_text(
                        cr,
                        &date_str,
                        rx,
                        h * 0.69,
                        (h * 0.125).clamp(8.5, 13.0),
                        (0.74, 0.74, 0.84),
                        false,
                        pango::Alignment::Left,
                        rw,
                    );
                }
            }
        }
    }
}

impl BallistaAction for ClockAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        self.style_idx = (self.style_idx + 1) % KEY_STYLE_OPTIONS.len();
        ctx.set_setting("style", json!(self.style_idx));
        ctx.push(HostCommand::RefreshConfigRows);
        ctx.mark_dirty();
    }

    fn on_dial_press(&mut self, ctx: &mut ActionContext) {
        // Cycles Split -> Digital -> Analog on Dial!
        self.dial_style_idx = (self.dial_style_idx + 1) % DIAL_STYLE_OPTIONS.len();
        ctx.set_setting("dial_style", json!(self.dial_style_idx));
        ctx.push(HostCommand::RefreshConfigRows);
        ctx.mark_dirty();
    }

    fn on_dial_rotate(&mut self, ctx: &mut ActionContext, delta: i32) {
        if delta == 0 {
            return;
        }
        let now = Instant::now();
        if let Some(last) = self.last_rotate_tz {
            if now.duration_since(last) < Duration::from_millis(160) {
                return;
            }
        }
        self.last_rotate_tz = Some(now);

        // Cycle world timezone presets right from the knob!
        let cur_idx = TZ_PRESETS
            .iter()
            .position(|(off, _)| {
                if self.use_local_tz {
                    off.is_none()
                } else {
                    *off == Some(self.utc_offset_hrs)
                }
            })
            .unwrap_or(0);
        let n = TZ_PRESETS.len() as i32;
        let next = (cur_idx as i32 + if delta > 0 { 1 } else { -1 }).rem_euclid(n) as usize;
        match TZ_PRESETS[next].0 {
            None => {
                self.use_local_tz = true;
                ctx.set_setting("use_local_tz", json!(true));
            }
            Some(hrs) => {
                self.use_local_tz = false;
                self.utc_offset_hrs = hrs;
                ctx.set_setting("use_local_tz", json!(false));
                ctx.set_setting("utc_offset", json!(hrs));
            }
        }
        self.last_rendered_sec = -1;
        ctx.push(HostCommand::RefreshConfigRows);
        ctx.mark_dirty();
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_secs(1))
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        let (_, min, sec, _, _, _, now) = self.current_tm();
        let key_val = if self.show_seconds {
            now
        } else {
            now / 60 + (min as i64) * 10000 + if sec == 0 { 1 } else { 0 }
        };
        if key_val != self.last_rendered_sec {
            self.last_rendered_sec = key_val;
            ctx.mark_dirty();
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let is_dial = ctx.surface == SlotSurface::Dial;
        let use_local = ctx.get_bool("use_local_tz", true);

        let (style_opts, cur_style, show_digital_opts, show_analog_opts) = if is_dial {
            let ds = ctx
                .get_usize("dial_style", 0)
                .min(DIAL_STYLE_OPTIONS.len() - 1);
            (DIAL_STYLE_OPTIONS, ds, ds != 2, ds != 1)
        } else {
            let ks = ctx.get_usize("style", 0).min(KEY_STYLE_OPTIONS.len() - 1);
            (KEY_STYLE_OPTIONS, ks, ks == 0, ks == 1)
        };

        let mut r0 = make_row(0, "ComboRow", "Clock Style");
        r0.options = style_opts.iter().map(|s| s.to_string()).collect();
        r0.selected = cur_style;

        let mut r1 = make_row(1, "SwitchRow", "24-Hour Format");
        r1.active = ctx.get_bool("use_24h", false);
        r1.visible = show_digital_opts;

        let mut r2 = make_row(2, "SwitchRow", "Show Seconds");
        r2.active = ctx.get_bool("show_seconds", false);

        let mut r3 = make_row(3, "SwitchRow", "Show Date");
        r3.active = ctx.get_bool("show_date", true);
        r3.visible = show_digital_opts;

        let mut r4 = make_row(4, "ComboRow", "Analog Face Theme");
        r4.options = ANALOG_THEMES.iter().map(|s| s.to_string()).collect();
        r4.selected = ctx
            .get_usize("analog_theme", 0)
            .min(r4.options.len() - 1);
        r4.visible = show_analog_opts;

        let mut r5 = make_row(5, "SwitchRow", "Use Local Time");
        r5.active = use_local;

        let mut r6 = make_row(6, "SpinRow", "UTC Offset (Hours)");
        r6.min = -12.0;
        r6.max = 14.0;
        r6.step = 1.0;
        r6.value = ctx.get_f64("utc_offset", 0.0);
        r6.visible = !use_local;

        let mut r7 = make_row(7, "ColorRow", "Accent Color");
        r7.color = ctx.get_str("accent_color", "#9146ff");

        let mut rows = vec![r0, r1, r2, r3, r4, r5, r6, r7];
        if is_dial {
            let mut r8 = make_row(8, "ActionRow", "Dial Controls");
            r8.subtitle = "Rotate: Cycle World Timezones · Press: Switch Layout".into();
            rows.push(r8);
        }
        rows
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        match row_idx {
            0 => {
                if let Some(s) = change.selected {
                    if ctx.surface == SlotSurface::Dial {
                        ctx.set_setting("dial_style", json!(s));
                    } else {
                        ctx.set_setting("style", json!(s));
                    }
                }
            }
            1 => {
                if let Some(a) = change.active {
                    ctx.set_setting("use_24h", json!(a));
                }
            }
            2 => {
                if let Some(a) = change.active {
                    ctx.set_setting("show_seconds", json!(a));
                }
            }
            3 => {
                if let Some(a) = change.active {
                    ctx.set_setting("show_date", json!(a));
                }
            }
            4 => {
                if let Some(s) = change.selected {
                    ctx.set_setting("analog_theme", json!(s));
                }
            }
            5 => {
                if let Some(a) = change.active {
                    ctx.set_setting("use_local_tz", json!(a));
                }
            }
            6 => {
                if let Some(v) = change.value {
                    ctx.set_setting("utc_offset", json!(v.round() as i32));
                }
            }
            7 => {
                if let Some(c) = change.color {
                    ctx.set_setting("accent_color", json!(c));
                }
            }
            _ => {}
        }
        self.sync_from_settings(ctx);
        self.last_rendered_sec = -1;
        ctx.push(HostCommand::RefreshConfigRows);
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
            self.render_dial_card(cr, w, h);
            return true;
        }

        let (hour24, min, sec, wday, mon, mday, _) = self.current_tm();
        let accent = parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0));
        let (ar, ag, ab) = accent;
        let min_d = w.min(h).max(24.0);

        if self.style_idx == 1 {
            let (bg_top, bg_bot) = match self.analog_theme {
                1 => ((0.92, 0.93, 0.96), (0.84, 0.86, 0.90)),
                2 => ((0.08, 0.08, 0.10), (0.06, 0.06, 0.08)),
                _ => ((0.12, 0.12, 0.16), (0.07, 0.07, 0.10)),
            };
            let bg_grad = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
            bg_grad.add_color_stop_rgb(0.0, bg_top.0, bg_top.1, bg_top.2);
            bg_grad.add_color_stop_rgb(1.0, bg_bot.0, bg_bot.1, bg_bot.2);
            let _ = cr.set_source(&bg_grad);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();

            self.draw_analog_face(
                cr,
                w / 2.0,
                h / 2.0,
                (min_d * 0.43).max(12.0),
                hour24,
                min,
                sec,
                accent,
            );
        } else {
            // DIGITAL KEY CLOCK
            let bg_grad = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
            bg_grad.add_color_stop_rgb(0.0, 0.115, 0.118, 0.160);
            bg_grad.add_color_stop_rgb(1.0, 0.072, 0.072, 0.098);
            let _ = cr.set_source(&bg_grad);
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();

            cr.set_source_rgba(ar, ag, ab, 0.85);
            cr.rectangle(w * 0.25, 3.0, w * 0.50, 2.5);
            let _ = cr.fill();

            let (disp_hour, ampm) = if self.use_24h {
                (hour24, "")
            } else {
                let suffix = if hour24 >= 12 { "PM" } else { "AM" };
                let h12 = match hour24 % 12 {
                    0 => 12,
                    n => n,
                };
                (h12, suffix)
            };

            let time_str = if self.show_seconds {
                format!("{:02}:{:02}:{:02}", disp_hour, min, sec)
            } else {
                format!("{:02}:{:02}", disp_hour, min)
            };

            let time_sz = if self.show_seconds {
                (min_d * 0.205).clamp(10.0, 24.0)
            } else {
                (min_d * 0.285).clamp(13.0, 34.0)
            };

            let time_y = if self.show_date {
                h * 0.42 - time_sz * 0.58
            } else {
                h * 0.50 - time_sz * 0.58
            };

            draw_cairo_text(
                cr,
                &time_str,
                3.0,
                time_y,
                time_sz,
                (1.0, 1.0, 1.0),
                true,
                pango::Alignment::Center,
                w - 6.0,
            );

            if !ampm.is_empty() {
                let ap_sz = (min_d * 0.125).clamp(8.0, 13.0);
                draw_cairo_text(
                    cr,
                    ampm,
                    3.0,
                    (h * 0.11).max(6.0),
                    ap_sz,
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Center,
                    w - 6.0,
                );
            }

            if self.show_date {
                let day_str = WEEKDAYS.get(wday as usize).copied().unwrap_or("");
                let mon_str = MONTHS.get(mon as usize).copied().unwrap_or("");
                let date_str = format!("{} {} {}", day_str, mon_str, mday);
                let d_sz = (min_d * 0.135).clamp(8.5, 14.0);
                draw_cairo_text(
                    cr,
                    &date_str,
                    3.0,
                    h - d_sz * 1.75,
                    d_sz,
                    (0.74, 0.74, 0.84),
                    false,
                    pango::Alignment::Center,
                    w - 6.0,
                );
            }
        }

        true
    }
}
