use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand,
};
use crate::plugins::ballista_core::system_metrics::{draw_cairo_text, parse_hex};
use serde_json::json;

pub struct FolderAction {
    folder_id: String,
    folder_name: String,
    folder_color: String,
}

impl FolderAction {
    pub fn new() -> Self {
        Self {
            folder_id: String::new(),
            folder_name: "Folder".to_string(),
            folder_color: "#9146ff".to_string(),
        }
    }

    fn sync_from_settings(&mut self, ctx: &mut ActionContext) {
        let id = ctx.get_str("folder_id", "");
        if id.is_empty() {
            let new_id = glib::uuid_string_random().to_string();
            ctx.set_setting("folder_id", json!(new_id));
            self.folder_id = new_id;
        } else {
            self.folder_id = id;
        }
        self.folder_name = ctx.get_str("folder_name", "Folder");
        self.folder_color = ctx.get_str("folder_color", "#9146ff");
    }
}

impl BallistaAction for FolderAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.push(HostCommand::EnterFolder {
            folder_id: self.folder_id.clone(),
            folder_name: self.folder_name.clone(),
        });
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let mut r0 = make_row(0, "EntryRow", "Folder Name");
        r0.text = ctx.get_str("folder_name", "Folder");

        let mut r1 = make_row(1, "ColorRow", "Folder Color");
        r1.color = ctx.get_str("folder_color", "#9146ff");

        let mut r2 = make_row(2, "ButtonRow", "Contents");
        r2.button_label = "Open Folder".to_string();
        r2.suggested = true;

        vec![r0, r1, r2]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        match row_idx {
            0 => {
                if let Some(name) = change.text {
                    let clean = if name.trim().is_empty() {
                        "Folder".to_string()
                    } else {
                        name
                    };
                    ctx.set_setting("folder_name", json!(clean));
                    ctx.push(HostCommand::SetTitle(clean));
                }
            }
            1 => {
                if let Some(col) = change.color {
                    ctx.set_setting("folder_color", json!(col));
                }
            }
            _ => {}
        }
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_button_clicked(&mut self, ctx: &mut ActionContext, row_idx: usize) {
        if row_idx == 2 {
            self.sync_from_settings(ctx);
            ctx.push(HostCommand::EnterFolder {
                folder_id: self.folder_id.clone(),
                folder_name: self.folder_name.clone(),
            });
        }
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let (ar, ag, ab) = parse_hex(&self.folder_color).unwrap_or((0.569, 0.275, 1.0));
        let min_d = w.min(h).max(24.0);

        let bg_grad = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
        bg_grad.add_color_stop_rgb(0.0, 0.115, 0.118, 0.160);
        bg_grad.add_color_stop_rgb(1.0, 0.072, 0.072, 0.100);
        let _ = cr.set_source(&bg_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        // Crisp vector folder icon
        let fw = min_d * 0.56;
        let fh = min_d * 0.42;
        let fx = (w - fw) / 2.0;
        let fy = h * 0.42 - fh / 2.0;
        let tab_w = fw * 0.42;
        let tab_h = fh * 0.22;

        // Back tab
        cr.new_path();
        cr.move_to(fx, fy + tab_h);
        cr.line_to(fx, fy + 2.0);
        cr.line_to(fx + tab_w, fy + 2.0);
        cr.line_to(fx + tab_w + tab_h * 0.7, fy + tab_h);
        cr.line_to(fx + fw, fy + tab_h);
        cr.line_to(fx + fw, fy + fh);
        cr.line_to(fx, fy + fh);
        cr.close_path();

        cr.set_source_rgba(ar, ag, ab, 0.30);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(ar, ag, ab, 0.95);
        cr.set_line_width((min_d * 0.026).clamp(1.5, 2.8));
        cr.set_line_join(cairo::LineJoin::Round);
        let _ = cr.stroke();

        // Front fold line
        cr.set_source_rgba(ar, ag, ab, 0.48);
        cr.set_line_width(1.2);
        cr.move_to(fx + 2.0, fy + tab_h + fh * 0.12);
        cr.line_to(fx + fw - 2.0, fy + tab_h + fh * 0.12);
        let _ = cr.stroke();

        true
    }
}

/// Built-in Back button placed automatically at Key 0 inside every Folder.
pub struct FolderBackAction;

impl FolderBackAction {
    pub fn new() -> Self {
        Self
    }
}

impl BallistaAction for FolderBackAction {
    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        ctx.push(HostCommand::ExitFolder);
    }

    fn config_rows(&self, _ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let mut r0 = make_row(0, "ActionRow", "Folder Back");
        r0.subtitle = "Press to return to the parent page or folder.".to_string();
        let mut r1 = make_row(1, "ButtonRow", "Navigation");
        r1.button_label = "Go Back Now".to_string();
        r1.suggested = true;
        vec![r0, r1]
    }

    fn on_button_clicked(&mut self, ctx: &mut ActionContext, row_idx: usize) {
        if row_idx == 1 {
            ctx.push(HostCommand::ExitFolder);
        }
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let min_d = w.min(h).max(24.0);
        let bg_grad = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
        bg_grad.add_color_stop_rgb(0.0, 0.145, 0.115, 0.215);
        bg_grad.add_color_stop_rgb(1.0, 0.085, 0.072, 0.125);
        let _ = cr.set_source(&bg_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        let sz = (min_d * 0.34).clamp(14.0, 38.0);
        draw_cairo_text(
            cr,
            "↩",
            4.0,
            h * 0.38 - sz * 0.56,
            sz,
            (0.847, 0.706, 0.996),
            true,
            pango::Alignment::Center,
            w - 8.0,
        );
        let lbl_sz = (min_d * 0.135).clamp(8.5, 14.0);
        draw_cairo_text(
            cr,
            "Back",
            4.0,
            h - lbl_sz * 1.75,
            lbl_sz,
            (0.92, 0.92, 0.96),
            true,
            pango::Alignment::Center,
            w - 8.0,
        );
        true
    }
}
