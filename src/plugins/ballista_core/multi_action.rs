use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand,
    MultiActionStep,
};
use crate::plugins::ballista_core::system_metrics::draw_cairo_text;
use serde_json::json;

const MULTI_MODES: &[&str] = &["Sequential", "Two-State Toggle"];

pub struct MultiActionRunner {
    mode_idx: usize,
    steps_a: Vec<MultiActionStep>,
    steps_b: Vec<MultiActionStep>,
    toggle_state: usize,
}

impl MultiActionRunner {
    pub fn new() -> Self {
        Self {
            mode_idx: 0,
            steps_a: Vec::new(),
            steps_b: Vec::new(),
            toggle_state: 0,
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.mode_idx = ctx.get_usize("mode", 0).min(MULTI_MODES.len() - 1);
        self.steps_a = ctx
            .settings
            .get("steps")
            .and_then(|v| serde_json::from_value::<Vec<MultiActionStep>>(v.clone()).ok())
            .unwrap_or_default();
        self.steps_b = ctx
            .settings
            .get("steps_b")
            .and_then(|v| serde_json::from_value::<Vec<MultiActionStep>>(v.clone()).ok())
            .unwrap_or_default();
    }
}

impl BallistaAction for MultiActionRunner {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        let steps = if self.mode_idx == 1 {
            let chosen = if self.toggle_state == 0 {
                self.steps_a.clone()
            } else {
                self.steps_b.clone()
            };
            self.toggle_state = 1 - self.toggle_state;
            ctx.mark_dirty();
            chosen
        } else {
            self.steps_a.clone()
        };

        if !steps.is_empty() {
            ctx.push(HostCommand::ExecuteMultiAction { steps });
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let mode = ctx.get_usize("mode", 0).min(MULTI_MODES.len() - 1);

        let mut r0 = make_row(0, "ComboRow", "Execution Mode");
        r0.options = MULTI_MODES.iter().map(|s| s.to_string()).collect();
        r0.selected = mode;

        let mut r1 = make_row(1, "ButtonRow", "Test Sequence");
        r1.button_label = "Run Sequence Now".to_string();
        r1.suggested = true;

        vec![r0, r1]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        if row_idx == 0 {
            if let Some(s) = change.selected {
                ctx.set_setting("mode", json!(s));
                ctx.push(HostCommand::RefreshConfigRows);
            }
        }
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_button_clicked(&mut self, ctx: &mut ActionContext, row_idx: usize) {
        if row_idx == 1 {
            self.on_key_down(ctx);
        }
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let min_d = w.min(h).max(24.0);

        // Stacked action bars glyph representing a multi-step sequence
        let bw = min_d * 0.50;
        let bh = (min_d * 0.075).clamp(4.0, 8.0);
        let gap = bh * 0.85;
        let start_x = (w - bw) / 2.0;
        let start_y = h * 0.25;
        for i in 0..3 {
            let y = start_y + i as f64 * (bh + gap);
            let alpha = if self.mode_idx == 1 && self.toggle_state == 1 {
                0.95 - i as f64 * 0.15
            } else {
                0.92 - i as f64 * 0.18
            };
            if self.mode_idx == 1 && self.toggle_state == 1 {
                cr.set_source_rgba(0.133, 0.773, 0.369, alpha);
            } else {
                cr.set_source_rgba(0.569, 0.275, 1.0, alpha);
            }
            cr.rectangle(start_x, y, bw - i as f64 * (bw * 0.12), bh);
            let _ = cr.fill();
        }

        let count = if self.mode_idx == 1 && self.toggle_state == 1 {
            self.steps_b.len()
        } else {
            self.steps_a.len()
        };
        let caption = if self.mode_idx == 1 {
            format!("S{} · {} step{}", self.toggle_state + 1, count, if count == 1 { "" } else { "s" })
        } else {
            format!("{} step{}", count, if count == 1 { "" } else { "s" })
        };
        let sz = (min_d * 0.13).clamp(8.5, 14.0);
        draw_cairo_text(
            cr,
            &caption,
            4.0,
            h - sz * 1.75,
            sz,
            (0.84, 0.84, 0.92),
            true,
            pango::Alignment::Center,
            w - 8.0,
        );
        true
    }
}
