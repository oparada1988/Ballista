use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand,
};
use crate::plugins::ballista_core::system_metrics::{draw_cairo_text, draw_pill, parse_hex};
use serde_json::json;
use std::process::Command;
use std::time::{Duration, Instant};

const TIMER_MODES: &[&str] = &["Countdown Timer", "Pomodoro (Focus / Break)"];
const STEP_OPTIONS: &[&str] = &["1 Minute", "30 Seconds", "5 Minutes"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimerState {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PomodoroPhase {
    Focus,
    Break,
}

/// Dial-Exclusive (`TargetCaps::DIALS_ONLY`) Showcase Action:
/// Rotate knob to dial in duration, press to Start / Pause, tap touchscreen zones to Reset or Switch Phase!
pub struct TimerDialAction {
    mode_idx: usize,       // 0 = Countdown Timer, 1 = Pomodoro
    step_idx: usize,       // 0 = 60s, 1 = 30s, 2 = 300s
    focus_mins: u32,       // default 25
    break_mins: u32,       // default 5
    notify_on_done: bool,
    accent_color: String,

    state: TimerState,
    phase: PomodoroPhase,
    target_secs: u32,      // total duration of current countdown
    remaining_secs: u32,
    last_tick_at: Option<Instant>,
    blink_on: bool,
}

impl TimerDialAction {
    pub fn new() -> Self {
        Self {
            mode_idx: 0,
            step_idx: 0,
            focus_mins: 25,
            break_mins: 5,
            notify_on_done: true,
            accent_color: "#9146ff".to_string(),
            state: TimerState::Idle,
            phase: PomodoroPhase::Focus,
            target_secs: 5 * 60,
            remaining_secs: 5 * 60,
            last_tick_at: None,
            blink_on: false,
        }
    }

    fn step_secs(&self) -> i32 {
        match self.step_idx {
            1 => 30,
            2 => 300,
            _ => 60,
        }
    }

    fn default_secs_for_mode(&self) -> u32 {
        if self.mode_idx == 1 {
            match self.phase {
                PomodoroPhase::Focus => self.focus_mins.max(1) * 60,
                PomodoroPhase::Break => self.break_mins.max(1) * 60,
            }
        } else {
            5 * 60
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        let prev_mode = self.mode_idx;
        self.mode_idx = ctx.get_usize("mode", 0).min(TIMER_MODES.len() - 1);
        self.step_idx = ctx.get_usize("step", 0).min(STEP_OPTIONS.len() - 1);
        self.focus_mins = ctx.get_f64("focus_mins", 25.0).round().clamp(1.0, 90.0) as u32;
        self.break_mins = ctx.get_f64("break_mins", 5.0).round().clamp(1.0, 45.0) as u32;
        self.notify_on_done = ctx.get_bool("notify", true);
        self.accent_color = ctx.get_str("accent_color", "#9146ff");

        let saved_target = ctx.get_f64("target_secs", 0.0).round() as u32;
        if prev_mode != self.mode_idx && self.state == TimerState::Idle {
            self.phase = PomodoroPhase::Focus;
            let secs = self.default_secs_for_mode();
            self.target_secs = secs;
            self.remaining_secs = secs;
        } else if self.state == TimerState::Idle && saved_target >= 30 {
            self.target_secs = saved_target.min(99 * 60);
            self.remaining_secs = self.target_secs;
        }
    }

    fn reset_timer(&mut self, ctx: &mut ActionContext) {
        self.state = TimerState::Idle;
        self.last_tick_at = None;
        self.blink_on = false;
        if self.mode_idx == 1 {
            self.target_secs = self.default_secs_for_mode();
        }
        self.remaining_secs = self.target_secs.max(30);
        ctx.mark_dirty();
    }

    fn toggle_start_pause(&mut self, ctx: &mut ActionContext) {
        match self.state {
            TimerState::Idle | TimerState::Paused => {
                if self.remaining_secs == 0 {
                    self.remaining_secs = self.target_secs.max(30);
                }
                self.state = TimerState::Running;
                self.last_tick_at = Some(Instant::now());
            }
            TimerState::Running => {
                self.state = TimerState::Paused;
                self.last_tick_at = None;
            }
            TimerState::Finished => {
                // In Pomodoro mode, pressing after completion advances cleanly to the next phase!
                if self.mode_idx == 1 {
                    self.phase = match self.phase {
                        PomodoroPhase::Focus => PomodoroPhase::Break,
                        PomodoroPhase::Break => PomodoroPhase::Focus,
                    };
                    self.target_secs = self.default_secs_for_mode();
                    self.remaining_secs = self.target_secs;
                    self.state = TimerState::Running;
                    self.last_tick_at = Some(Instant::now());
                } else {
                    self.reset_timer(ctx);
                    return;
                }
            }
        }
        ctx.mark_dirty();
    }

    fn active_palette(&self) -> (f64, f64, f64) {
        match self.state {
            TimerState::Finished => {
                if self.blink_on {
                    (0.133, 0.773, 0.369) // #22c55e green flash
                } else {
                    (0.961, 0.620, 0.043) // #f59e0b amber flash
                }
            }
            _ if self.mode_idx == 1 => match self.phase {
                PomodoroPhase::Focus => (0.941, 0.329, 0.435), // warm focus coral-rose
                PomodoroPhase::Break => (0.133, 0.773, 0.369), // calm break green
            },
            _ => parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0)),
        }
    }
}

impl BallistaAction for TimerDialAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_dial_rotate(&mut self, ctx: &mut ActionContext, delta: i32) {
        if delta == 0 {
            return;
        }
        let step = self.step_secs();
        let change = delta.clamp(-5, 5) * step;

        if self.state == TimerState::Finished {
            self.state = TimerState::Idle;
            self.blink_on = false;
        }

        let new_rem = (self.remaining_secs as i32 + change).clamp(30, 99 * 60) as u32;
        self.remaining_secs = new_rem;
        if self.state != TimerState::Running || new_rem > self.target_secs {
            self.target_secs = new_rem;
            if self.mode_idx == 0 {
                ctx.set_setting("target_secs", json!(self.target_secs));
            }
        }
        ctx.mark_dirty();
    }

    fn on_dial_press(&mut self, ctx: &mut ActionContext) {
        self.toggle_start_pause(ctx);
    }

    fn on_touch_tap(&mut self, ctx: &mut ActionContext, norm_x: f64, _norm_y: f64) {
        if norm_x < 0.30 {
            // Left tap: Reset
            self.reset_timer(ctx);
        } else if norm_x > 0.72 {
            // Right tap: Toggle Countdown <-> Pomodoro (or switch Focus/Break phase)
            if self.mode_idx == 1 {
                self.phase = match self.phase {
                    PomodoroPhase::Focus => PomodoroPhase::Break,
                    PomodoroPhase::Break => PomodoroPhase::Focus,
                };
                self.state = TimerState::Idle;
                self.last_tick_at = None;
                self.target_secs = self.default_secs_for_mode();
                self.remaining_secs = self.target_secs;
                ctx.mark_dirty();
            } else {
                self.mode_idx = 1;
                self.phase = PomodoroPhase::Focus;
                ctx.set_setting("mode", json!(1));
                self.reset_timer(ctx);
                ctx.push(HostCommand::RefreshConfigRows);
            }
        } else {
            // Center tap: Start / Pause
            self.toggle_start_pause(ctx);
        }
    }

    fn tick_interval(&self) -> Option<Duration> {
        match self.state {
            TimerState::Running => Some(Duration::from_secs(1)),
            TimerState::Finished => Some(Duration::from_millis(600)),
            _ => None,
        }
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        match self.state {
            TimerState::Running => {
                if self.remaining_secs > 1 {
                    self.remaining_secs -= 1;
                } else {
                    self.remaining_secs = 0;
                    self.state = TimerState::Finished;
                    self.blink_on = true;
                    if self.notify_on_done {
                        let msg = if self.mode_idx == 1 {
                            match self.phase {
                                PomodoroPhase::Focus => "Focus session complete! Time for a break.",
                                PomodoroPhase::Break => "Break complete! Ready to focus.",
                            }
                        } else {
                            "Countdown timer complete!"
                        };
                        std::thread::spawn(move || {
                            let _ = Command::new("notify-send")
                                .args(["-a", "Ballista", "Ballista Timer", msg])
                                .status();
                        });
                    }
                }
                ctx.mark_dirty();
            }
            TimerState::Finished => {
                self.blink_on = !self.blink_on;
                ctx.mark_dirty();
            }
            _ => {}
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let mode = ctx.get_usize("mode", 0).min(TIMER_MODES.len() - 1);

        let mut r0 = make_row(0, "ComboRow", "Timer Mode");
        r0.options = TIMER_MODES.iter().map(|s| s.to_string()).collect();
        r0.selected = mode;

        let mut r1 = make_row(1, "ComboRow", "Dial Step");
        r1.options = STEP_OPTIONS.iter().map(|s| s.to_string()).collect();
        r1.selected = ctx.get_usize("step", 0).min(r1.options.len() - 1);

        let mut r2 = make_row(2, "SpinRow", "Focus Duration (Min)");
        r2.min = 1.0;
        r2.max = 90.0;
        r2.step = 1.0;
        r2.value = ctx.get_f64("focus_mins", 25.0);
        r2.visible = mode == 1;

        let mut r3 = make_row(3, "SpinRow", "Break Duration (Min)");
        r3.min = 1.0;
        r3.max = 45.0;
        r3.step = 1.0;
        r3.value = ctx.get_f64("break_mins", 5.0);
        r3.visible = mode == 1;

        let mut r4 = make_row(4, "SwitchRow", "Desktop Notification");
        r4.active = ctx.get_bool("notify", true);

        let mut r5 = make_row(5, "ColorRow", "Accent Color");
        r5.color = ctx.get_str("accent_color", "#9146ff");
        r5.visible = mode == 0;

        let mut r6 = make_row(6, "ButtonRow", "Reset Timer");
        r6.button_label = "Reset".to_string();

        let mut r7 = make_row(7, "ActionRow", "Dial Controls");
        r7.subtitle = "Rotate: Adjust Time · Press: Start / Pause · Tap Left: Reset".into();

        vec![r0, r1, r2, r3, r4, r5, r6, r7]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        match row_idx {
            0 => {
                if let Some(s) = change.selected {
                    ctx.set_setting("mode", json!(s));
                    self.state = TimerState::Idle;
                    self.phase = PomodoroPhase::Focus;
                    ctx.push(HostCommand::RefreshConfigRows);
                }
            }
            1 => {
                if let Some(s) = change.selected {
                    ctx.set_setting("step", json!(s));
                }
            }
            2 => {
                if let Some(v) = change.value {
                    ctx.set_setting("focus_mins", json!(v.round() as u32));
                }
            }
            3 => {
                if let Some(v) = change.value {
                    ctx.set_setting("break_mins", json!(v.round() as u32));
                }
            }
            4 => {
                if let Some(a) = change.active {
                    ctx.set_setting("notify", json!(a));
                }
            }
            5 => {
                if let Some(c) = change.color {
                    ctx.set_setting("accent_color", json!(c));
                }
            }
            _ => {}
        }
        self.sync_from_settings(ctx);
        if self.state == TimerState::Idle && self.mode_idx == 1 {
            self.target_secs = self.default_secs_for_mode();
            self.remaining_secs = self.target_secs;
        }
        ctx.mark_dirty();
    }

    fn on_button_clicked(&mut self, ctx: &mut ActionContext, row_idx: usize) {
        if row_idx == 6 {
            self.reset_timer(ctx);
        }
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let (ar, ag, ab) = self.active_palette();

        let frac = if self.target_secs > 0 {
            (self.remaining_secs as f64 / self.target_secs as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Left side: Circular Countdown Ring
        let r = (h * 0.34).clamp(16.0, 36.0);
        let cx = r + 14.0;
        let cy = h * 0.48;
        let lw = (h * 0.065).clamp(3.8, 6.5);

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.14);
        cr.set_line_width(lw);
        cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.stroke();

        if frac > 0.0 {
            let start = -std::f64::consts::FRAC_PI_2;
            let sweep = 2.0 * std::f64::consts::PI * frac;
            cr.set_source_rgba(ar, ag, ab, 0.98);
            cr.set_line_width(lw);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.arc(cx, cy, r, start, start + sweep);
            let _ = cr.stroke();
        }

        // Play / Pause / Check glyph inside circular ring
        cr.set_source_rgb(0.96, 0.96, 1.0);
        let gs = r * 0.44;
        match self.state {
            TimerState::Running => {
                // Pause bars
                let bw = gs * 0.42;
                let gap = gs * 0.34;
                let bh = gs * 1.45;
                cr.rectangle(cx - gap / 2.0 - bw, cy - bh / 2.0, bw, bh);
                cr.rectangle(cx + gap / 2.0, cy - bh / 2.0, bw, bh);
                let _ = cr.fill();
            }
            TimerState::Finished => {
                // Checkmark
                cr.set_source_rgb(ar, ag, ab);
                cr.set_line_width(2.6);
                cr.set_line_cap(cairo::LineCap::Round);
                cr.set_line_join(cairo::LineJoin::Round);
                cr.move_to(cx - gs * 0.65, cy + gs * 0.05);
                cr.line_to(cx - gs * 0.12, cy + gs * 0.55);
                cr.line_to(cx + gs * 0.72, cy - gs * 0.48);
                let _ = cr.stroke();
            }
            _ => {
                // Play triangle
                let ox = gs * 0.14;
                cr.new_path();
                cr.move_to(cx - gs * 0.62 + ox, cy - gs * 0.78);
                cr.line_to(cx + gs * 0.84 + ox, cy);
                cr.line_to(cx - gs * 0.62 + ox, cy + gs * 0.78);
                cr.close_path();
                let _ = cr.fill();
            }
        }

        // Right side: Mode pill + Huge MM:SS + Hint footer
        let rx = cx + r + 12.0;
        let rw = (w - rx - 8.0).max(30.0);

        let badge = match self.state {
            TimerState::Finished => "DONE!",
            TimerState::Paused => "PAUSED",
            _ if self.mode_idx == 1 => match self.phase {
                PomodoroPhase::Focus => "FOCUS",
                PomodoroPhase::Break => "BREAK",
            },
            TimerState::Running => "RUNNING",
            TimerState::Idle => "TIMER",
        };
        let pill_w = (badge.len() as f64 * 6.8 + 12.0).clamp(rw.min(44.0), rw);
        let pill_h = (h * 0.17).clamp(13.0, 18.0);
        draw_pill(cr, rx, h * 0.10, pill_w, pill_h, pill_h / 2.0);
        cr.set_source_rgba(ar, ag, ab, 0.28);
        let _ = cr.fill();
        draw_cairo_text(
            cr,
            badge,
            rx,
            h * 0.10 + (pill_h - (h * 0.10).clamp(7.5, 10.5)) * 0.36,
            (h * 0.10).clamp(7.5, 10.5),
            (1.0, 1.0, 1.0),
            true,
            pango::Alignment::Center,
            pill_w,
        );

        let mins = self.remaining_secs / 60;
        let secs = self.remaining_secs % 60;
        let time_str = format!("{:02}:{:02}", mins, secs);
        let time_sz = (h * 0.30).clamp(15.0, 31.0);
        draw_cairo_text(
            cr,
            &time_str,
            rx,
            h * 0.34,
            time_sz,
            (1.0, 1.0, 1.0),
            true,
            pango::Alignment::Left,
            rw,
        );

        let hint = match self.state {
            TimerState::Finished if self.mode_idx == 1 => "Press for next phase",
            TimerState::Finished => "Press to reset",
            TimerState::Running => "Turn ± · Press Pause",
            _ => "Turn ± · Press Start",
        };
        draw_cairo_text(
            cr,
            hint,
            rx,
            h * 0.72,
            (h * 0.095).clamp(7.5, 10.0),
            (0.66, 0.66, 0.78),
            false,
            pango::Alignment::Left,
            rw,
        );

        // Sleek bottom progress hairline across entire segment
        let bar_h = 3.0;
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        cr.rectangle(0.0, h - bar_h, w, bar_h);
        let _ = cr.fill();
        cr.set_source_rgba(ar, ag, ab, 0.96);
        cr.rectangle(0.0, h - bar_h, w * frac, bar_h);
        let _ = cr.fill();

        true
    }
}
