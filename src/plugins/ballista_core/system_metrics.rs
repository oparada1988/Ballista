use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand, SlotSurface,
};
use pangocairo::functions as pc;
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::time::{Duration, Instant};

const METRIC_OPTIONS: &[&str] = &["CPU Usage", "RAM Usage", "CPU Temperature", "Disk Usage (/)"];
const STYLE_OPTIONS: &[&str] = &["Percentage", "Live Graph", "Radial Gauge"];
const REFRESH_OPTIONS: &[&str] = &["0.5s", "1s", "2s", "5s"];
const MAX_HISTORY: usize = 64;

pub struct SystemMetricsAction {
    surface: SlotSurface,
    metric_idx: usize,
    style_idx: usize,
    refresh_idx: usize,
    auto_heat: bool,
    accent_color: String,
    prev_cpu_total: u64,
    prev_cpu_idle: u64,
    current_pct: f64,
    display_value: String,
    sub_label: String,
    detail_label: String,
    history: VecDeque<f64>,
    last_rotate_switch: Option<Instant>,
}

impl SystemMetricsAction {
    pub fn new() -> Self {
        Self {
            surface: SlotSurface::Key,
            metric_idx: 0,
            style_idx: 0,
            refresh_idx: 1,
            auto_heat: true,
            accent_color: "#9146ff".to_string(),
            prev_cpu_total: 0,
            prev_cpu_idle: 0,
            current_pct: 0.0,
            display_value: "--%".to_string(),
            sub_label: "CPU".to_string(),
            detail_label: String::new(),
            history: VecDeque::with_capacity(MAX_HISTORY),
            last_rotate_switch: None,
        }
    }

    fn default_style_for(surface: SlotSurface) -> usize {
        if surface == SlotSurface::Dial {
            1 // Live Graph shines by default on 200x100 widescreen dials!
        } else {
            0 // Percentage by default on square keys
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.surface = ctx.surface;
        let prev_metric = self.metric_idx;
        self.metric_idx = ctx.get_usize("metric", 0).min(METRIC_OPTIONS.len() - 1);
        self.style_idx = ctx
            .get_usize("style", Self::default_style_for(ctx.surface))
            .min(STYLE_OPTIONS.len() - 1);
        self.refresh_idx = ctx.get_usize("refresh", 1).min(REFRESH_OPTIONS.len() - 1);
        self.auto_heat = ctx.get_bool("auto_heat", true);
        self.accent_color = ctx.get_str("accent_color", "#9146ff");
        if prev_metric != self.metric_idx {
            self.history.clear();
        }
    }

    fn sample(&mut self) {
        match self.metric_idx {
            0 => {
                // CPU Usage from /proc/stat + secondary CPU temp readout for Dial card
                self.sub_label = "CPU".to_string();
                if let Some((total, idle)) = read_proc_cpu() {
                    if self.prev_cpu_total > 0 && total > self.prev_cpu_total {
                        let dt = (total - self.prev_cpu_total) as f64;
                        let di = idle.saturating_sub(self.prev_cpu_idle) as f64;
                        let usage = ((1.0 - di / dt) * 100.0).clamp(0.0, 100.0);
                        self.current_pct = usage;
                        self.display_value = format!("{}%", usage.round() as u32);
                    }
                    self.prev_cpu_total = total;
                    self.prev_cpu_idle = idle;
                }
                self.detail_label = read_cpu_temp_c()
                    .map(|c| format!("{}°C", c.round() as i32))
                    .unwrap_or_default();
            }
            1 => {
                // RAM Usage from /proc/meminfo
                self.sub_label = "RAM".to_string();
                if let Some((used_gb, total_gb, pct)) = read_proc_mem() {
                    self.current_pct = pct;
                    self.display_value = format!("{}%", pct.round() as u32);
                    if total_gb > 0.0 {
                        self.sub_label = format!("RAM {:.1}G", used_gb);
                        self.detail_label = format!("{:.1} / {:.0} GB", used_gb, total_gb);
                    }
                }
            }
            2 => {
                // CPU Temperature
                self.sub_label = "TEMP".to_string();
                if let Some(temp_c) = read_cpu_temp_c() {
                    self.current_pct = ((temp_c - 25.0) / 70.0 * 100.0).clamp(0.0, 100.0);
                    self.display_value = format!("{}°C", temp_c.round() as i32);
                    self.detail_label = "CPU Package".to_string();
                } else {
                    self.display_value = "N/A".to_string();
                    self.detail_label = "No Sensor".to_string();
                }
            }
            _ => {
                // Root Disk Usage via libc::statvfs
                self.sub_label = "DISK".to_string();
                if let Some((used_gb, total_gb, pct)) = read_disk_usage("/") {
                    self.current_pct = pct;
                    self.display_value = format!("{}%", pct.round() as u32);
                    self.detail_label = format!("{:.0} / {:.0} GB", used_gb, total_gb);
                }
            }
        }

        if self.history.len() >= MAX_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.current_pct);
    }

