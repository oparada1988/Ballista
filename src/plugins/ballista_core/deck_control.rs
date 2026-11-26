use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, BrightnessOp, ConfigChange, ConfigContext,
    HostCommand, PageNav, SlotSurface,
};
use crate::plugins::ballista_core::system_metrics::{draw_cairo_text, draw_pill, parse_hex};
use serde_json::json;
use std::time::{Duration, Instant};

const DECK_MODES: &[&str] = &[
    "Next Page",
    "Previous Page",
    "Go to Page",
    "Switch Profile",
    "Brightness",
    "Sleep / Wake Deck",
];
const DIAL_DECK_MODES: &[&str] = &["Brightness Slider", "Page Switcher", "Profile Switcher"];
const BRIGHTNESS_OPS: &[&str] = &["Set Brightness", "Increase (+10%)", "Decrease (-10%)"];

pub struct DeckControlAction {
    surface: SlotSurface,
    mode_idx: usize,
    dial_mode_idx: usize, // 0 = Brightness Slider, 1 = Page Switcher, 2 = Profile Switcher
    target_page: usize,
    target_profile: String,
    brightness_op: usize,
    brightness_pct: u8,
    accent_color: String,
    // Live mirrored deck state for rich 200x100 dial card rendering
    live_brightness: u8,
    live_page_idx: usize,
    live_total_pages: usize,
    live_profile: String,
    live_profiles: Vec<String>,
    last_rotate_step: Option<Instant>,
}

impl DeckControlAction {
    pub fn new() -> Self {
        Self {
            surface: SlotSurface::Key,
            mode_idx: 0,
            dial_mode_idx: 0,
            target_page: 1,
            target_profile: String::new(),
            brightness_op: 0,
            brightness_pct: 80,
            accent_color: "#9146ff".to_string(),
            live_brightness: 80,
            live_page_idx: 0,
            live_total_pages: 1,
            live_profile: "Default Profile".to_string(),
            live_profiles: Vec::new(),
            last_rotate_step: None,
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.surface = ctx.surface;
        self.mode_idx = ctx.get_usize("mode", 0).min(DECK_MODES.len() - 1);
        self.dial_mode_idx = ctx
            .get_usize("dial_mode", 0)
            .min(DIAL_DECK_MODES.len() - 1);
        self.target_page = ctx.get_f64("target_page", 1.0).round().clamp(1.0, 10.0) as usize;
        self.target_profile = ctx.get_str("target_profile", "");
        self.brightness_op = ctx
            .get_usize("brightness_op", 0)
            .min(BRIGHTNESS_OPS.len() - 1);
        self.brightness_pct = ctx.get_f64("brightness_pct", 80.0).round().clamp(0.0, 100.0) as u8;
        self.accent_color = ctx.get_str("accent_color", "#9146ff");

        self.live_brightness = ctx.current_brightness;
        self.live_page_idx = ctx.current_page_idx;
        self.live_total_pages = ctx.total_pages.max(1);
        self.live_profile = ctx.active_profile_name.to_string();
        self.live_profiles = ctx.available_profiles.to_vec();
    }

    fn render_dial_card(&self, cr: &cairo::Context, w: f64, h: f64) {
        let (ar, ag, ab) = parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0));

        let pad_x = 12.0;

