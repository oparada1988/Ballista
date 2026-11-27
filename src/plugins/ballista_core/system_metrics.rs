use crate::core::plugin_bridge::BridgeConfigRow;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand, SlotSurface,
};
use pangocairo::functions as pc;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const BASE_METRIC_OPTIONS: &[&str] = &["CPU Usage", "RAM Usage", "CPU Temperature", "Disk Usage (/)"];
const ALL_METRIC_OPTIONS: &[&str] = &[
    "CPU Usage",
    "RAM Usage",
    "CPU Temperature",
    "Disk Usage (/)",
    "GPU Usage",
    "GPU VRAM",
    "GPU Temperature",
];
const STYLE_OPTIONS: &[&str] = &["Percentage", "Live Graph", "Radial Gauge"];
const REFRESH_OPTIONS: &[&str] = &["0.5s", "1s", "2s", "5s"];
const MAX_HISTORY: usize = 64;

fn is_gpu_metric(metric_idx: usize) -> bool {
    (4..=6).contains(&metric_idx)
}

fn metric_options_for(has_gpu: bool) -> &'static [&'static str] {
    if has_gpu {
        ALL_METRIC_OPTIONS
    } else {
        BASE_METRIC_OPTIONS
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GpuSample {
    pub id: String,
    pub pci_slot: Option<String>,
    pub name: String,
    pub util_pct: Option<f64>,
    pub vram_used_gb: Option<f64>,
    pub vram_total_gb: Option<f64>,
    pub vram_pct: Option<f64>,
    pub temp_c: Option<f64>,
    pub is_discrete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuCliBackend {
    None,
    NvidiaSmi,
    Nvtop,
    AmdgpuTop,
}

struct GpuMonitorState {
    sysfs_cards: Vec<SysfsGpuCard>,
    cli_backend: GpuCliBackend,
    cli_samples: Vec<GpuSample>,
    merged_samples: Vec<GpuSample>,
    probe_done: bool,
    last_requested: Option<Instant>,
}

struct GpuMonitor {
    state: Mutex<GpuMonitorState>,
    cv: Condvar,
}

static GPU_MONITOR: OnceLock<Arc<GpuMonitor>> = OnceLock::new();

impl GpuMonitor {
    fn global() -> Arc<Self> {
        GPU_MONITOR
            .get_or_init(|| {
                // Fast synchronous Linux sysfs DRM scan (<0.2ms, zero subprocesses) so AMD GPUs
                // are available on Frame 1 at startup.
                let sysfs_cards = discover_sysfs_gpus();
                let mut initial_samples: Vec<GpuSample> =
                    sysfs_cards.iter().map(sample_sysfs_gpu).collect();
                sort_gpus_primary_first(&mut initial_samples);

                let monitor = Arc::new(Self {
                    state: Mutex::new(GpuMonitorState {
                        sysfs_cards,
                        cli_backend: GpuCliBackend::None,
                        cli_samples: Vec::new(),
                        merged_samples: initial_samples,
                        probe_done: false,
                        last_requested: None,
                    }),
                    cv: Condvar::new(),
                });

                let mon_bg = monitor.clone();
                std::thread::spawn(move || {
                    mon_bg.run_background_loop();
                });

                monitor
            })
            .clone()
    }

    fn run_background_loop(&self) {
        // Phase 1: One-time CLI backend probe off the GTK main thread.
        // - Always check `nvidia-smi` when present so hybrid AMD-iGPU + NVIDIA-dGPU laptops
        //   automatically discover the discrete NVIDIA GPU alongside sysfs cards.
        // - Fall back to `nvtop -s` (Intel / multi-vendor) and `amdgpu_top -J -n 1` when no
        //   native sysfs or NVIDIA GPU was discovered.
        let has_sysfs = !self.state.lock().unwrap().sysfs_cards.is_empty();
        let mut chosen_backend = GpuCliBackend::None;
        let mut probed_samples: Vec<GpuSample> = Vec::new();

        if command_in_path("nvidia-smi") {
            let nv = poll_nvidia_smi();
            if !nv.is_empty() {
                chosen_backend = GpuCliBackend::NvidiaSmi;
                probed_samples = nv;
            }
        }

        if chosen_backend == GpuCliBackend::None && !has_sysfs {
            if command_in_path("nvtop") {
                let nvt = poll_nvtop();
                if !nvt.is_empty() {
                    chosen_backend = GpuCliBackend::Nvtop;
                    probed_samples = nvt;
                }
            }
            if chosen_backend == GpuCliBackend::None && command_in_path("amdgpu_top") {
                let amdt = poll_amdgpu_top();
                if !amdt.is_empty() {
                    chosen_backend = GpuCliBackend::AmdgpuTop;
                    probed_samples = amdt;
                }
            }
        }

        {
            let mut st = self.state.lock().unwrap();
            st.cli_backend = chosen_backend;
            st.cli_samples = probed_samples;
            st.probe_done = true;
            rebuild_merged_samples(&mut st);
        }

        if chosen_backend == GpuCliBackend::None {
            // Pure sysfs or no GPU: zero background subprocess polling required.
            return;
        }

        let poll_cadence = match chosen_backend {
            GpuCliBackend::AmdgpuTop => Duration::from_millis(1800),
            _ => Duration::from_millis(900),
        };

        // Phase 2: Lazy on-demand background polling (only runs while a GPU metric is active)
        loop {
            let should_poll = {
                let mut st = self.state.lock().unwrap();
                loop {
                    let active = st
                        .last_requested
                        .map(|t| t.elapsed() < Duration::from_secs(6))
                        .unwrap_or(false);
                    if active {
                        break true;
                    }
                    st = self.cv.wait(st).unwrap();
                }
            };

            if should_poll {
                let fresh = match chosen_backend {
                    GpuCliBackend::NvidiaSmi => poll_nvidia_smi(),
                    GpuCliBackend::Nvtop => poll_nvtop(),
                    GpuCliBackend::AmdgpuTop => poll_amdgpu_top(),
                    GpuCliBackend::None => Vec::new(),
                };
                if !fresh.is_empty() {
                    let mut st = self.state.lock().unwrap();
                    st.cli_samples = fresh;
                    rebuild_merged_samples(&mut st);
                }
                std::thread::sleep(poll_cadence);
            }
        }
    }

    fn has_gpu(&self) -> bool {
        !self.state.lock().unwrap().merged_samples.is_empty()
    }

    fn is_probe_done(&self) -> bool {
        self.state.lock().unwrap().probe_done
    }

    fn list_gpus(&self) -> Vec<GpuSample> {
        self.state.lock().unwrap().merged_samples.clone()
    }

    /// Instant non-blocking GPU sample lookup. Refreshes ultra-fast kernel `sysfs` cards directly
    /// (<0.05ms) and wakes the lazy background thread if a CLI backend (`nvidia-smi` / `nvtop` /
    /// `amdgpu_top`) is active.
    fn sample_for(&self, preferred_id: &str) -> Option<GpuSample> {
        let mut st = self.state.lock().unwrap();
        let was_idle = st
            .last_requested
            .map(|t| t.elapsed() >= Duration::from_secs(6))
            .unwrap_or(true);
        st.last_requested = Some(Instant::now());
        if st.cli_backend != GpuCliBackend::None && was_idle {
            self.cv.notify_one();
        }

        if !st.sysfs_cards.is_empty() {
            rebuild_merged_samples(&mut st);
        }

        if st.merged_samples.is_empty() {
            return None;
        }
        if !preferred_id.is_empty() && preferred_id != "auto" {
            if let Some(found) = st.merged_samples.iter().find(|g| g.id == preferred_id) {
                return Some(found.clone());
            }
        }
        st.merged_samples.first().cloned()
    }
}

fn rebuild_merged_samples(st: &mut GpuMonitorState) {
    let mut combined: Vec<GpuSample> = st.sysfs_cards.iter().map(sample_sysfs_gpu).collect();
    for cli in &st.cli_samples {
        let duplicate = cli.pci_slot.as_deref().map_or(false, |slot| {
            combined
                .iter()
                .any(|existing| existing.pci_slot.as_deref() == Some(slot))
        });
        if !duplicate {
            combined.push(cli.clone());
        }
    }
    sort_gpus_primary_first(&mut combined);
    st.merged_samples = combined;
}

fn sort_gpus_primary_first(gpus: &mut [GpuSample]) {
    gpus.sort_by(|a, b| {
        b.is_discrete
            .cmp(&a.is_discrete)
            .then_with(|| {
                let va = a.vram_total_gb.unwrap_or(0.0);
                let vb = b.vram_total_gb.unwrap_or(0.0);
                vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.id.cmp(&b.id))
    });
}

pub struct SystemMetricsAction {
    surface: SlotSurface,
    configured_metric_idx: usize,
    metric_idx: usize,
    gpu_id: String,
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
    last_gpu_count: usize,
}

impl SystemMetricsAction {
    pub fn new() -> Self {
        Self {
            surface: SlotSurface::Key,
            configured_metric_idx: 0,
            metric_idx: 0,
            gpu_id: String::new(),
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
            last_gpu_count: 0,
        }
    }

    fn default_style_for(surface: SlotSurface) -> usize {
        if surface == SlotSurface::Dial {
            1 // Live Graph shines by default on 200x100 widescreen dials!
        } else {
            0 // Percentage by default on square keys
        }
    }

    fn resolve_effective_metric(&self, configured: usize) -> usize {
        let mon = GpuMonitor::global();
        if is_gpu_metric(configured) {
            if mon.has_gpu() || !mon.is_probe_done() {
                configured
            } else {
                0 // Fall back to CPU Usage if GPU telemetry is unavailable on this host
            }
        } else {
            configured.min(BASE_METRIC_OPTIONS.len() - 1)
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.surface = ctx.surface;
        let prev_metric = self.metric_idx;
        let prev_gpu = self.gpu_id.clone();
        self.configured_metric_idx = ctx.get_usize("metric", 0).min(ALL_METRIC_OPTIONS.len() - 1);
        self.metric_idx = self.resolve_effective_metric(self.configured_metric_idx);
        self.gpu_id = ctx.get_str("gpu_id", "");
        self.style_idx = ctx
            .get_usize("style", Self::default_style_for(ctx.surface))
            .min(STYLE_OPTIONS.len() - 1);
        self.refresh_idx = ctx.get_usize("refresh", 1).min(REFRESH_OPTIONS.len() - 1);
        self.auto_heat = ctx.get_bool("auto_heat", true);
        self.accent_color = ctx.get_str("accent_color", "#9146ff");
        if prev_metric != self.metric_idx || prev_gpu != self.gpu_id {
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
            3 => {
                // Root Disk Usage via libc::statvfs
                self.sub_label = "DISK".to_string();
                if let Some((used_gb, total_gb, pct)) = read_disk_usage("/") {
                    self.current_pct = pct;
                    self.display_value = format!("{}%", pct.round() as u32);
                    self.detail_label = format!("{:.0} / {:.0} GB", used_gb, total_gb);
                }
            }
            4 => {
                // GPU Usage (%) + GPU temp secondary readout on Dial card
                self.sub_label = "GPU".to_string();
                if let Some(gpu) = GpuMonitor::global().sample_for(&self.gpu_id) {
                    if let Some(util) = gpu.util_pct {
                        self.current_pct = util.clamp(0.0, 100.0);
                        self.display_value = format!("{}%", self.current_pct.round() as u32);
                    } else {
                        self.current_pct = 0.0;
                        self.display_value = "N/A".to_string();
                    }
                    self.detail_label = gpu
                        .temp_c
                        .map(|c| format!("{}°C", c.round() as i32))
                        .unwrap_or_else(|| short_gpu_name(&gpu.name));
                } else {
                    self.current_pct = 0.0;
                    self.display_value = "--%".to_string();
                    self.detail_label.clear();
                }
            }
            5 => {
                // GPU VRAM Usage (% + GB used/total)
                self.sub_label = "VRAM".to_string();
                if let Some(gpu) = GpuMonitor::global().sample_for(&self.gpu_id) {
                    if let Some(pct) = gpu.vram_pct {
                        self.current_pct = pct.clamp(0.0, 100.0);
                        self.display_value = format!("{}%", self.current_pct.round() as u32);
                    } else {
                        self.current_pct = 0.0;
                        self.display_value = "N/A".to_string();
                    }
                    if let (Some(used), Some(total)) = (gpu.vram_used_gb, gpu.vram_total_gb) {
                        if total > 0.0 {
                            self.sub_label = format!("VRAM {:.1}G", used);
                            self.detail_label = format!("{:.1} / {:.0} GB", used, total);
                        } else {
                            self.detail_label = short_gpu_name(&gpu.name);
                        }
                    } else {
                        self.detail_label = short_gpu_name(&gpu.name);
                    }
                } else {
                    self.current_pct = 0.0;
                    self.display_value = "--%".to_string();
                    self.detail_label.clear();
                }
            }
            _ => {
                // GPU Temperature (°C)
                self.sub_label = "GPU TEMP".to_string();
                if let Some(gpu) = GpuMonitor::global().sample_for(&self.gpu_id) {
                    if let Some(temp_c) = gpu.temp_c {
                        self.current_pct = ((temp_c - 25.0) / 70.0 * 100.0).clamp(0.0, 100.0);
                        self.display_value = format!("{}°C", temp_c.round() as i32);
                        self.detail_label = short_gpu_name(&gpu.name);
                    } else {
                        self.current_pct = 0.0;
                        self.display_value = "N/A".to_string();
                        self.detail_label = "No Sensor".to_string();
                    }
                } else {
                    self.current_pct = 0.0;
                    self.display_value = "--°C".to_string();
                    self.detail_label.clear();
                }
            }
        }

        if self.history.len() >= MAX_HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(self.current_pct);
    }

    fn metric_short_label(&self) -> &'static str {
        match self.metric_idx {
            0 => "CPU",
            1 => "RAM",
            2 => "TEMP",
            3 => "DISK",
            4 => "GPU",
            5 => "VRAM",
            _ => "GPU °C",
        }
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
        let metric_short = self.metric_short_label();

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

        // Bottom metric pager dots (rotate dial to switch CPU / RAM / TEMP / DISK [/ GPU])
        let dot_count = metric_options_for(GpuMonitor::global().has_gpu()).len();
        let dot_gap = if dot_count > 4 { 8.5 } else { 10.0 };
        let total_dots_w = (dot_count.saturating_sub(1)) as f64 * dot_gap;
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
        let mon = GpuMonitor::global();
        self.last_gpu_count = mon.list_gpus().len();
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
        // Debounce brisk knob flicks so each turn cleanly steps to the next available metric
        let now = Instant::now();
        if let Some(last) = self.last_rotate_switch {
            if now.duration_since(last) < Duration::from_millis(160) {
                return;
            }
        }
        self.last_rotate_switch = Some(now);
        let n = metric_options_for(GpuMonitor::global().has_gpu()).len() as i32;
        let next = (self.metric_idx as i32 + if delta > 0 { 1 } else { -1 }).rem_euclid(n) as usize;
        self.configured_metric_idx = next;
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
        let mon = GpuMonitor::global();
        let cur_gpu_count = mon.list_gpus().len();
        if cur_gpu_count != self.last_gpu_count {
            self.last_gpu_count = cur_gpu_count;
            let resolved = self.resolve_effective_metric(self.configured_metric_idx);
            if resolved != self.metric_idx {
                self.metric_idx = resolved;
                self.history.clear();
            }
            ctx.push(HostCommand::RefreshConfigRows);
        } else if mon.is_probe_done() && is_gpu_metric(self.metric_idx) && !mon.has_gpu() {
            self.metric_idx = 0;
            self.history.clear();
            ctx.push(HostCommand::RefreshConfigRows);
        }
        self.sample();
        ctx.mark_dirty();
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let gpus = GpuMonitor::global().list_gpus();
        build_config_rows_with_gpus(ctx, &gpus)
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
            5 => {
                if let Some(sel) = change.selected {
                    let gpus = GpuMonitor::global().list_gpus();
                    if sel == 0 {
                        ctx.set_setting("gpu_id", json!(""));
                    } else if let Some(gpu) = gpus.get(sel - 1) {
                        ctx.set_setting("gpu_id", json!(gpu.id));
                    }
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

        let metric_short = self.metric_short_label();

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
                        if fallback.is_none()
                            && !matches!(name.as_str(), "amdgpu" | "nouveau" | "i915" | "xe")
                        {
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

pub(crate) fn build_config_rows_with_gpus(
    ctx: &ConfigContext,
    gpus: &[GpuSample],
) -> Vec<BridgeConfigRow> {
    let def_style = SystemMetricsAction::default_style_for(ctx.surface);
    let has_gpu = !gpus.is_empty();
    let metric_opts = metric_options_for(has_gpu);

    let mut r0 = make_row(0, "ComboRow", "Metric");
    r0.options = metric_opts.iter().map(|s| s.to_string()).collect();
    r0.selected = ctx.get_usize("metric", 0).min(r0.options.len() - 1);

    let mut rows = vec![r0];

    // Multi-GPU selector: only shown when >1 GPU is detected AND a GPU metric is selected
    if gpus.len() > 1 {
        let primary_name = gpus
            .first()
            .map(|g| short_gpu_name(&g.name))
            .unwrap_or_else(|| "Primary".to_string());
        let saved_gpu_id = ctx.get_str("gpu_id", "");
        let mut gpu_opts = Vec::with_capacity(gpus.len() + 1);
        gpu_opts.push(format!("Auto (Primary: {primary_name})"));
        let mut gpu_sel = 0usize;
        for (i, g) in gpus.iter().enumerate() {
            gpu_opts.push(g.name.clone());
            if !saved_gpu_id.is_empty() && g.id == saved_gpu_id {
                gpu_sel = i + 1;
            }
        }
        let mut r_gpu = make_row(5, "ComboRow", "GPU Device");
        r_gpu.options = gpu_opts;
        r_gpu.selected = gpu_sel;
        r_gpu.visible = is_gpu_metric(rows[0].selected);
        rows.push(r_gpu);
    }

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

    rows.push(r1);
    rows.push(r2);
    rows.push(r3);
    rows.push(r4);

    if ctx.surface == SlotSurface::Dial {
        let mut r_info = make_row(6, "ActionRow", "Dial Controls");
        r_info.subtitle = if has_gpu {
            "Rotate: Switch Metric (CPU / RAM / Temp / Disk / GPU) · Press: Cycle View".into()
        } else {
            "Rotate: Switch Metric (CPU / RAM / Temp / Disk) · Press: Cycle View".into()
        };
        rows.push(r_info);
    }
    rows
}

fn short_gpu_name(raw: &str) -> String {
    let mut s = raw.trim();
    for prefix in [
        "NVIDIA GeForce ",
        "NVIDIA ",
        "AMD Radeon ",
        "AMD ",
        "Intel(R) ",
        "Intel ",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            if !rest.trim().is_empty() {
                s = rest.trim();
                break;
            }
        }
    }
    if s.is_empty() {
        "GPU".to_string()
    } else if s.chars().count() > 22 {
        let truncated: String = s.chars().take(21).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

fn command_in_path(bin: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(bin);
            if cand.is_file() {
                return true;
            }
        }
    }
    false
}

// =========================================================================
// Tier 1: Native Linux DRM sysfs GPU Telemetry (AMD `amdgpu` & DRM drivers)
// =========================================================================

#[derive(Debug, Clone)]
struct SysfsGpuCard {
    id: String,
    pci_slot: Option<String>,
    name: String,
    device_dir: PathBuf,
    is_discrete: bool,
}

fn discover_sysfs_gpus() -> Vec<SysfsGpuCard> {
    let mut cards = Vec::new();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return cards;
    };

    let mut card_dirs: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if let Some(num) = fname.strip_prefix("card") {
            if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                let dev_dir = entry.path().join("device");
                if dev_dir.is_dir() {
                    card_dirs.push((fname, dev_dir));
                }
            }
        }
    }
    card_dirs.sort_by(|a, b| a.0.cmp(&b.0));

    for (card_name, dev_dir) in card_dirs {
        let has_busy = dev_dir.join("gpu_busy_percent").is_file();
        let has_vram = dev_dir.join("mem_info_vram_total").is_file()
            && dev_dir.join("mem_info_vram_used").is_file();
        if !has_busy && !has_vram {
            continue;
        }

        let vendor_hex = read_trimmed_hex(&dev_dir.join("vendor"));
        let device_hex = read_trimmed_hex(&dev_dir.join("device"));
        let sub_vendor_hex = read_trimmed_hex(&dev_dir.join("subsystem_vendor"));
        let sub_device_hex = read_trimmed_hex(&dev_dir.join("subsystem_device"));
        let pci_slot = parse_uevent_field(&dev_dir.join("uevent"), "PCI_SLOT_NAME")
            .map(|s| normalize_pci_slot(&s));

        let vram_total_bytes = fs::read_to_string(dev_dir.join("mem_info_vram_total"))
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        // Discrete GPUs expose >= 1.5 GiB dedicated VRAM; integrated APUs typically expose <= 1 GiB
        let is_discrete = vram_total_bytes >= 1_500_000_000;

        let product_name = fs::read_to_string(dev_dir.join("product_name"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let name = product_name
            .or_else(|| {
                lookup_pci_gpu_name(
                    vendor_hex.as_deref()?,
                    device_hex.as_deref()?,
                    sub_vendor_hex.as_deref(),
                    sub_device_hex.as_deref(),
                )
            })
            .unwrap_or_else(|| {
                let vendor_lbl = match vendor_hex.as_deref() {
                    Some("1002") => "AMD GPU",
                    Some("10de") => "NVIDIA GPU",
                    Some("8086") => "Intel GPU",
                    _ => "GPU",
                };
                format!("{vendor_lbl} ({card_name})")
            });

        cards.push(SysfsGpuCard {
            id: format!("drm:{card_name}"),
            pci_slot,
            name,
            device_dir: dev_dir,
            is_discrete,
        });
    }

    cards
}

fn sample_sysfs_gpu(card: &SysfsGpuCard) -> GpuSample {
    let util_pct = fs::read_to_string(card.device_dir.join("gpu_busy_percent"))
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|v| v.clamp(0.0, 100.0));

    let vram_used_bytes = fs::read_to_string(card.device_dir.join("mem_info_vram_used"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    let vram_total_bytes = fs::read_to_string(card.device_dir.join("mem_info_vram_total"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());

    let gib = 1024.0 * 1024.0 * 1024.0;
    let (vram_used_gb, vram_total_gb, vram_pct) = match (vram_used_bytes, vram_total_bytes) {
        (Some(used), Some(total)) if total > 0 => {
            let u = used as f64 / gib;
            let t = total as f64 / gib;
            let p = ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
            (Some(u), Some(t), Some(p))
        }
        _ => (None, None, None),
    };

    let temp_c = read_drm_hwmon_temp_c(&card.device_dir);

    GpuSample {
        id: card.id.clone(),
        pci_slot: card.pci_slot.clone(),
        name: card.name.clone(),
        util_pct,
        vram_used_gb,
        vram_total_gb,
        vram_pct,
        temp_c,
        is_discrete: card.is_discrete,
    }
}

fn read_drm_hwmon_temp_c(dev_dir: &Path) -> Option<f64> {
    let hwmon_root = dev_dir.join("hwmon");
    let entries = fs::read_dir(hwmon_root).ok()?;
    let mut fallback: Option<f64> = None;

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        for idx in 1..=6 {
            let input_p = dir.join(format!("temp{idx}_input"));
            let Ok(raw) = fs::read_to_string(&input_p) else {
                continue;
            };
            let Ok(milli_c) = raw.trim().parse::<f64>() else {
                continue;
            };
            let c = milli_c / 1000.0;
            if !(5.0..135.0).contains(&c) {
                continue;
            }
            let lbl = fs::read_to_string(dir.join(format!("temp{idx}_label")))
                .unwrap_or_default()
                .trim()
                .to_lowercase();
            if lbl == "edge" || lbl == "gpu" || lbl == "composite" {
                return Some(c);
            }
            if fallback.is_none() {
                fallback = Some(c);
            }
        }
    }
    fallback
}

fn read_trimmed_hex(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let hex = raw.trim().trim_start_matches("0x").trim_start_matches("0X").to_lowercase();
    if hex.is_empty() { None } else { Some(hex) }
}

fn parse_uevent_field(path: &Path, key: &str) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let prefix = format!("{key}=");
    for line in content.lines() {
        if let Some(val) = line.strip_prefix(&prefix) {
            let t = val.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn normalize_pci_slot(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    // nvidia-smi uses 8-digit domain `00000000:01:00.0`, sysfs uses 4-digit `0000:01:00.0`
    if let Some((dom, rest)) = lower.split_once(':') {
        let short_dom = if dom.len() > 4 {
            &dom[dom.len() - 4..]
        } else {
            dom
        };
        format!("{short_dom}:{rest}")
    } else {
        lower
    }
}

/// Resolves a friendly GPU marketing name from system `pci.ids` once at startup (zero subprocesses).
fn lookup_pci_gpu_name(
    vendor: &str,
    device: &str,
    sub_vendor: Option<&str>,
    sub_device: Option<&str>,
) -> Option<String> {
    let paths = [
        "/usr/share/misc/pci.ids",
        "/usr/share/hwdata/pci.ids",
        "/usr/share/pci.ids",
    ];
    let content = paths.iter().find_map(|p| fs::read_to_string(p).ok())?;

    let mut in_vendor = false;
    let mut in_device = false;
    let mut device_name: Option<String> = None;
    let mut subsys_name: Option<String> = None;
    let target_sub = match (sub_vendor, sub_device) {
        (Some(sv), Some(sd)) => Some(format!("{sv} {sd}")),
        _ => None,
    };

    for line in content.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with('\t') {
            if in_vendor {
                break;
            }
            if let Some(id) = line.split_whitespace().next() {
                if id.eq_ignore_ascii_case(vendor) {
                    in_vendor = true;
                }
            }
        } else if in_vendor && !line.starts_with("\t\t") {
            if in_device {
                break;
            }
            let trimmed = line.trim_start_matches('\t');
            if let Some((id, rest)) = trimmed.split_once("  ") {
                if id.trim().eq_ignore_ascii_case(device) {
                    in_device = true;
                    device_name = Some(extract_marketing_name(rest.trim(), vendor));
                }
            }
        } else if in_device && line.starts_with("\t\t") {
            if let Some(ref ts) = target_sub {
                let trimmed = line.trim_start_matches('\t');
                if let Some((ids, rest)) = trimmed.split_once("  ") {
                    if ids.trim().eq_ignore_ascii_case(ts) {
                        subsys_name = Some(extract_marketing_name(rest.trim(), vendor));
                        break;
                    }
                }
            }
        }
    }

    subsys_name.or(device_name)
}

fn extract_marketing_name(raw: &str, vendor: &str) -> String {
    let clean = if let (Some(l), Some(r)) = (raw.find('['), raw.rfind(']')) {
        if r > l + 2 {
            let inside = raw[l + 1..r].trim();
            if inside.len() >= 4 {
                inside
            } else {
                raw
            }
        } else {
            raw
        }
    } else {
        raw
    };
    let prefix = match vendor {
        "1002" if !clean.starts_with("AMD") => "AMD ",
        "10de" if !clean.starts_with("NVIDIA") => "NVIDIA ",
        "8086" if !clean.starts_with("Intel") => "Intel ",
        _ => "",
    };
    format!("{prefix}{clean}")
}

// =========================================================================
// Tier 2: `nvidia-smi` (bundled with NVIDIA Linux driver)
// =========================================================================

fn poll_nvidia_smi() -> Vec<GpuSample> {
    let Ok(out) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,pci.bus_id,name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_nvidia_smi_csv(&text)
}

pub(crate) fn parse_nvidia_smi_csv(csv: &str) -> Vec<GpuSample> {
    let mut gpus = Vec::new();
    for line in csv.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cols: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
        if cols.len() < 7 {
            continue;
        }
        let idx = cols[0];
        let pci_slot = if cols[1].is_empty() {
            None
        } else {
            Some(normalize_pci_slot(cols[1]))
        };
        let name = if cols[2].is_empty() {
            format!("NVIDIA GPU {idx}")
        } else {
            cols[2].to_string()
        };
        let util_pct = cols[3].parse::<f64>().ok().map(|v| v.clamp(0.0, 100.0));
        let mem_used_mib = cols[4].parse::<f64>().ok();
        let mem_total_mib = cols[5].parse::<f64>().ok();
        let (vram_used_gb, vram_total_gb, vram_pct) = match (mem_used_mib, mem_total_mib) {
            (Some(u), Some(t)) if t > 0.0 => (
                Some(u / 1024.0),
                Some(t / 1024.0),
                Some((u / t * 100.0).clamp(0.0, 100.0)),
            ),
            _ => (None, None, None),
        };
        let temp_c = cols[6].parse::<f64>().ok().filter(|c| (5.0..135.0).contains(c));

        gpus.push(GpuSample {
            id: format!("nv:{idx}"),
            pci_slot,
            name,
            util_pct,
            vram_used_gb,
            vram_total_gb,
            vram_pct,
            temp_c,
            is_discrete: true,
        });
    }
    sort_gpus_primary_first(&mut gpus);
    gpus
}

// =========================================================================
// Tier 3: `nvtop -s` (universal snapshot JSON: Intel, AMD, NVIDIA, etc.)
// =========================================================================

fn poll_nvtop() -> Vec<GpuSample> {
    let Ok(out) = Command::new("nvtop").arg("-s").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_nvtop_json(&text)
}

fn parse_numeric_str(val: Option<&Value>) -> Option<f64> {
    let v = val?;
    if let Some(f) = v.as_f64() {
        return Some(f);
    }
    let s = v.as_str()?.trim();
    let num: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if num.is_empty() {
        None
    } else {
        num.parse::<f64>().ok()
    }
}

pub(crate) fn parse_nvtop_json(raw: &str) -> Vec<GpuSample> {
    let Ok(val) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };

    let gib = 1024.0 * 1024.0 * 1024.0;
    let mut gpus = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let name = item
            .get("device_name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("GPU {i}"));

        let util_pct = parse_numeric_str(item.get("gpu_util")).map(|v| v.clamp(0.0, 100.0));
        let temp_c = parse_numeric_str(item.get("temp")).filter(|c| (5.0..135.0).contains(c));
        let mem_used_b = parse_numeric_str(item.get("mem_used"));
        let mem_total_b = parse_numeric_str(item.get("mem_total"));
        let mem_util = parse_numeric_str(item.get("mem_util")).map(|v| v.clamp(0.0, 100.0));

        let (vram_used_gb, vram_total_gb, vram_pct) = match (mem_used_b, mem_total_b) {
            (Some(u), Some(t)) if t > 0.0 => (
                Some(u / gib),
                Some(t / gib),
                mem_util.or_else(|| Some((u / t * 100.0).clamp(0.0, 100.0))),
            ),
            _ => (None, None, mem_util),
        };

        if util_pct.is_none() && vram_pct.is_none() && temp_c.is_none() {
            continue;
        }

        let is_discrete = mem_total_b.unwrap_or(0.0) >= 1_500_000_000.0
            && !name.to_lowercase().contains("integrated")
            && !name.to_lowercase().contains("uhd graphics");

        gpus.push(GpuSample {
            id: format!("nvtop:{i}"),
            pci_slot: None,
            name,
            util_pct,
            vram_used_gb,
            vram_total_gb,
            vram_pct,
            temp_c,
            is_discrete,
        });
    }
    sort_gpus_primary_first(&mut gpus);
    gpus
}

// =========================================================================
// Tier 4: `amdgpu_top -J -n 1` (AMD JSON fallback)
// =========================================================================

fn poll_amdgpu_top() -> Vec<GpuSample> {
    let Ok(out) = Command::new("amdgpu_top").args(["-J", "-n", "1"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_amdgpu_top_json(&text)
}

pub(crate) fn parse_amdgpu_top_json(raw: &str) -> Vec<GpuSample> {
    let Ok(val) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = val.get("devices").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut gpus = Vec::new();
    for (i, dev) in arr.iter().enumerate() {
        let name = dev
            .get("Info")
            .and_then(|info| info.get("DeviceName"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("AMD GPU {i}"));

        let pci_slot = dev
            .get("Info")
            .and_then(|info| info.get("DevicePath"))
            .and_then(|dp| dp.get("pci"))
            .and_then(|v| v.as_str())
            .map(normalize_pci_slot);

        let util_pct = dev
            .get("gpu_activity")
            .and_then(|a| a.get("GFX"))
            .and_then(|g| g.get("value"))
            .and_then(|v| v.as_f64())
            .map(|v| v.clamp(0.0, 100.0));

        let vram_used_mib = dev
            .get("VRAM")
            .and_then(|v| v.get("Total VRAM Usage"))
            .and_then(|x| x.get("value"))
            .and_then(|v| v.as_f64());
        let vram_total_mib = dev
            .get("VRAM")
            .and_then(|v| v.get("Total VRAM"))
            .and_then(|x| x.get("value"))
            .and_then(|v| v.as_f64());

        let (vram_used_gb, vram_total_gb, vram_pct) = match (vram_used_mib, vram_total_mib) {
            (Some(u), Some(t)) if t > 0.0 => (
                Some(u / 1024.0),
                Some(t / 1024.0),
                Some((u / t * 100.0).clamp(0.0, 100.0)),
            ),
            _ => (None, None, None),
        };

        let temp_c = dev
            .get("Sensors")
            .and_then(|s| {
                s.get("Edge Temperature")
                    .or_else(|| s.get("Junction Temperature"))
            })
            .and_then(|t| t.get("value"))
            .and_then(|v| v.as_f64())
            .filter(|c| (5.0..135.0).contains(c));

        if util_pct.is_none() && vram_pct.is_none() && temp_c.is_none() {
            continue;
        }

        let is_discrete = vram_total_mib.unwrap_or(0.0) >= 1536.0;

        gpus.push(GpuSample {
            id: format!("amdtop:{i}"),
            pci_slot,
            name,
            util_pct,
            vram_used_gb,
            vram_total_gb,
            vram_pct,
            temp_c,
            is_discrete,
        });
    }
    sort_gpus_primary_first(&mut gpus);
    gpus
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parses_nvtop_snapshot_json_cleanly() {
        let sample = r#"[
          {
            "device_name": "AMD Radeon RX 7800 XT",
            "temp": "48C",
            "gpu_util": "37%",
            "mem_util": "11%",
            "mem_total": "17163091968",
            "mem_used": "1808175104"
          }
        ]"#;
        let gpus = parse_nvtop_json(sample);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "AMD Radeon RX 7800 XT");
        assert_eq!(gpus[0].util_pct, Some(37.0));
        assert_eq!(gpus[0].temp_c, Some(48.0));
        assert_eq!(gpus[0].vram_pct, Some(11.0));
        assert!(gpus[0].vram_total_gb.unwrap() > 15.9);
        assert!(gpus[0].is_discrete);
    }

    #[test]
    fn parses_amdgpu_top_json_cleanly() {
        let sample = r#"{
          "devices": [
            {
              "Info": {
                "DeviceName": "AMD Radeon RX 7800 XT",
                "DevicePath": { "pci": "0000:12:00.0" }
              },
              "VRAM": {
                "Total VRAM": { "unit": "MiB", "value": 16368 },
                "Total VRAM Usage": { "unit": "MiB", "value": 1724 }
              },
              "gpu_activity": {
                "GFX": { "unit": "%", "value": 42 }
              },
              "Sensors": {
                "Edge Temperature": { "unit": "C", "value": 53 }
              }
            }
          ]
        }"#;
        let gpus = parse_amdgpu_top_json(sample);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "AMD Radeon RX 7800 XT");
        assert_eq!(gpus[0].pci_slot.as_deref(), Some("0000:12:00.0"));
        assert_eq!(gpus[0].util_pct, Some(42.0));
        assert_eq!(gpus[0].temp_c, Some(53.0));
        assert!(gpus[0].vram_pct.unwrap() > 10.0 && gpus[0].vram_pct.unwrap() < 11.0);
        assert!(gpus[0].is_discrete);
    }

    #[test]
    fn parses_nvidia_smi_csv_and_prioritizes_primary_gpu() {
        let csv = "0, 00000000:01:00.0, NVIDIA GeForce RTX 3060, 22, 2048, 12288, 54\n\
                   1, 00000000:02:00.0, NVIDIA GeForce RTX 4090, 81, 12288, 24576, 67\n";
        let gpus = parse_nvidia_smi_csv(csv);
        assert_eq!(gpus.len(), 2);
        // Higher-VRAM primary GPU sorted first
        assert_eq!(gpus[0].name, "NVIDIA GeForce RTX 4090");
        assert_eq!(gpus[0].pci_slot.as_deref(), Some("0000:02:00.0"));
        assert_eq!(gpus[0].util_pct, Some(81.0));
        assert_eq!(gpus[0].vram_pct, Some(50.0));
        assert_eq!(gpus[0].temp_c, Some(67.0));
        assert_eq!(gpus[1].name, "NVIDIA GeForce RTX 3060");
    }

    #[test]
    fn gpu_metrics_and_device_selector_visibility_rules() {
        let profiles = vec!["Default".to_string()];
        fn make_ctx<'a>(
            settings: &'a HashMap<String, Value>,
            profiles: &'a [String],
        ) -> ConfigContext<'a> {
            ConfigContext {
                slot: "key_0",
                surface: SlotSurface::Key,
                settings,
                available_profiles: profiles,
                active_profile_name: "Default",
                current_page_idx: 0,
                total_pages: 1,
                current_brightness: 80,
            }
        }

        // 0 GPUs: GPU metrics completely hidden, no GPU Device row
        let empty_settings = HashMap::new();
        let rows_no_gpu = build_config_rows_with_gpus(&make_ctx(&empty_settings, &profiles), &[]);
        assert_eq!(rows_no_gpu[0].options.len(), 4);
        assert!(!rows_no_gpu.iter().any(|r| r.title == "GPU Device"));

        // 1 GPU: 7 metric options exposed, GPU Device row stays hidden (primary auto-selected)
        let gpu_a = GpuSample {
            id: "drm:card1".into(),
            pci_slot: Some("0000:12:00.0".into()),
            name: "AMD Radeon RX 7800 XT".into(),
            util_pct: Some(25.0),
            vram_used_gb: Some(4.0),
            vram_total_gb: Some(16.0),
            vram_pct: Some(25.0),
            temp_c: Some(52.0),
            is_discrete: true,
        };
        let mut gpu_metric_settings = HashMap::new();
        gpu_metric_settings.insert("metric".into(), json!(4));
        let rows_one_gpu =
            build_config_rows_with_gpus(&make_ctx(&gpu_metric_settings, &profiles), &[gpu_a.clone()]);
        assert_eq!(rows_one_gpu[0].options.len(), 7);
        assert_eq!(rows_one_gpu[0].options[4], "GPU Usage");
        assert_eq!(rows_one_gpu[0].options[5], "GPU VRAM");
        assert_eq!(rows_one_gpu[0].options[6], "GPU Temperature");
        assert!(!rows_one_gpu.iter().any(|r| r.title == "GPU Device"));

        // 2 GPUs: GPU Device row hidden when CPU metric selected, visible when GPU metric selected
        let gpu_b = GpuSample {
            id: "drm:card0".into(),
            pci_slot: Some("0000:00:02.0".into()),
            name: "AMD Radeon Integrated".into(),
            util_pct: Some(2.0),
            vram_used_gb: Some(0.2),
            vram_total_gb: Some(0.5),
            vram_pct: Some(40.0),
            temp_c: Some(41.0),
            is_discrete: false,
        };
        let two_gpus = [gpu_a, gpu_b];

        let rows_two_cpu = build_config_rows_with_gpus(&make_ctx(&empty_settings, &profiles), &two_gpus);
        let dev_row_cpu = rows_two_cpu
            .iter()
            .find(|r| r.title == "GPU Device")
            .expect("GPU Device row exists for multi-GPU");
        assert!(!dev_row_cpu.visible);

        let rows_two_gpu =
            build_config_rows_with_gpus(&make_ctx(&gpu_metric_settings, &profiles), &two_gpus);
        let dev_row_gpu = rows_two_gpu
            .iter()
            .find(|r| r.title == "GPU Device")
            .expect("GPU Device row exists for multi-GPU");
        assert!(dev_row_gpu.visible);
        assert_eq!(dev_row_gpu.options.len(), 3);
        assert!(dev_row_gpu.options[0].starts_with("Auto (Primary:"));
    }

    #[test]
    fn gpu_metrics_render_cleanly_on_keys_and_dials() {
        let key_surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 120, 120).unwrap();
        let key_cr = cairo::Context::new(&key_surface).unwrap();
        let dial_surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 100).unwrap();
        let dial_cr = cairo::Context::new(&dial_surface).unwrap();

        for metric in 4..=6 {
            for style in 0..=2 {
                let mut act = SystemMetricsAction::new();
                act.metric_idx = metric;
                act.style_idx = style;
                act.sample();
                act.sample();
                assert!(act.render_visual_for_surface(&key_cr, 120.0, 120.0, SlotSurface::Key));
                assert!(act.render_visual_for_surface(&dial_cr, 200.0, 100.0, SlotSurface::Dial));
            }
        }
    }
}
