use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand,
};
use crate::plugins::ballista_core::system_metrics::draw_cairo_text;
use serde_json::json;
use std::process::Command;

const LAUNCHER_MODES: &[&str] = &["Application", "Website URL", "File / Folder"];

pub struct LauncherAction {
    mode_idx: usize,
    desktop_id: String,
    app_name: String,
    args: String,
    url: String,
    file_path: String,
    has_app_icon: bool,
}

impl LauncherAction {
    pub fn new() -> Self {
        Self {
            mode_idx: 0,
            desktop_id: String::new(),
            app_name: String::new(),
            args: String::new(),
            url: String::new(),
            file_path: String::new(),
            has_app_icon: false,
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.mode_idx = ctx.get_usize("mode", 0).min(LAUNCHER_MODES.len() - 1);
        self.desktop_id = ctx.get_str("desktop_id", "");
        self.app_name = ctx.get_str("app_name", "");
        self.args = ctx.get_str("args", "");
        self.url = ctx.get_str("url", "");
        self.file_path = ctx.get_str("file_path", "");
        self.has_app_icon = !ctx.get_str("icon_path", "").is_empty()
            || !ctx.get_str("icon_name", "").is_empty();
    }
}

pub fn launch_desktop_app(desktop_id: &str, extra_args: &str) {
    let id = desktop_id.trim();
    if id.is_empty() {
        return;
    }
    let id_owned = id.to_string();
    let args_owned = extra_args.trim().to_string();
    std::thread::spawn(move || {
        // 1. Try gtk-launch (handles native, Flatpak, and user .desktop entries cleanly)
        let mut cmd = Command::new("gtk-launch");
        cmd.arg(&id_owned);
        if !args_owned.is_empty() {
            for part in args_owned.split_whitespace() {
                cmd.arg(part);
            }
        }
        if cmd.spawn().is_ok() {
            return;
        }
        // 2. Fallback to gio launch
        let _ = Command::new("gio")
            .args(["launch", &format!("/usr/share/applications/{id_owned}")])
            .spawn();
    });
}

pub fn open_url_or_path(target: &str) {
    let raw = target.trim();
    if raw.is_empty() {
        return;
    }
    let normalized = if raw.starts_with("http://")
        || raw.starts_with("https://")
        || raw.starts_with("file://")
        || raw.starts_with('/')
        || raw.starts_with('~')
    {
        if let Some(rest) = raw.strip_prefix("~/") {
            let home = std::env::var("HOME").unwrap_or_default();
            format!("{home}/{rest}")
        } else {
            raw.to_string()
        }
    } else {
        format!("https://{raw}")
    };
    std::thread::spawn(move || {
        let _ = Command::new("xdg-open").arg(&normalized).spawn();
    });
}

impl BallistaAction for LauncherAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, _ctx: &mut ActionContext) {
        match self.mode_idx {
            0 => launch_desktop_app(&self.desktop_id, &self.args),
            1 => open_url_or_path(&self.url),
            _ => open_url_or_path(&self.file_path),
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let mode = ctx.get_usize("mode", 0).min(LAUNCHER_MODES.len() - 1);
        let app_name = ctx.get_str("app_name", "");
        let file_path = ctx.get_str("file_path", "");

        let mut r0 = make_row(0, "ComboRow", "Launch Type");
        r0.options = LAUNCHER_MODES.iter().map(|s| s.to_string()).collect();
        r0.selected = mode;

        let mut r1 = make_row(1, "ActionRow", "Selected App");
        r1.subtitle = if app_name.is_empty() {
            "None selected — click Choose Application".to_string()
        } else {
            app_name.clone()
        };
        r1.visible = mode == 0;

        let mut r2 = make_row(2, "ButtonRow", "Application");
        r2.button_label = if app_name.is_empty() {
            "Choose Application...".to_string()
        } else {
            format!("Change App ({})", app_name)
        };
        r2.suggested = true;
        r2.visible = mode == 0;

        let mut r3 = make_row(3, "EntryRow", "Extra Arguments");
        r3.text = ctx.get_str("args", "");
        r3.visible = mode == 0;

        let mut r4 = make_row(4, "EntryRow", "Website URL");
        r4.text = ctx.get_str("url", "");
        r4.visible = mode == 1;

        let mut r5 = make_row(5, "ActionRow", "Selected Target");
        r5.subtitle = if file_path.is_empty() {
            "No file or folder selected".to_string()
        } else {
            file_path
        };
        r5.visible = mode == 2;

        let mut r6 = make_row(6, "ButtonRow", "Pick File");
        r6.button_label = "Choose File...".to_string();
        r6.suggested = true;
        r6.visible = mode == 2;

        let mut r7 = make_row(7, "ButtonRow", "Pick Folder");
        r7.button_label = "Choose Folder...".to_string();
        r7.visible = mode == 2;

        vec![r0, r1, r2, r3, r4, r5, r6, r7]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        match row_idx {
            0 => {
                if let Some(s) = change.selected {
                    ctx.set_setting("mode", json!(s));
                    ctx.push(HostCommand::RefreshConfigRows);
                }
            }
            3 => {
                if let Some(t) = change.text {
                    ctx.set_setting("args", json!(t));
                }
            }
            4 => {
                if let Some(t) = change.text {
                    ctx.set_setting("url", json!(t));
                }
            }
            _ => {}
        }
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_button_clicked(&mut self, ctx: &mut ActionContext, row_idx: usize) {
        match row_idx {
            2 => ctx.push(HostCommand::OpenAppPicker),
            6 => ctx.push(HostCommand::OpenFilePicker {
                directories_only: false,
            }),
            7 => ctx.push(HostCommand::OpenFilePicker {
                directories_only: true,
            }),
            _ => {}
        }
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        // When an application has been selected and has its real freedesktop icon, let the standard
        // key icon layer render the crisp app icon natively!
        if self.mode_idx == 0 && self.has_app_icon {
            return false;
        }

        let min_d = w.min(h).max(24.0);
        let bg_grad = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
        bg_grad.add_color_stop_rgb(0.0, 0.115, 0.120, 0.165);
        bg_grad.add_color_stop_rgb(1.0, 0.072, 0.074, 0.102);
        let _ = cr.set_source(&bg_grad);
        cr.rectangle(0.0, 0.0, w, h);
        let _ = cr.fill();

        let cx = w / 2.0;
        let cy = h * 0.44;
        match self.mode_idx {
            1 => {
                // Globe emblem for Website URL
                let r = min_d * 0.24;
                cr.set_source_rgba(0.38, 0.65, 0.98, 0.92);
                cr.set_line_width((min_d * 0.028).clamp(1.6, 2.8));
                cr.arc(cx, cy, r, 0.0, 2.0 * std::f64::consts::PI);
                let _ = cr.stroke();
                cr.move_to(cx - r, cy);
                cr.line_to(cx + r, cy);
                cr.move_to(cx, cy - r);
                cr.line_to(cx, cy + r);
                let _ = cr.stroke();
            }
            2 => {
                // File / Document emblem
                let dw = min_d * 0.36;
                let dh = min_d * 0.44;
                cr.set_source_rgba(0.659, 0.400, 1.0, 0.22);
                cr.rectangle(cx - dw / 2.0, cy - dh / 2.0, dw, dh);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(0.659, 0.400, 1.0, 0.92);
                cr.set_line_width(1.8);
                let _ = cr.stroke();
            }
            _ => {
                // 3x3 App Grid emblem before user picks an app
                let cell = min_d * 0.11;
                let gap = min_d * 0.055;
                let total = cell * 3.0 + gap * 2.0;
                let sx = cx - total / 2.0;
                let sy = cy - total / 2.0;
                cr.set_source_rgba(0.659, 0.400, 1.0, 0.88);
                for gy in 0..3 {
                    for gx in 0..3 {
                        cr.rectangle(
                            sx + gx as f64 * (cell + gap),
                            sy + gy as f64 * (cell + gap),
                            cell,
                            cell,
                        );
                    }
                }
                let _ = cr.fill();
                draw_cairo_text(
                    cr,
                    "Pick App",
                    4.0,
                    h - (min_d * 0.21),
                    (min_d * 0.125).clamp(8.5, 13.0),
                    (0.78, 0.78, 0.86),
                    true,
                    pango::Alignment::Center,
                    w - 8.0,
                );
            }
        }
        true
    }
}