    fn active_rgb(&self) -> (f64, f64, f64) {
        if self.auto_heat {
            if self.current_pct >= 85.0 {
                (0.937, 0.267, 0.267) // #ef4444 red
            } else if self.current_pct >= 60.0 {
                (0.961, 0.620, 0.043) // #f59e0b amber
            } else {
                (0.133, 0.773, 0.369) // #22c55e green
            }
        } else {
            parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0))
        }
    }

    fn render_dial_card(&self, cr: &cairo::Context, w: f64, h: f64) {
        let (ar, ag, ab) = self.active_rgb();
        let metric_short = match self.metric_idx {
            0 => "CPU",
            1 => "RAM",
            2 => "TEMP",
            _ => "DISK",
        };

        let pad_x = 10.0;
        let dot_y = h - 7.5;

        match self.style_idx {
            2 => {
                // Widescreen Radial Gauge Left + Rich Telemetry Right
                let r = (h * 0.34).clamp(16.0, 36.0);
                let cx = pad_x + r + 8.0;
                let cy = h * 0.48;
                let lw = (h * 0.068).clamp(4.0, 7.0);
                let start_a = 135.0_f64.to_radians();
                let sweep = 270.0_f64.to_radians();

                cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
                cr.set_line_width(lw);
                cr.set_line_cap(cairo::LineCap::Round);
                cr.arc(cx, cy, r, start_a, start_a + sweep);
                let _ = cr.stroke();

                let frac = (self.current_pct / 100.0).clamp(0.01, 1.0);
                cr.set_source_rgba(ar, ag, ab, 0.98);
                cr.arc(cx, cy, r, start_a, start_a + sweep * frac);
                let _ = cr.stroke();

                let g_sz = (h * 0.18).clamp(10.0, 18.0);
                draw_cairo_text(
                    cr,
                    &self.display_value,
                    cx - r,
                    cy - g_sz * 0.62,
                    g_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    r * 2.0,
                );

                let rx = cx + r + 14.0;
                let rw = (w - rx - pad_x).max(30.0);
                draw_cairo_text(
                    cr,
                    metric_short,
                    rx,
                    h * 0.16,
                    (h * 0.15).clamp(10.0, 15.0),
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Left,
                    rw,
                );
                if !self.detail_label.is_empty() {
                    draw_cairo_text(
                        cr,
                        &self.detail_label,
                        rx,
                        h * 0.42,
                        (h * 0.135).clamp(9.0, 13.5),
                        (0.86, 0.86, 0.94),
                        false,
                        pango::Alignment::Left,
                        rw,
                    );
                }
                let peak = self
                    .history
                    .iter()
                    .copied()
                    .fold(0.0_f64, f64::max)
                    .round() as u32;
                draw_cairo_text(
                    cr,
                    &format!("Peak {}%", peak),
                    rx,
                    h * 0.65,
                    (h * 0.105).clamp(8.0, 11.0),
                    (0.62, 0.62, 0.72),
                    false,
                    pango::Alignment::Left,
                    rw,
                );
            }
            0 => {
                // Widescreen Percentage + Big Bar + Detail Telemetry
                draw_cairo_text(
                    cr,
                    metric_short,
                    pad_x,
                    h * 0.10,
                    (h * 0.145).clamp(10.0, 14.5),
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Left,
                    w * 0.40,
                );
                if !self.detail_label.is_empty() {
                    draw_cairo_text(
                        cr,
                        &self.detail_label,
                        w * 0.35,
                        h * 0.11,
                        (h * 0.125).clamp(9.0, 12.5),
                        (0.75, 0.75, 0.85),
                        false,
                        pango::Alignment::Right,
                        w * 0.65 - pad_x,
                    );
                }
                let val_sz = (h * 0.30).clamp(16.0, 30.0);
                draw_cairo_text(
                    cr,
                    &self.display_value,
                    pad_x,
                    h * 0.46 - val_sz * 0.58,
                    val_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    w - pad_x * 2.0,
                );

                let bar_h = (h * 0.085).clamp(5.0, 9.0);
                let bar_y = h - bar_h - 16.0;
                let bar_w = (w - pad_x * 2.0).max(20.0);
                let r = bar_h / 2.0;
                draw_pill(cr, pad_x, bar_y, bar_w, bar_h, r);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
                let _ = cr.fill();

                let fill_w = (bar_w * (self.current_pct / 100.0).clamp(0.0, 1.0)).max(bar_h);
                draw_pill(cr, pad_x, bar_y, fill_w.min(bar_w), bar_h, r);
                cr.set_source_rgba(ar, ag, ab, 0.98);
                let _ = cr.fill();
            }
            _ => {
                // Widescreen 64-Sample Live Sparkline Graph (Flagship Dial View)
                let header_h = (h * 0.28).max(22.0);
                let graph_y = header_h + 2.0;
                let graph_h = (h - graph_y - 14.0).max(18.0);
                let graph_w = (w - pad_x * 2.0).max(24.0);

                // Metric pill + secondary detail on left, big value on right
                draw_cairo_text(
                    cr,
                    metric_short,
                    pad_x,
                    h * 0.06,
                    (h * 0.145).clamp(10.0, 15.0),
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Left,
                    w * 0.28,
                );
                if !self.detail_label.is_empty() {
                    draw_cairo_text(
                        cr,
                        &self.detail_label,
                        pad_x + w * 0.24,
                        h * 0.075,
                        (h * 0.12).clamp(8.5, 12.0),
                        (0.72, 0.72, 0.82),
                        false,
                        pango::Alignment::Left,
                        w * 0.44,
                    );
                }
                draw_cairo_text(
                    cr,
                    &self.display_value,
                    w * 0.62,
                    h * 0.045,
                    (h * 0.18).clamp(11.0, 18.0),
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Right,
                    w * 0.38 - pad_x,
                );

                // Subtle reference gridlines
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
                cr.set_line_width(1.0);
                for frac in [0.25, 0.50, 0.75] {
                    let gy = graph_y + graph_h * frac;
                    cr.move_to(pad_x, gy);
                    cr.line_to(pad_x + graph_w, gy);
                    let _ = cr.stroke();
                }

                // 64-sample sparkline curve
                let n = self.history.len();
                if n >= 2 {
                    let step_x = graph_w / (MAX_HISTORY - 1) as f64;
                    let start_offset = MAX_HISTORY - n;

                    cr.new_path();
                    let first_x = pad_x + start_offset as f64 * step_x;
                    cr.move_to(first_x, graph_y + graph_h);
                    for (i, &val) in self.history.iter().enumerate() {
                        let px = pad_x + (start_offset + i) as f64 * step_x;
                        let py = graph_y + graph_h * (1.0 - (val / 100.0).clamp(0.0, 1.0));
                        cr.line_to(px, py);
                    }
                    let last_x = pad_x + (start_offset + n - 1) as f64 * step_x;
                    cr.line_to(last_x, graph_y + graph_h);
                    cr.close_path();

                    let fill_grad = cairo::LinearGradient::new(0.0, graph_y, 0.0, graph_y + graph_h);
                    fill_grad.add_color_stop_rgba(0.0, ar, ag, ab, 0.46);
                    fill_grad.add_color_stop_rgba(1.0, ar, ag, ab, 0.04);
                    let _ = cr.set_source(&fill_grad);
                    let _ = cr.fill();

                    cr.new_path();
                    let mut tip_y = graph_y + graph_h;
                    for (i, &val) in self.history.iter().enumerate() {
                        let px = pad_x + (start_offset + i) as f64 * step_x;
                        let py = graph_y + graph_h * (1.0 - (val / 100.0).clamp(0.0, 1.0));
                        tip_y = py;
                        if i == 0 {
                            cr.move_to(px, py);
                        } else {
                            cr.line_to(px, py);
                        }
                    }
                    cr.set_source_rgba(ar, ag, ab, 0.98);
                    cr.set_line_width(2.0);
                    cr.set_line_join(cairo::LineJoin::Round);
                    let _ = cr.stroke();

                    // Live tip glow dot
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    cr.arc(last_x, tip_y, 2.5, 0.0, 2.0 * std::f64::consts::PI);
                    let _ = cr.fill();
                }
            }
        }

        // Bottom metric pager dots (rotate dial to switch CPU / RAM / TEMP / DISK)
        let dot_count = METRIC_OPTIONS.len();
        let dot_gap = 10.0;
        let total_dots_w = (dot_count - 1) as f64 * dot_gap;
        let start_dx = (w - total_dots_w) / 2.0;
        for i in 0..dot_count {
            let dx = start_dx + i as f64 * dot_gap;
            if i == self.metric_idx {
                cr.set_source_rgba(ar, ag, ab, 0.98);
                cr.arc(dx, dot_y, 2.7, 0.0, 2.0 * std::f64::consts::PI);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.26);
                cr.arc(dx, dot_y, 1.9, 0.0, 2.0 * std::f64::consts::PI);
            }
            let _ = cr.fill();
        }
    }
}

