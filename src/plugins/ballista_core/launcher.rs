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
    desktop_path: String,
    exec_cmd: String,
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
            desktop_path: String::new(),
            exec_cmd: String::new(),
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
        self.desktop_path = ctx.get_str("desktop_path", "");
        self.exec_cmd = ctx.get_str("exec", "");
        self.app_name = ctx.get_str("app_name", "");
        self.args = ctx.get_str("args", "");
        self.url = ctx.get_str("url", "");
        self.file_path = ctx.get_str("file_path", "");
        self.has_app_icon = !ctx.get_str("icon_path", "").is_empty()
            || !ctx.get_str("icon_name", "").is_empty();
    }
}

fn enriched_xdg_data_dirs() -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut push_unique = |s: String| {
        if !s.is_empty() && !parts.contains(&s) {
            parts.push(s);
        }
    };
    if let Ok(cur) = std::env::var("XDG_DATA_DIRS") {
        for p in cur.split(':') {
            push_unique(p.trim().to_string());
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        push_unique(format!("{home}/.local/share/flatpak/exports/share"));
        push_unique(format!("{home}/.local/share"));
    }
    push_unique("/var/lib/flatpak/exports/share".to_string());
    push_unique("/var/lib/snapd/desktop".to_string());
    push_unique("/usr/local/share".to_string());
    push_unique("/usr/share".to_string());
    parts.join(":")
}

fn find_desktop_file(desktop_id: &str, desktop_path: &str) -> Option<std::path::PathBuf> {
    if !desktop_path.is_empty() {
        let p = std::path::PathBuf::from(desktop_path);
        if p.exists() {
            return Some(p);
        }
    }
    let clean_id = desktop_id.trim();
    if clean_id.is_empty() {
        return None;
    }
    let file_name = if clean_id.ends_with(".desktop") {
        clean_id.to_string()
    } else {
        format!("{clean_id}.desktop")
    };
    for dir in crate::core::PluginManager::desktop_application_dirs() {
        let direct = dir.join(&file_name);
        if direct.exists() {
            return Some(direct);
        }
        // Check 1 level of subdirs (e.g. AppImages / Wine / custom categories)
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let sub = e.path();
                if sub.is_dir() {
                    let nested = sub.join(&file_name);
                    if nested.exists() {
                        return Some(nested);
                    }
                }
            }
        }
    }
    None
}

fn read_desktop_exec(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut in_main = false;
    for raw in content.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            if line == "[Desktop Entry]" {
                in_main = true;
            } else if in_main {
                break;
            }
            continue;
        }
        if in_main {
            if let Some(val) = line.strip_prefix("Exec=") {
                let trimmed = val.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

fn strip_freedesktop_field_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some(&next) = chars.peek() {
                match next {
                    '%' => {
                        chars.next();
                        out.push('%');
                    }
                    'f' | 'F' | 'u' | 'U' | 'd' | 'D' | 'n' | 'N' | 'i' | 'c' | 'k' | 'v' | 'm' => {
                        chars.next();
                    }
                    _ => out.push(c),
                }
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

pub fn launch_desktop_app(desktop_id: &str, extra_args: &str) {
    launch_desktop_app_full(desktop_id, "", "", extra_args);
}

pub fn launch_desktop_app_full(
    desktop_id: &str,
    desktop_path: &str,
    exec_cmd: &str,
    extra_args: &str,
) {
    let id_owned = desktop_id.trim().to_string();
    let path_owned = desktop_path.trim().to_string();
    let exec_owned = exec_cmd.trim().to_string();
    let args_owned = extra_args.trim().to_string();
    if id_owned.is_empty() && path_owned.is_empty() && exec_owned.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        let resolved_path = find_desktop_file(&id_owned, &path_owned);
        let xdg_dirs = enriched_xdg_data_dirs();

        // 1. When no custom CLI args are added and we have the exact .desktop file,
        //    `gio launch <file.desktop>` handles System, Flatpak, Snap, and AppImage cleanly!
        if args_owned.is_empty() {
            if let Some(ref dp) = resolved_path {
                if let Ok(status) = Command::new("gio")
                    .arg("launch")
                    .arg(dp)
                    .env("XDG_DATA_DIRS", &xdg_dirs)
                    .status()
                {
                    if status.success() {
                        return;
                    }
                }
            }
        }

        // 2. Try gtk-launch with enriched XDG_DATA_DIRS (includes Flatpak & Snap dirs even under systemd --user)
        if !id_owned.is_empty() {
            let mut cmd = Command::new("gtk-launch");
            cmd.env("XDG_DATA_DIRS", &xdg_dirs);
            cmd.arg(&id_owned);
            if !args_owned.is_empty() {
                for part in args_owned.split_whitespace() {
                    cmd.arg(part);
                }
            }
            if let Ok(status) = cmd.status() {
                if status.success() {
                    return;
                }
            }
        }

        // 3. Direct Exec= fallback (works reliably for AppImages, custom commands, and extra arguments)
        let raw_exec = if !exec_owned.is_empty() {
            Some(exec_owned)
        } else {
            resolved_path.as_deref().and_then(read_desktop_exec)
        };
        if let Some(raw) = raw_exec {
            let mut cleaned = strip_freedesktop_field_codes(&raw);
            if !args_owned.is_empty() {
                cleaned.push(' ');
                cleaned.push_str(&args_owned);
            }
            if !cleaned.is_empty() {
                let _ = Command::new("sh")
                    .arg("-c")
                    .arg(&cleaned)
                    .env("XDG_DATA_DIRS", &xdg_dirs)
                    .spawn();
            }
        }
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
            0 => launch_desktop_app_full(&self.desktop_id, &self.desktop_path, &self.exec_cmd, &self.args),
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