        match self.dial_mode_idx {
            1 => {
                // LIVE PAGE SWITCHER CARD
                draw_cairo_text(
                    cr,
                    "DECK PAGE",
                    pad_x,
                    h * 0.10,
                    (h * 0.115).clamp(8.5, 11.5),
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Left,
                    w * 0.5,
                );
                draw_cairo_text(
                    cr,
                    &format!("{} / {}", self.live_page_idx + 1, self.live_total_pages),
                    w * 0.5,
                    h * 0.09,
                    (h * 0.13).clamp(9.0, 13.0),
                    (0.86, 0.86, 0.94),
                    true,
                    pango::Alignment::Right,
                    w * 0.5 - pad_x,
                );

                let num_sz = (h * 0.25).clamp(14.0, 26.0);
                draw_cairo_text(
                    cr,
                    &format!("Page {}", self.live_page_idx + 1),
                    pad_x,
                    h * 0.44 - num_sz * 0.54,
                    num_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    w - pad_x * 2.0,
                );

                // Visual page dot strip
                let count = self.live_total_pages.clamp(1, 10);
                let gap = ((w - 40.0) / count.max(2) as f64).clamp(10.0, 18.0);
                let total_w = (count - 1) as f64 * gap;
                let start_x = (w - total_w) / 2.0;
                let dot_y = h * 0.77;
                for i in 0..count {
                    let dx = start_x + i as f64 * gap;
                    if i == self.live_page_idx {
                        draw_pill(cr, dx - 7.0, dot_y - 3.5, 14.0, 7.0, 3.5);
                        cr.set_source_rgba(ar, ag, ab, 0.98);
                        let _ = cr.fill();
                    } else {
                        cr.set_source_rgba(1.0, 1.0, 1.0, 0.26);
                        cr.arc(dx, dot_y, 3.2, 0.0, 2.0 * std::f64::consts::PI);
                        let _ = cr.fill();
                    }
                }
            }
            2 => {
                // LIVE PROFILE SWITCHER CARD
                draw_cairo_text(
                    cr,
                    "PROFILE",
                    pad_x,
                    h * 0.11,
                    (h * 0.115).clamp(8.5, 11.5),
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Center,
                    w - pad_x * 2.0,
                );

                // Carousel pill with left/right arrows
                let pill_y = h * 0.34;
                let pill_h = h * 0.36;
                draw_pill(cr, pad_x, pill_y, w - pad_x * 2.0, pill_h, 9.0);
                cr.set_source_rgba(ar, ag, ab, 0.16);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(ar, ag, ab, 0.65);
                cr.set_line_width(1.3);
                let _ = cr.stroke();

                let p_sz = (h * 0.155).clamp(10.0, 16.0);
                draw_cairo_text(
                    cr,
                    "◀",
                    pad_x + 6.0,
                    pill_y + (pill_h - p_sz) * 0.35,
                    p_sz * 0.85,
                    (0.75, 0.75, 0.88),
                    true,
                    pango::Alignment::Left,
                    18.0,
                );
                draw_cairo_text(
                    cr,
                    &self.live_profile,
                    pad_x + 22.0,
                    pill_y + (pill_h - p_sz) * 0.34,
                    p_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    (w - pad_x * 2.0 - 44.0).max(20.0),
                );
                draw_cairo_text(
                    cr,
                    "▶",
                    w - pad_x - 24.0,
                    pill_y + (pill_h - p_sz) * 0.35,
                    p_sz * 0.85,
                    (0.75, 0.75, 0.88),
                    true,
                    pango::Alignment::Right,
                    18.0,
                );

                draw_cairo_text(
                    cr,
                    "Rotate to switch profile",
                    pad_x,
                    h * 0.77,
                    (h * 0.10).clamp(7.5, 10.5),
                    (0.60, 0.60, 0.72),
                    false,
                    pango::Alignment::Center,
                    w - pad_x * 2.0,
                );
            }
            _ => {
                // LIVE BRIGHTNESS SLIDER CARD (Default Dial Mode)
                let pct = self.live_brightness.min(100);
                let is_sleep = pct == 0;

                draw_cairo_text(
                    cr,
                    if is_sleep { "☾  SLEEPING" } else { "☀  BRIGHTNESS" },
                    pad_x,
                    h * 0.12,
                    (h * 0.125).clamp(9.0, 13.0),
                    if is_sleep { (0.65, 0.65, 0.78) } else { (ar, ag, ab) },
                    true,
                    pango::Alignment::Left,
                    w * 0.68,
                );
                draw_cairo_text(
                    cr,
                    &format!("{}%", pct),
                    w * 0.60,
                    h * 0.10,
                    (h * 0.175).clamp(11.0, 18.0),
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Right,
                    w * 0.40 - pad_x,
                );

                // Horizontal slider track + glowing active bar + circular thumb
                let bar_w = (w - pad_x * 2.0).max(24.0);
                let bar_h = (h * 0.13).clamp(8.0, 14.0);
                let bar_y = h * 0.52 - bar_h / 2.0;
                let r = bar_h / 2.0;

                draw_pill(cr, pad_x, bar_y, bar_w, bar_h, r);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
                let _ = cr.fill();

                let frac = pct as f64 / 100.0;
                if frac > 0.0 {
                    let fill_w = (bar_w * frac).max(bar_h).min(bar_w);
                    draw_pill(cr, pad_x, bar_y, fill_w, bar_h, r);
                    let bar_grad = cairo::LinearGradient::new(pad_x, 0.0, pad_x + bar_w, 0.0);
                    bar_grad.add_color_stop_rgba(0.0, ar, ag, ab, 0.85);
                    bar_grad.add_color_stop_rgba(1.0, (ar + 0.18).min(1.0), (ag + 0.18).min(1.0), 1.0, 0.98);
                    let _ = cr.set_source(&bar_grad);
                    let _ = cr.fill();

                    // Tactile white knob thumb
                    let thumb_x = pad_x + fill_w - r.min(fill_w / 2.0);
                    let thumb_y = bar_y + bar_h / 2.0;
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    cr.arc(thumb_x, thumb_y, bar_h * 0.64, 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.fill();
                }

                draw_cairo_text(
                    cr,
                    "Press dial to Sleep / Wake",
                    pad_x,
                    h * 0.74,
                    (h * 0.105).clamp(7.5, 10.5),
                    (0.62, 0.62, 0.74),
                    false,
                    pango::Alignment::Center,
                    w - pad_x * 2.0,
                );
            }
        }
    }
}