impl BallistaAction for SystemMetricsAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        if self.prev_cpu_total == 0 {
            if let Some((t, i)) = read_proc_cpu() {
                self.prev_cpu_total = t;
                self.prev_cpu_idle = i;
            }
        }
        self.sample();
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        self.style_idx = (self.style_idx + 1) % STYLE_OPTIONS.len();
        ctx.set_setting("style", json!(self.style_idx));
        ctx.push(HostCommand::RefreshConfigRows);
        ctx.mark_dirty();
    }

    fn on_dial_press(&mut self, ctx: &mut ActionContext) {
        self.on_key_down(ctx);
    }

    fn on_dial_rotate(&mut self, ctx: &mut ActionContext, delta: i32) {
        if delta == 0 {
            return;
        }
        // Debounce brisk knob flicks so each turn cleanly steps to the next metric (CPU -> RAM -> TEMP -> DISK)
        let now = Instant::now();
        if let Some(last) = self.last_rotate_switch {
            if now.duration_since(last) < Duration::from_millis(160) {
                return;
            }
        }
        self.last_rotate_switch = Some(now);
        let n = METRIC_OPTIONS.len() as i32;
        let next = (self.metric_idx as i32 + if delta > 0 { 1 } else { -1 }).rem_euclid(n) as usize;
        self.metric_idx = next;
        self.history.clear();
        ctx.set_setting("metric", json!(next));
        self.sample();
        ctx.push(HostCommand::RefreshConfigRows);
        ctx.mark_dirty();
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(match self.refresh_idx {
            0 => Duration::from_millis(500),
            1 => Duration::from_secs(1),
            2 => Duration::from_secs(2),
            _ => Duration::from_secs(5),
        })
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        self.sample();
        ctx.mark_dirty();
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let def_style = Self::default_style_for(ctx.surface);

        let mut r0 = make_row(0, "ComboRow", "Metric");
        r0.options = METRIC_OPTIONS.iter().map(|s| s.to_string()).collect();
        r0.selected = ctx.get_usize("metric", 0).min(r0.options.len() - 1);

        let mut r1 = make_row(1, "ComboRow", "Display Style");
        r1.options = STYLE_OPTIONS.iter().map(|s| s.to_string()).collect();
        r1.selected = ctx.get_usize("style", def_style).min(r1.options.len() - 1);

        let mut r2 = make_row(2, "ComboRow", "Refresh Interval");
        r2.options = REFRESH_OPTIONS.iter().map(|s| s.to_string()).collect();
        r2.selected = ctx.get_usize("refresh", 1).min(r2.options.len() - 1);

        let mut r3 = make_row(3, "SwitchRow", "Auto Heat Color");
        r3.active = ctx.get_bool("auto_heat", true);

        let mut r4 = make_row(4, "ColorRow", "Accent Color");
        r4.color = ctx.get_str("accent_color", "#9146ff");
        r4.visible = !r3.active;

        let mut rows = vec![r0, r1, r2, r3, r4];
        if ctx.surface == SlotSurface::Dial {
            let mut r5 = make_row(5, "ActionRow", "Dial Controls");
            r5.subtitle = "Rotate: Switch Metric (CPU / RAM / Temp / Disk) · Press: Cycle View".into();
            rows.push(r5);
        }
        rows
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        match row_idx {
            0 => {
                if let Some(sel) = change.selected {
                    ctx.set_setting("metric", json!(sel));
                }
            }
            1 => {
                if let Some(sel) = change.selected {
                    ctx.set_setting("style", json!(sel));
                }
            }
            2 => {
                if let Some(sel) = change.selected {
                    ctx.set_setting("refresh", json!(sel));
                }
            }
            3 => {
                if let Some(act) = change.active {
                    ctx.set_setting("auto_heat", json!(act));
                }
            }
            4 => {
                if let Some(col) = change.color {
                    ctx.set_setting("accent_color", json!(col));
                }
            }
            _ => {}
        }
        self.sync_from_settings(ctx);
        self.sample();
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

        let (ar, ag, ab) = self.active_rgb();
        let min_d = w.min(h).max(24.0);

        let metric_short = match self.metric_idx {
            0 => "CPU",
            1 => "RAM",
            2 => "TEMP",
            _ => "DISK",
        };

        match self.style_idx {
            1 => {
                // LIVE SPARKLINE GRAPH (last 32 samples on square key)
                let pad_x = (w * 0.07).max(5.0);
                let top_h = (h * 0.28).max(18.0);
                let graph_y = top_h + 2.0;
                let graph_h = (h - graph_y - (h * 0.09).max(5.0)).max(16.0);
                let graph_w = (w - pad_x * 2.0).max(16.0);

                cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
                cr.set_line_width(1.0);
                for frac in [0.25, 0.5, 0.75] {
                    let gy = graph_y + graph_h * frac;
                    cr.move_to(pad_x, gy);
                    cr.line_to(pad_x + graph_w, gy);
                    let _ = cr.stroke();
                }

                let skip = self.history.len().saturating_sub(32);
                let n = self.history.len() - skip;
                if n >= 2 {
                    let step_x = graph_w / 31.0;
                    let start_offset = 32 - n;

                    cr.new_path();
                    let first_x = pad_x + start_offset as f64 * step_x;
                    cr.move_to(first_x, graph_y + graph_h);
                    for (i, &val) in self.history.iter().skip(skip).enumerate() {
                        let px = pad_x + (start_offset + i) as f64 * step_x;
                        let py = graph_y + graph_h * (1.0 - (val / 100.0).clamp(0.0, 1.0));
                        cr.line_to(px, py);
                    }
                    let last_x = pad_x + (start_offset + n - 1) as f64 * step_x;
                    cr.line_to(last_x, graph_y + graph_h);
                    cr.close_path();

                    let fill_grad = cairo::LinearGradient::new(0.0, graph_y, 0.0, graph_y + graph_h);
                    fill_grad.add_color_stop_rgba(0.0, ar, ag, ab, 0.48);
                    fill_grad.add_color_stop_rgba(1.0, ar, ag, ab, 0.04);
                    let _ = cr.set_source(&fill_grad);
                    let _ = cr.fill();

                    cr.new_path();
                    for (i, &val) in self.history.iter().skip(skip).enumerate() {
                        let px = pad_x + (start_offset + i) as f64 * step_x;
                        let py = graph_y + graph_h * (1.0 - (val / 100.0).clamp(0.0, 1.0));
                        if i == 0 {
                            cr.move_to(px, py);
                        } else {
                            cr.line_to(px, py);
                        }
                    }
                    cr.set_source_rgba(ar, ag, ab, 0.98);
                    cr.set_line_width((min_d * 0.025).clamp(1.6, 2.8));
                    cr.set_line_join(cairo::LineJoin::Round);
                    let _ = cr.stroke();
                }

                draw_cairo_text(
                    cr,
                    metric_short,
                    pad_x,
                    (h * 0.07).max(4.0),
                    (min_d * 0.145).clamp(9.0, 16.0),
                    (0.76, 0.76, 0.84),
                    false,
                    pango::Alignment::Left,
                    w * 0.45,
                );
                draw_cairo_text(
                    cr,
                    &self.display_value,
                    w - pad_x - w * 0.52,
                    (h * 0.06).max(3.0),
                    (min_d * 0.165).clamp(10.0, 18.0),
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Right,
                    w * 0.52,
                );
            }
            2 => {
                // RADIAL GAUGE
                let cx = w / 2.0;
                let cy = h * 0.47;
                let radius = (min_d * 0.34).max(12.0);
                let lw = (min_d * 0.075).clamp(4.0, 9.0);
                let start_a = 135.0_f64.to_radians();
                let sweep = 270.0_f64.to_radians();

                cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
                cr.set_line_width(lw);
                cr.set_line_cap(cairo::LineCap::Round);
                cr.arc(cx, cy, radius, start_a, start_a + sweep);
                let _ = cr.stroke();

                let frac = (self.current_pct / 100.0).clamp(0.01, 1.0);
                cr.set_source_rgba(ar, ag, ab, 0.96);
                cr.set_line_width(lw);
                cr.set_line_cap(cairo::LineCap::Round);
                cr.arc(cx, cy, radius, start_a, start_a + sweep * frac);
                let _ = cr.stroke();

                let val_sz = (min_d * 0.21).clamp(10.0, 24.0);
                draw_cairo_text(
                    cr,
                    &self.display_value,
                    4.0,
                    cy - val_sz * 0.62,
                    val_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    w - 8.0,
                );

                let tag_sz = (min_d * 0.125).clamp(8.0, 13.0);
                draw_cairo_text(
                    cr,
                    metric_short,
                    4.0,
                    h - tag_sz * 1.65,
                    tag_sz,
                    (0.75, 0.75, 0.85),
                    true,
                    pango::Alignment::Center,
                    w - 8.0,
                );
            }
            _ => {
                // PERCENTAGE + SLEEK PROGRESS BAR
                let pad_x = (w * 0.10).max(6.0);
                let tag_sz = (min_d * 0.145).clamp(9.0, 15.0);
                draw_cairo_text(
                    cr,
                    &self.sub_label,
                    4.0,
                    (h * 0.10).max(5.0),
                    tag_sz,
                    (ar, ag, ab),
                    true,
                    pango::Alignment::Center,
                    w - 8.0,
                );

                let val_sz = (min_d * 0.28).clamp(13.0, 32.0);
                draw_cairo_text(
                    cr,
                    &self.display_value,
                    4.0,
                    (h * 0.50) - val_sz * 0.62,
                    val_sz,
                    (1.0, 1.0, 1.0),
                    true,
                    pango::Alignment::Center,
                    w - 8.0,
                );

                let bar_h = (min_d * 0.085).clamp(4.0, 9.0);
                let bar_y = h - bar_h - (h * 0.12).max(6.0);
                let bar_w = (w - pad_x * 2.0).max(16.0);
                let r = bar_h / 2.0;

                draw_pill(cr, pad_x, bar_y, bar_w, bar_h, r);
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
                let _ = cr.fill();

                let fill_w = (bar_w * (self.current_pct / 100.0).clamp(0.0, 1.0)).max(bar_h);
                draw_pill(cr, pad_x, bar_y, fill_w.min(bar_w), bar_h, r);
                cr.set_source_rgba(ar, ag, ab, 0.96);
                let _ = cr.fill();
            }
        }

        true
    }
}

