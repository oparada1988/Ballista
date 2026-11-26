use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand, LabelPos,
};
use crate::plugins::ballista_core::system_metrics::draw_cairo_text;
use serde_json::json;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct RunResult {
    output: String,
    success: bool,
    at: Instant,
}

pub struct RunCommandAction {
    command: String,
    workdir: String,
    in_terminal: bool,
    show_output: bool,
    last_result: Arc<Mutex<Option<RunResult>>>,
    last_shown_at: Option<Instant>,
}

impl RunCommandAction {
    pub fn new() -> Self {
        Self {
            command: String::new(),
            workdir: String::new(),
            in_terminal: false,
            show_output: false,
            last_result: Arc::new(Mutex::new(None)),
            last_shown_at: None,
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.command = ctx.get_str("command", "");
        self.workdir = ctx.get_str("workdir", "");
        self.in_terminal = ctx.get_bool("in_terminal", false);
        self.show_output = ctx.get_bool("show_output", false);
    }

    fn trigger(&self) {
        let cmd = self.command.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        let workdir = self.workdir.trim().to_string();
        let in_term = self.in_terminal;
        let capture = self.show_output;
        let res_slot = self.last_result.clone();

        std::thread::spawn(move || {
            if in_term {
                let ok = spawn_in_terminal(&cmd, &workdir);
                *res_slot.lock().unwrap() = Some(RunResult {
                    output: if ok { "Launched".into() } else { "No Term".into() },
                    success: ok,
                    at: Instant::now(),
                });
            } else if capture {
                let mut c = Command::new("sh");
                c.arg("-c").arg(&cmd);
                if !workdir.is_empty() && std::path::Path::new(&workdir).is_dir() {
                    c.current_dir(&workdir);
                }
                match c.output() {
                    Ok(out) => {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        let first_line = stdout
                            .lines()
                            .find(|l| !l.trim().is_empty())
                            .or_else(|| stderr.lines().find(|l| !l.trim().is_empty()))
                            .unwrap_or(if out.status.success() { "Done" } else { "Failed" })
                            .trim()
                            .to_string();
                        *res_slot.lock().unwrap() = Some(RunResult {
                            output: first_line,
                            success: out.status.success(),
                            at: Instant::now(),
                        });
                    }
                    Err(e) => {
                        *res_slot.lock().unwrap() = Some(RunResult {
                            output: format!("Err: {e}"),
                            success: false,
                            at: Instant::now(),
                        });
                    }
                }
            } else {
                let ok = execute_shell_command(&cmd, &workdir, false);
                *res_slot.lock().unwrap() = Some(RunResult {
                    output: if ok { "OK".into() } else { "Err".into() },
                    success: ok,
                    at: Instant::now(),
                });
            }
        });
    }
}

pub fn execute_shell_command(cmd: &str, workdir: &str, in_terminal: bool) -> bool {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return false;
    }
    if in_terminal {
        return spawn_in_terminal(trimmed, workdir);
    }
    let mut c = Command::new("sh");
    c.arg("-c").arg(trimmed);
    if !workdir.is_empty() && std::path::Path::new(workdir).is_dir() {
        c.current_dir(workdir);
    }
    c.spawn().is_ok()
}

fn spawn_in_terminal(cmd: &str, workdir: &str) -> bool {
    let shell_cmd = format!("{cmd}; echo; read -n 1 -s -r -p 'Press any key to close...'");
    let candidates: &[(&str, &[&str])] = &[
        ("ptyxis", &["--", "sh", "-c"]),
        ("gnome-terminal", &["--", "sh", "-c"]),
        ("kgx", &["-e", "sh", "-c"]),
        ("kitty", &["sh", "-c"]),
        ("alacritty", &["-e", "sh", "-c"]),
        ("konsole", &["-e", "sh", "-c"]),
        ("xfce4-terminal", &["-e"]),
        ("xterm", &["-e", "sh", "-c"]),
    ];
    for (bin, args) in candidates {
        let mut c = Command::new(bin);
        if !workdir.is_empty() && std::path::Path::new(workdir).is_dir() {
            c.current_dir(workdir);
        }
        c.args(*args);
        c.arg(&shell_cmd);
        if c.spawn().is_ok() {
            return true;
        }
    }
    false
}

impl BallistaAction for RunCommandAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_key_down(&mut self, _ctx: &mut ActionContext) {
        self.trigger();
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_millis(350))
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        let res_opt = self.last_result.lock().unwrap().clone();
        if let Some(res) = res_opt {
            if self.last_shown_at != Some(res.at) {
                self.last_shown_at = Some(res.at);
                if self.show_output {
                    ctx.push(HostCommand::SetLabel {
                        position: LabelPos::Bottom,
                        text: res.output,
                        font_size: Some(10),
                        color: Some(if res.success {
                            "#86efac".into()
                        } else {
                            "#fca5a5".into()
                        }),
                    });
                }
                ctx.mark_dirty();
            } else if res.at.elapsed() >= Duration::from_secs(2) && res.at.elapsed() < Duration::from_millis(2500) {
                // Fade out status border flash cleanly
                ctx.mark_dirty();
            }
        }
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let mut r0 = make_row(0, "EntryRow", "Command");
        r0.text = ctx.get_str("command", "");

        let mut r1 = make_row(1, "EntryRow", "Working Directory");
        r1.text = ctx.get_str("workdir", "");

        let mut r2 = make_row(2, "SwitchRow", "Open in Terminal");
        r2.active = ctx.get_bool("in_terminal", false);

        let mut r3 = make_row(3, "SwitchRow", "Show Output on Key");
        r3.active = ctx.get_bool("show_output", false);
        r3.visible = !r2.active;

        let mut r4 = make_row(4, "ButtonRow", "Test Execution");
        r4.button_label = "Run Now".to_string();
        r4.suggested = true;

        vec![r0, r1, r2, r3, r4]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        match row_idx {
            0 => {
                if let Some(t) = change.text {
                    ctx.set_setting("command", json!(t));
                }
            }
            1 => {
                if let Some(t) = change.text {
                    ctx.set_setting("workdir", json!(t));
                }
            }
            2 => {
                if let Some(a) = change.active {
                    ctx.set_setting("in_terminal", json!(a));
                    ctx.push(HostCommand::RefreshConfigRows);
                }
            }
            3 => {
                if let Some(a) = change.active {
                    ctx.set_setting("show_output", json!(a));
                }
            }
            _ => {}
        }
        self.sync_from_settings(ctx);
        ctx.mark_dirty();
    }

    fn on_button_clicked(&mut self, _ctx: &mut ActionContext, row_idx: usize) {
        if row_idx == 4 {
            self.trigger();
        }
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let min_d = w.min(h).max(24.0);

        // Crisp terminal prompt glyph `>_`
        let glyph_sz = (min_d * 0.34).clamp(14.0, 38.0);
        draw_cairo_text(
            cr,
            ">_",
            4.0,
            h * 0.43 - glyph_sz * 0.62,
            glyph_sz,
            (0.659, 0.400, 1.0), // #a866ff
            true,
            pango::Alignment::Center,
            w - 8.0,
        );

        // Brief exit-code status accent strip at bottom
        if let Some(res) = self.last_result.lock().unwrap().as_ref() {
            if res.at.elapsed() < Duration::from_secs(2) {
                if res.success {
                    cr.set_source_rgba(0.133, 0.773, 0.369, 0.9);
                } else {
                    cr.set_source_rgba(0.937, 0.267, 0.267, 0.9);
                }
                cr.rectangle(w * 0.2, h - 4.0, w * 0.6, 3.0);
                let _ = cr.fill();
            }
        }
        true
    }
}