impl BallistaAction for DeckControlAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        match self.mode_idx {
            0 => ctx.push(HostCommand::SwitchPage(PageNav::Next)),
            1 => ctx.push(HostCommand::SwitchPage(PageNav::Prev)),
            2 => ctx.push(HostCommand::SwitchPage(PageNav::Index(
                self.target_page.saturating_sub(1),
            ))),
            3 => {
                if !self.target_profile.is_empty() {
                    ctx.push(HostCommand::SwitchProfile(self.target_profile.clone()));
                }
            }
            4 => match self.brightness_op {
                1 => ctx.push(HostCommand::SetBrightness(BrightnessOp::Delta(10))),
                2 => ctx.push(HostCommand::SetBrightness(BrightnessOp::Delta(-10))),
                _ => ctx.push(HostCommand::SetBrightness(BrightnessOp::Set(
                    self.brightness_pct,
                ))),
            },
            _ => ctx.push(HostCommand::ToggleSleep),
        }
    }

    fn on_dial_rotate(&mut self, ctx: &mut ActionContext, delta: i32) {
        if delta == 0 {
            return;
        }
        match self.dial_mode_idx {
            0 => {
                // Smooth brightness slider (±5% per detent)
                let step = (delta * 5) as i16;
                self.live_brightness = (self.live_brightness as i16 + step).clamp(0, 100) as u8;
                ctx.push(HostCommand::SetBrightness(BrightnessOp::Delta(step)));
                ctx.mark_dirty();
            }
            1 => {
                // Debounced page flip
                let now = Instant::now();
                if let Some(last) = self.last_rotate_step {
                    if now.duration_since(last) < Duration::from_millis(180) {
                        return;
                    }
                }
                self.last_rotate_step = Some(now);
                if delta > 0 {
                    ctx.push(HostCommand::SwitchPage(PageNav::Next));
                } else {
                    ctx.push(HostCommand::SwitchPage(PageNav::Prev));
                }
            }
            _ => {
                // Debounced profile carousel
                let now = Instant::now();
                if let Some(last) = self.last_rotate_step {
                    if now.duration_since(last) < Duration::from_millis(240) {
                        return;
                    }
                }
                self.last_rotate_step = Some(now);
                if ctx.available_profiles.len() > 1 {
                    let cur = ctx
                        .available_profiles
                        .iter()
                        .position(|p| p == ctx.active_profile_name)
                        .unwrap_or(0);
                    let n = ctx.available_profiles.len() as i32;
                    let next = (cur as i32 + if delta > 0 { 1 } else { -1 }).rem_euclid(n) as usize;
                    if let Some(name) = ctx.available_profiles.get(next) {
                        ctx.push(HostCommand::SwitchProfile(name.clone()));
                    }
                }
            }
        }
    }

    fn on_dial_press(&mut self, ctx: &mut ActionContext) {
        match self.dial_mode_idx {
            0 => {
                ctx.push(HostCommand::ToggleSleep);
                ctx.mark_dirty();
            }
            1 => {
                ctx.push(HostCommand::SwitchPage(PageNav::Index(0)));
            }
            _ => {
                if let Some(first) = ctx.available_profiles.first() {
                    ctx.push(HostCommand::SwitchProfile(first.clone()));
                }
            }
        }
    }

    fn tick_interval(&self) -> Option<Duration> {
        if self.surface == SlotSurface::Dial {
            Some(Duration::from_secs(1))
        } else {
            None
        }
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        let changed = self.live_brightness != ctx.current_brightness
            || self.live_page_idx != ctx.current_page_idx
            || self.live_total_pages != ctx.total_pages
            || self.live_profile != ctx.active_profile_name;
        if changed {
            self.live_brightness = ctx.current_brightness;
            self.live_page_idx = ctx.current_page_idx;
            self.live_total_pages = ctx.total_pages.max(1);
            self.live_profile = ctx.active_profile_name.to_string();
            self.live_profiles = ctx.available_profiles.to_vec();
            ctx.mark_dirty();
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        if ctx.surface == SlotSurface::Dial {
            let d_mode = ctx
                .get_usize("dial_mode", 0)
                .min(DIAL_DECK_MODES.len() - 1);

            let mut r0 = make_row(0, "ComboRow", "Dial Function");
            r0.options = DIAL_DECK_MODES.iter().map(|s| s.to_string()).collect();
            r0.selected = d_mode;

            let mut r1 = make_row(1, "ColorRow", "Accent Color");
            r1.color = ctx.get_str("accent_color", "#9146ff");

            let mut r2 = make_row(2, "ActionRow", "Dial Controls");
            r2.subtitle = match d_mode {
                0 => "Rotate: Dim / Brighten (±5%) · Press: Sleep / Wake".into(),
                1 => "Rotate: Next / Previous Page · Press: Jump to Page 1".into(),
                _ => "Rotate: Cycle Profiles · Press: Default Profile".into(),
            };

            return vec![r0, r1, r2];
        }

        // Square Key Settings
        let mode = ctx.get_usize("mode", 0).min(DECK_MODES.len() - 1);
        let b_op = ctx
            .get_usize("brightness_op", 0)
            .min(BRIGHTNESS_OPS.len() - 1);

        let mut r0 = make_row(0, "ComboRow", "Deck Action");
        r0.options = DECK_MODES.iter().map(|s| s.to_string()).collect();
        r0.selected = mode;

        let mut r1 = make_row(1, "SpinRow", "Page Number");
        r1.min = 1.0;
        r1.max = ctx.total_pages.max(10) as f64;
        r1.step = 1.0;
        r1.value = ctx.get_f64("target_page", 1.0);
        r1.visible = mode == 2;

        let mut r2 = make_row(2, "ComboRow", "Target Profile");
        r2.options = if ctx.available_profiles.is_empty() {
            vec!["Default Profile".to_string()]
        } else {
            ctx.available_profiles.to_vec()
        };
        let saved_prof = ctx.get_str("target_profile", "");
        r2.selected = r2
            .options
            .iter()
            .position(|p| p == &saved_prof)
            .unwrap_or(0);
        r2.visible = mode == 3;

        let mut r3 = make_row(3, "ComboRow", "Brightness Mode");
        r3.options = BRIGHTNESS_OPS.iter().map(|s| s.to_string()).collect();
        r3.selected = b_op;
        r3.visible = mode == 4;

        let mut r4 = make_row(4, "SpinRow", "Brightness (%)");
        r4.min = 0.0;
        r4.max = 100.0;
        r4.step = 5.0;
        r4.value = ctx.get_f64("brightness_pct", 80.0);
        r4.visible = mode == 4 && b_op == 0;

        vec![r0, r1, r2, r3, r4]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        if ctx.surface == SlotSurface::Dial {
            match row_idx {
                0 => {
                    if let Some(s) = change.selected {
                        ctx.set_setting("dial_mode", json!(s));
                        ctx.push(HostCommand::RefreshConfigRows);
                    }
                }
                1 => {
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
                    if let Some(v) = change.value {
                        ctx.set_setting("target_page", json!(v.round() as usize));
                    }
                }
                2 => {
                    if let Some(s) = change.selected {
                        if let Some(name) = ctx.available_profiles.get(s).cloned() {
                            ctx.set_setting("target_profile", json!(name));
                        }
                    }
                }
                3 => {
                    if let Some(s) = change.selected {
                        ctx.set_setting("brightness_op", json!(s));
                        ctx.push(HostCommand::RefreshConfigRows);
                    }
                }
                4 => {
                    if let Some(v) = change.value {
                        ctx.set_setting("brightness_pct", json!(v.round() as u8));
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
            self.render_dial_card(cr, w, h);
            return true;
        }

        let min_d = w.min(h).max(24.0);

        let (glyph, label) = match self.mode_idx {
            0 => ("▶", "Next Page".to_string()),
            1 => ("◀", "Prev Page".to_string()),
            2 => ("#", format!("Page {}", self.target_page)),
            3 => (
                "⇄",
                if self.target_profile.is_empty() {
                    "Profile".into()
                } else {
                    self.target_profile.clone()
                },
            ),
            4 => match self.brightness_op {
                1 => ("☀+", "Bright +".into()),
                2 => ("☀-", "Bright -".into()),
                _ => ("☀", format!("{}%", self.brightness_pct)),
            },
            _ => ("☾", "Sleep".into()),
        };

        let g_sz = (min_d * 0.30).clamp(13.0, 34.0);
        draw_cairo_text(
            cr,
            glyph,
            4.0,
            h * 0.36 - g_sz * 0.56,
            g_sz,
            (0.659, 0.400, 1.0),
            true,
            pango::Alignment::Center,
            w - 8.0,
        );

        let l_sz = (min_d * 0.13).clamp(8.5, 14.0);
        draw_cairo_text(
            cr,
            &label,
            4.0,
            h - l_sz * 1.75,
            l_sz,
            (0.86, 0.86, 0.92),
            true,
            pango::Alignment::Center,
            w - 8.0,
        );
        true
    }
}