pub fn draw_pill(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

pub fn draw_cairo_text(
    cr: &cairo::Context,
    text: &str,
    x: f64,
    y: f64,
    px_size: f64,
    rgb: (f64, f64, f64),
    bold: bool,
    align: pango::Alignment,
    width: f64,
) {
    let layout = pc::create_layout(cr);
    let mut fd = pango::FontDescription::new();
    fd.set_family("Cantarell");
    fd.set_absolute_size(px_size * pango::SCALE as f64);
    fd.set_weight(if bold {
        pango::Weight::Bold
    } else {
        pango::Weight::Semibold
    });
    layout.set_font_description(Some(&fd));
    layout.set_single_paragraph_mode(true);
    layout.set_ellipsize(pango::EllipsizeMode::End);
    layout.set_alignment(align);
    layout.set_width((width.max(8.0) as i32) * pango::SCALE);
    layout.set_text(text);

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    cr.move_to(x + 1.0, y + 1.0);
    pc::show_layout(cr, &layout);

    cr.set_source_rgb(rgb.0, rgb.1, rgb.2);
    cr.move_to(x, y);
    pc::show_layout(cr, &layout);
}

pub fn parse_hex(hex: &str) -> Option<(f64, f64, f64)> {
    let s = hex.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f64 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f64 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f64 / 255.0;
        return Some((r, g, b));
    }
    None
}

fn read_proc_cpu() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    let line = content.lines().next()?;
    let mut parts = line.split_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    let nums: Vec<u64> = parts.filter_map(|p| p.parse::<u64>().ok()).collect();
    if nums.len() < 4 {
        return None;
    }
    let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
    let total: u64 = nums.iter().sum();
    Some((total, idle))
}

fn read_proc_mem() -> Option<(f64, f64, f64)> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb: u64 = 0;
    let mut avail_kb: u64 = 0;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest.split_whitespace().next()?.parse().ok()?;
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kb = rest.split_whitespace().next()?.parse().ok()?;
        }
    }
    if total_kb == 0 {
        return None;
    }
    let used_kb = total_kb.saturating_sub(avail_kb);
    let pct = (used_kb as f64 / total_kb as f64 * 100.0).clamp(0.0, 100.0);
    let used_gb = used_kb as f64 / (1024.0 * 1024.0);
    let total_gb = total_kb as f64 / (1024.0 * 1024.0);
    Some((used_gb, total_gb, pct))
}

fn read_cpu_temp_c() -> Option<f64> {
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        let mut fallback: Option<f64> = None;
        for entry in entries.flatten() {
            let dir = entry.path();
            let name = fs::read_to_string(dir.join("name"))
                .unwrap_or_default()
                .trim()
                .to_lowercase();
            let temp_path = dir.join("temp1_input");
            if let Ok(raw) = fs::read_to_string(&temp_path) {
                if let Ok(milli_c) = raw.trim().parse::<f64>() {
                    let c = milli_c / 1000.0;
                    if c > 5.0 && c < 130.0 {
                        if matches!(
                            name.as_str(),
                            "k10temp" | "coretemp" | "zenpower" | "cpu_thermal"
                        ) {
                            return Some(c);
                        }
                        if fallback.is_none() {
                            fallback = Some(c);
                        }
                    }
                }
            }
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    if let Ok(raw) = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp") {
        if let Ok(milli_c) = raw.trim().parse::<f64>() {
            let c = milli_c / 1000.0;
            if c > 5.0 && c < 130.0 {
                return Some(c);
            }
        }
    }
    None
}

fn read_disk_usage(path: &str) -> Option<(f64, f64, f64)> {
    let c_path = std::ffi::CString::new(path).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) };
    if rc != 0 || stat.f_blocks == 0 {
        return None;
    }
    let frsize = stat.f_frsize as f64;
    let total_bytes = stat.f_blocks as f64 * frsize;
    let avail_bytes = stat.f_bavail as f64 * frsize;
    let used_bytes = (total_bytes - avail_bytes).max(0.0);
    let pct = (used_bytes / total_bytes * 100.0).clamp(0.0, 100.0);
    let gb = 1024.0 * 1024.0 * 1024.0;
    Some((used_bytes / gb, total_bytes / gb, pct))
}
