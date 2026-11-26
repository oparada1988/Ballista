use crate::core::hardware::draw_single_line_key_label;
use crate::core::plugin_bridge::BridgeConfigRow;
use crate::models::TextVerticalAlign;
use crate::native_sdk::plugin::{
    make_row, ActionContext, BallistaAction, ConfigChange, ConfigContext, HostCommand, LabelPos,
    SlotSurface,
};
use crate::plugins::ballista_core::system_metrics::{draw_cairo_text, parse_hex};
use gio::prelude::*;
use glib::VariantTy;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const MEDIA_MODES: &[&str] = &[
    "Play / Pause",
    "Play",
    "Pause",
    "Next Track",
    "Previous Track",
    "Stop",
    "Track Info",
];

const DIAL_ROTATE_MODES: &[&str] = &["Prev / Next Track", "Volume +/-", "Seek ±5s"];
const DIAL_CARD_LAYOUTS: &[&str] = &["Full Art Backdrop", "Split Cover"];

#[derive(Debug, Clone, Default)]
pub struct CachedArt {
    pub width: i32,
    pub height: i32,
    pub bgra_premul: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct MprisPlayerInfo {
    pub bus_name: String,
    pub identity: String,
    pub status: String, // "Playing", "Paused", "Stopped"
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub volume: f64,
    pub length_us: i64,
    pub position_us: i64,
    pub rate: f64,
    pub polled_at: Instant,
}

impl Default for MprisPlayerInfo {
    fn default() -> Self {
        Self {
            bus_name: String::new(),
            identity: String::new(),
            status: "Stopped".to_string(),
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            art_url: String::new(),
            volume: 1.0,
            length_us: 0,
            position_us: 0,
            rate: 1.0,
            polled_at: Instant::now(),
        }
    }
}

impl MprisPlayerInfo {
    /// Returns smoothly interpolated current playback position in microseconds.
    pub fn estimated_position_us(&self) -> i64 {
        let mut pos = self.position_us.max(0);
        if self.status == "Playing" {
            let elapsed_us = self.polled_at.elapsed().as_micros() as f64;
            let rate = if self.rate > 0.0 { self.rate } else { 1.0 };
            pos += (elapsed_us * rate) as i64;
        }
        if self.length_us > 0 {
            pos.clamp(0, self.length_us.max(0))
        } else {
            pos.max(0)
        }
    }
}

#[derive(Default)]
struct MprisSnapshot {
    players: Vec<MprisPlayerInfo>,
    art_cache: HashMap<String, Option<Arc<CachedArt>>>,
    art_in_flight: HashMap<String, bool>,
}

/// Shared native Linux D-Bus MPRIS2 monitor (zero playerctl subprocesses, supports http/https/file art).
pub struct MprisService {
    state: Arc<Mutex<MprisSnapshot>>,
}

static MPRIS_SINGLETON: OnceLock<Arc<MprisService>> = OnceLock::new();

impl MprisService {
    pub fn global() -> Arc<Self> {
        MPRIS_SINGLETON
            .get_or_init(|| {
                let state = Arc::new(Mutex::new(MprisSnapshot::default()));
                let svc = Arc::new(Self {
                    state: state.clone(),
                });
                let state_bg = state.clone();
                std::thread::spawn(move || {
                    loop {
                        let players = poll_mpris_players();
                        let urls_to_fetch: Vec<String> = {
                            let mut guard = state_bg.lock().unwrap();
                            let mut fetch = Vec::new();
                            for p in &players {
                                if !p.art_url.is_empty()
                                    && !guard.art_cache.contains_key(&p.art_url)
                                    && !guard.art_in_flight.contains_key(&p.art_url)
                                {
                                    guard.art_in_flight.insert(p.art_url.clone(), true);
                                    fetch.push(p.art_url.clone());
                                }
                            }
                            guard.players = players;
                            fetch
                        };

                        for url in urls_to_fetch {
                            let st_art = state_bg.clone();
                            std::thread::spawn(move || {
                                let decoded = fetch_and_decode_art(&url).map(Arc::new);
                                let mut guard = st_art.lock().unwrap();
                                guard.art_in_flight.remove(&url);
                                if guard.art_cache.len() > 24 {
                                    guard.art_cache.clear();
                                }
                                guard.art_cache.insert(url, decoded);
                            });
                        }

                        std::thread::sleep(Duration::from_millis(900));
                    }
                });
                svc
            })
            .clone()
    }

    pub fn list_identities(&self) -> Vec<String> {
        let guard = self.state.lock().unwrap();
        let mut out = vec!["Auto (Active Player)".to_string()];
        for p in &guard.players {
            let label = if !p.identity.is_empty() {
                p.identity.clone()
            } else {
                p.bus_name
                    .trim_start_matches("org.mpris.MediaPlayer2.")
                    .to_string()
            };
            if !out.contains(&label) {
                out.push(label);
            }
        }
        out
    }

    pub fn resolve_player(
        &self,
        preferred: &str,
    ) -> Option<(MprisPlayerInfo, Option<Arc<CachedArt>>)> {
        let guard = self.state.lock().unwrap();
        if guard.players.is_empty() {
            return None;
        }
        let chosen = if !preferred.is_empty() && preferred != "Auto (Active Player)" {
            guard
                .players
                .iter()
                .find(|p| {
                    p.identity.eq_ignore_ascii_case(preferred) || p.bus_name.contains(preferred)
                })
                .cloned()
        } else {
            guard
                .players
                .iter()
                .find(|p| p.status == "Playing")
                .or_else(|| {
                    guard
                        .players
                        .iter()
                        .find(|p| p.status == "Paused" && !p.title.is_empty())
                })
                .or_else(|| guard.players.first())
                .cloned()
        }?;

        let art = guard
            .art_cache
            .get(&chosen.art_url)
            .and_then(|opt| opt.clone());
        Some((chosen, art))
    }

    pub fn invoke_transport(&self, preferred: &str, method: &str) {
        let target_bus = self.resolve_player(preferred).map(|(p, _)| p.bus_name);
        let Some(bus) = target_bus else {
            return;
        };
        let method_owned = method.to_string();
        std::thread::spawn(move || {
            call_mpris_method(&bus, &method_owned);
        });
    }

    pub fn adjust_volume(&self, preferred: &str, delta: i32) {
        let target = self
            .resolve_player(preferred)
            .map(|(p, _)| (p.bus_name, p.volume));
        let Some((bus, cur_vol)) = target else {
            return;
        };
        let new_vol = (cur_vol + (delta as f64) * 0.05).clamp(0.0, 1.0);
        std::thread::spawn(move || {
            set_mpris_volume(&bus, new_vol);
        });
    }

    pub fn seek_offset(&self, preferred: &str, offset_us: i64) {
        let target_bus = self.resolve_player(preferred).map(|(p, _)| p.bus_name);
        let Some(bus) = target_bus else {
            return;
        };
        std::thread::spawn(move || {
            call_mpris_seek(&bus, offset_us);
        });
    }
}

pub fn trigger_mpris_action(action_name: &str) {
    let method = match action_name.to_lowercase().as_str() {
        "play" => "Play",
        "pause" => "Pause",
        "next" | "next track" => "Next",
        "previous" | "prev" | "previous track" => "Previous",
        "stop" => "Stop",
        _ => "PlayPause",
    };
    MprisService::global().invoke_transport("Auto (Active Player)", method);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlashControl {
    None,
    Prev,
    PlayPause,
    Next,
}

pub struct MediaAction {
    surface: SlotSurface,
    mode_idx: usize,
    dial_rotate_mode: usize, // 0 = Prev/Next Track, 1 = Volume, 2 = Seek ±5s
    dial_layout: usize,      // 0 = Full Art Backdrop, 1 = Split Cover
    show_progress: bool,
    player_pref: String,
    show_art_backdrop: bool,
    accent_color: String,
    cached_player: Option<MprisPlayerInfo>,
    cached_art: Option<Arc<CachedArt>>,
    last_sig: String,
    last_track_skip: Option<Instant>,
    flash_control: FlashControl,
    flash_until: Option<Instant>,
    marquee_start: Instant,
}

impl MediaAction {
    pub fn new() -> Self {
        Self {
            surface: SlotSurface::Key,
            mode_idx: 0,
            dial_rotate_mode: 0,
            dial_layout: 0,
            show_progress: true,
            player_pref: "Auto (Active Player)".to_string(),
            show_art_backdrop: true,
            accent_color: "#9146ff".to_string(),
            cached_player: None,
            cached_art: None,
            last_sig: String::new(),
            last_track_skip: None,
            flash_control: FlashControl::None,
            flash_until: None,
            marquee_start: Instant::now(),
        }
    }

    fn sync_from_settings(&mut self, ctx: &ActionContext) {
        self.surface = ctx.surface;
        self.mode_idx = ctx.get_usize("mode", 0).min(MEDIA_MODES.len() - 1);
        self.dial_rotate_mode = ctx
            .get_usize("dial_rotate_mode", 0)
            .min(DIAL_ROTATE_MODES.len() - 1);
        self.dial_layout = ctx
            .get_usize("dial_layout", 0)
            .min(DIAL_CARD_LAYOUTS.len() - 1);
        self.show_progress = ctx.get_bool("show_progress", true);
        self.player_pref = ctx.get_str("player", "Auto (Active Player)");
        self.show_art_backdrop = ctx.get_bool("show_art", true);
        self.accent_color = ctx.get_str("accent_color", "#9146ff");
    }

    fn trigger_flash(&mut self, ctrl: FlashControl, ctx: &mut ActionContext) {
        self.flash_control = ctrl;
        self.flash_until = Some(Instant::now() + Duration::from_millis(420));
        ctx.mark_dirty();
    }

    fn active_flash(&self) -> FlashControl {
        if let Some(until) = self.flash_until {
            if Instant::now() < until {
                return self.flash_control;
            }
        }
        FlashControl::None
    }

    fn toggle_play_pause(&mut self, ctx: &mut ActionContext) {
        MprisService::global().invoke_transport(&self.player_pref, "PlayPause");
        if let Some(ref mut p) = self.cached_player {
            p.status = if p.status == "Playing" {
                "Paused".to_string()
            } else {
                "Playing".to_string()
            };
            p.polled_at = Instant::now();
        }
        self.trigger_flash(FlashControl::PlayPause, ctx);
    }

    fn skip_track(&mut self, forward: bool, ctx: &mut ActionContext) {
        let method = if forward { "Next" } else { "Previous" };
        MprisService::global().invoke_transport(&self.player_pref, method);
        self.trigger_flash(
            if forward {
                FlashControl::Next
            } else {
                FlashControl::Prev
            },
            ctx,
        );
    }

    fn refresh_snapshot(&mut self, ctx: &mut ActionContext) {
        self.surface = ctx.surface;
        let svc = MprisService::global();
        let resolved = svc.resolve_player(&self.player_pref);
        let (p_opt, art_opt) = match resolved {
            Some((p, a)) => (Some(p), a),
            None => (None, None),
        };

        let flash_expired = self
            .flash_until
            .map(|u| Instant::now() >= u)
            .unwrap_or(false);
        if flash_expired {
            self.flash_control = FlashControl::None;
            self.flash_until = None;
        }

        let pos_sec = p_opt
            .as_ref()
            .map(|p| p.estimated_position_us() / 1_000_000)
            .unwrap_or(-1);
        let len_sec = p_opt.as_ref().map(|p| p.length_us / 1_000_000).unwrap_or(-1);

        // On a Dial card, include elapsed second & flash state in signature so the progress bar ticks smoothly at 1Hz
        let sig = if self.surface == SlotSurface::Dial {
            format!(
                "dial:{:?}:{:?}:{:?}:{:?}:{}:{}:{}:{:?}",
                p_opt.as_ref().map(|p| &p.status),
                p_opt.as_ref().map(|p| &p.title),
                p_opt.as_ref().map(|p| &p.artist),
                p_opt.as_ref().map(|p| &p.art_url),
                art_opt.is_some(),
                pos_sec,
                len_sec,
                self.active_flash(),
            )
        } else {
            format!(
                "key:{:?}:{:?}:{:?}:{:?}:{}",
                p_opt.as_ref().map(|p| &p.status),
                p_opt.as_ref().map(|p| &p.title),
                p_opt.as_ref().map(|p| &p.artist),
                p_opt.as_ref().map(|p| &p.art_url),
                art_opt.is_some(),
            )
        };

        let title_changed =
            self.cached_player.as_ref().map(|p| &p.title) != p_opt.as_ref().map(|p| &p.title);
        if title_changed {
            self.marquee_start = Instant::now();
        }

        self.cached_player = p_opt.clone();
        self.cached_art = art_opt;

        if sig != self.last_sig || flash_expired {
            self.last_sig = sig;
            if self.surface == SlotSurface::Key && self.mode_idx == 6 {
                let title = p_opt
                    .as_ref()
                    .map(|p| p.title.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "No Media".to_string());
                let artist = p_opt
                    .as_ref()
                    .map(|p| p.artist.clone())
                    .unwrap_or_default();
                ctx.push(HostCommand::SetLabel {
                    position: LabelPos::Top,
                    text: artist,
                    font_size: Some(10),
                    color: Some("#d4d4d8".to_string()),
                });
                ctx.push(HostCommand::SetLabel {
                    position: LabelPos::Bottom,
                    text: title,
                    font_size: Some(11),
                    color: Some("#ffffff".to_string()),
                });
            } else {
                // On Dial cards or non-TrackInfo keys, clear external label overlays so custom Cairo artwork stays clean
                ctx.push(HostCommand::SetLabel {
                    position: LabelPos::Top,
                    text: String::new(),
                    font_size: None,
                    color: None,
                });
                ctx.push(HostCommand::SetLabel {
                    position: LabelPos::Bottom,
                    text: String::new(),
                    font_size: None,
                    color: None,
                });
            }
            ctx.mark_dirty();
        }
    }

    fn render_dial_card(&self, cr: &cairo::Context, w: f64, h: f64) {
        let (ar, ag, ab) = parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0));
        let is_playing = self
            .cached_player
            .as_ref()
            .map(|p| p.status == "Playing")
            .unwrap_or(false);
        let flash = self.active_flash();

        let title = self
            .cached_player
            .as_ref()
            .map(|p| p.title.as_str())
            .filter(|t| !t.is_empty())
            .unwrap_or("No Media");
        let artist = self
            .cached_player
            .as_ref()
            .map(|p| p.artist.as_str())
            .unwrap_or("");
        let (pos_us, len_us) = self
            .cached_player
            .as_ref()
            .map(|p| (p.estimated_position_us(), p.length_us))
            .unwrap_or((0, 0));

        let (content_x, content_w) = if self.dial_layout == 0 {
            // FULL ART BACKDROP: album art fills the entire designated 200x100 segment
            if let Some(ref art) = self.cached_art {
                paint_cached_art(cr, art, 0.0, 0.0, w, h, 0.0);
                // Multi-stop dark scrim guarantees crisp white typography & controls on any cover
                let scrim = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
                scrim.add_color_stop_rgba(0.0, 0.04, 0.04, 0.07, 0.62);
                scrim.add_color_stop_rgba(0.42, 0.04, 0.04, 0.07, 0.44);
                scrim.add_color_stop_rgba(0.75, 0.04, 0.04, 0.07, 0.72);
                scrim.add_color_stop_rgba(1.0, 0.03, 0.03, 0.06, 0.90);
                let _ = cr.set_source(&scrim);
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();
            }
            (8.0, (w - 16.0).max(40.0))
        } else {
            // SPLIT COVER LAYOUT: crisp square album cover left + rich controls right
            let pad = (h * 0.09).clamp(5.0, 10.0);
            let art_sz = (h - pad * 2.0).max(24.0);
            if let Some(ref art) = self.cached_art {
                paint_cached_art(cr, art, pad, pad, art_sz, art_sz, 7.0);
            } else {
                draw_rounded_rect(cr, pad, pad, art_sz, art_sz, 7.0);
                cr.set_source_rgba(ar, ag, ab, 0.22);
                let _ = cr.fill();
                draw_transport_glyph(
                    cr,
                    pad + art_sz / 2.0,
                    pad + art_sz / 2.0,
                    art_sz * 0.24,
                    0,
                    is_playing,
                    (0.92, 0.92, 0.98),
                );
            }
            let cx = pad + art_sz + 8.0;
            (cx, (w - cx - 8.0).max(40.0))
        };

        // 1. Track Title & Artist (Top Band)
        let title_sz = (h * 0.145).clamp(10.0, 15.0);
        let artist_sz = (h * 0.110).clamp(8.5, 11.5);
        let align = if self.dial_layout == 0 {
            pango::Alignment::Center
        } else {
            pango::Alignment::Left
        };
        draw_cairo_text(
            cr,
            title,
            content_x,
            h * 0.06,
            title_sz,
            (1.0, 1.0, 1.0),
            true,
            align,
            content_w,
        );
        if !artist.is_empty() {
            draw_cairo_text(
                cr,
                artist,
                content_x,
                h * 0.06 + title_sz * 1.24,
                artist_sz,
                (0.82, 0.82, 0.90),
                false,
                align,
                content_w,
            );
        }

        // 2. Interactive Transport Controls Strip: [ ⏮ ]  [ ⏯ ]  [ ⏭ ]
        let ctrl_cy = if self.show_progress { h * 0.56 } else { h * 0.62 };
        let center_x = content_x + content_w * 0.50;
        let side_offset = (content_w * 0.26).clamp(24.0, 48.0);
        let prev_x = center_x - side_offset;
        let next_x = center_x + side_offset;
        let play_r = (h * 0.145).clamp(10.0, 16.0);
        let side_r = (h * 0.115).clamp(8.0, 13.0);

        // Previous button pill
        let prev_active = flash == FlashControl::Prev;
        cr.set_source_rgba(
            if prev_active { ar } else { 1.0 },
            if prev_active { ag } else { 1.0 },
            if prev_active { ab } else { 1.0 },
            if prev_active { 0.85 } else { 0.14 },
        );
        cr.arc(prev_x, ctrl_cy, side_r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
        draw_transport_glyph(
            cr,
            prev_x,
            ctrl_cy,
            side_r * 0.56,
            4,
            false,
            (0.96, 0.96, 1.0),
        );

        // Play/Pause center pill (accent glow when Playing or flashed)
        let play_highlight = is_playing || flash == FlashControl::PlayPause;
        if play_highlight {
            cr.set_source_rgba(ar, ag, ab, 0.92);
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.20);
        }
        cr.arc(center_x, ctrl_cy, play_r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
        if !play_highlight {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
            cr.set_line_width(1.2);
            cr.arc(center_x, ctrl_cy, play_r, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();
        }
        draw_transport_glyph(
            cr,
            center_x,
            ctrl_cy,
            play_r * 0.56,
            0,
            is_playing,
            (1.0, 1.0, 1.0),
        );

        // Next button pill
        let next_active = flash == FlashControl::Next;
        cr.set_source_rgba(
            if next_active { ar } else { 1.0 },
            if next_active { ag } else { 1.0 },
            if next_active { ab } else { 1.0 },
            if next_active { 0.85 } else { 0.14 },
        );
        cr.arc(next_x, ctrl_cy, side_r, 0.0, 2.0 * std::f64::consts::PI);
        let _ = cr.fill();
        draw_transport_glyph(
            cr,
            next_x,
            ctrl_cy,
            side_r * 0.56,
            3,
            false,
            (0.96, 0.96, 1.0),
        );

        // 3. Bottom Progress Bar + Elapsed / Total Time
        if self.show_progress {
            let bar_h = (h * 0.055).clamp(3.5, 6.0);
            let bar_y = h - bar_h - (h * 0.075).clamp(5.0, 9.0);
            let time_sz = (h * 0.095).clamp(7.5, 10.5);
            let time_y = bar_y - time_sz * 1.25;

            let (left_lbl, right_lbl, frac) = if len_us > 0 {
                let f = (pos_us as f64 / len_us as f64).clamp(0.0, 1.0);
                (format_time_us(pos_us), format_time_us(len_us), f)
            } else if is_playing {
                (format_time_us(pos_us), "LIVE".to_string(), 1.0)
            } else {
                ("0:00".to_string(), "--:--".to_string(), 0.0)
            };

            draw_cairo_text(
                cr,
                &left_lbl,
                content_x,
                time_y,
                time_sz,
                (0.86, 0.86, 0.94),
                false,
                pango::Alignment::Left,
                content_w * 0.5,
            );
            draw_cairo_text(
                cr,
                &right_lbl,
                content_x + content_w * 0.5,
                time_y,
                time_sz,
                (0.76, 0.76, 0.86),
                false,
                pango::Alignment::Right,
                content_w * 0.5,
            );

            // Track background
            let r = bar_h / 2.0;
            draw_rounded_rect(cr, content_x, bar_y, content_w, bar_h, r);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.22);
            let _ = cr.fill();

            // Active fill + subtle head dot
            if frac > 0.0 {
                let fill_w = (content_w * frac).max(bar_h).min(content_w);
                draw_rounded_rect(cr, content_x, bar_y, fill_w, bar_h, r);
                cr.set_source_rgba(ar, ag, ab, 0.98);
                let _ = cr.fill();

                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.arc(
                    content_x + fill_w - r.min(fill_w / 2.0),
                    bar_y + bar_h / 2.0,
                    bar_h * 0.62,
                    0.0,
                    2.0 * std::f64::consts::PI,
                );
                let _ = cr.fill();
            }
        }
    }
}

impl BallistaAction for MediaAction {
    fn on_load(&mut self, ctx: &mut ActionContext) {
        self.sync_from_settings(ctx);
        self.last_sig.clear();
        self.refresh_snapshot(ctx);
    }

    fn on_key_down(&mut self, ctx: &mut ActionContext) {
        let method = match self.mode_idx {
            0 | 6 => "PlayPause",
            1 => "Play",
            2 => "Pause",
            3 => "Next",
            4 => "Previous",
            5 => "Stop",
            _ => "PlayPause",
        };
        MprisService::global().invoke_transport(&self.player_pref, method);
        if matches!(self.mode_idx, 0 | 6) {
            if let Some(ref mut p) = self.cached_player {
                p.status = if p.status == "Playing" {
                    "Paused".to_string()
                } else {
                    "Playing".to_string()
                };
                ctx.mark_dirty();
            }
        }
    }

    fn on_dial_press(&mut self, ctx: &mut ActionContext) {
        self.toggle_play_pause(ctx);
    }

    fn on_dial_rotate(&mut self, ctx: &mut ActionContext, delta: i32) {
        if delta == 0 {
            return;
        }
        match self.dial_rotate_mode {
            0 => {
                // Default: CCW = Previous Track, CW = Next Track (with 300ms detent debounce so 1 flick skips 1 track!)
                let now = Instant::now();
                if let Some(last) = self.last_track_skip {
                    if now.duration_since(last) < Duration::from_millis(300) {
                        return;
                    }
                }
                self.last_track_skip = Some(now);
                self.skip_track(delta > 0, ctx);
            }
            1 => {
                MprisService::global().adjust_volume(&self.player_pref, delta);
                ctx.mark_dirty();
            }
            _ => {
                // Seek ±5 seconds per step
                let offset_us = (delta as i64) * 5_000_000;
                MprisService::global().seek_offset(&self.player_pref, offset_us);
                if let Some(ref mut p) = self.cached_player {
                    let cur = p.estimated_position_us();
                    p.position_us = if p.length_us > 0 {
                        (cur + offset_us).clamp(0, p.length_us.max(0))
                    } else {
                        (cur + offset_us).max(0)
                    };
                    p.polled_at = Instant::now();
                }
                ctx.mark_dirty();
            }
        }
    }

    fn on_touch_tap(&mut self, ctx: &mut ActionContext, norm_x: f64, _norm_y: f64) {
        // 3 intuitive touch zones matching the [ ⏮ ] [ ⏯ ] [ ⏭ ] controls painted on the 200x100 card!
        if norm_x < 0.33 {
            self.skip_track(false, ctx);
        } else if norm_x > 0.67 {
            self.skip_track(true, ctx);
        } else {
            self.toggle_play_pause(ctx);
        }
    }

    fn tick_interval(&self) -> Option<Duration> {
        Some(Duration::from_secs(1))
    }

    fn on_tick(&mut self, ctx: &mut ActionContext) {
        self.refresh_snapshot(ctx);
    }

    fn config_rows(&self, ctx: &ConfigContext) -> Vec<BridgeConfigRow> {
        let players = MprisService::global().list_identities();
        let saved_player = ctx.get_str("player", "Auto (Active Player)");
        let player_sel = players.iter().position(|p| p == &saved_player).unwrap_or(0);

        let status_summary = match &self.cached_player {
            Some(p) => {
                let trk = if p.title.is_empty() {
                    "Idle"
                } else {
                    p.title.as_str()
                };
                if p.length_us > 0 {
                    format!(
                        "{} — {} · {} / {} ({})",
                        p.identity,
                        trk,
                        format_time_us(p.estimated_position_us()),
                        format_time_us(p.length_us),
                        p.status
                    )
                } else {
                    format!("{} — {} ({})", p.identity, trk, p.status)
                }
            }
            None => "No active MPRIS media player detected".to_string(),
        };

        if ctx.surface == SlotSurface::Dial {
            // Surface-Aware Dial Card Settings
            let mut r0 = make_row(0, "ComboRow", "Dial Rotate");
            r0.options = DIAL_ROTATE_MODES.iter().map(|s| s.to_string()).collect();
            r0.selected = ctx
                .get_usize("dial_rotate_mode", 0)
                .min(r0.options.len() - 1);

            let mut r1 = make_row(1, "ComboRow", "MPRIS Player");
            r1.options = players;
            r1.selected = player_sel;

            let mut r2 = make_row(2, "ComboRow", "Card Layout");
            r2.options = DIAL_CARD_LAYOUTS.iter().map(|s| s.to_string()).collect();
            r2.selected = ctx.get_usize("dial_layout", 0).min(r2.options.len() - 1);

            let mut r3 = make_row(3, "SwitchRow", "Show Progress Bar");
            r3.active = ctx.get_bool("show_progress", true);

            let mut r4 = make_row(4, "ColorRow", "Accent Color");
            r4.color = ctx.get_str("accent_color", "#9146ff");

            let mut r5 = make_row(5, "ActionRow", "Now Playing");
            r5.subtitle = status_summary;

            return vec![r0, r1, r2, r3, r4, r5];
        }

        // Key Mode Settings
        let mode = ctx.get_usize("mode", 0).min(MEDIA_MODES.len() - 1);

        let mut r0 = make_row(0, "ComboRow", "Media Action");
        r0.options = MEDIA_MODES.iter().map(|s| s.to_string()).collect();
        r0.selected = mode;

        let mut r1 = make_row(1, "ComboRow", "MPRIS Player");
        r1.options = players;
        r1.selected = player_sel;

        let mut r2 = make_row(2, "SwitchRow", "Show Album Art");
        r2.active = ctx.get_bool("show_art", true);
        r2.visible = matches!(mode, 0 | 6);

        let mut r3 = make_row(3, "ColorRow", "Accent Color");
        r3.color = ctx.get_str("accent_color", "#9146ff");

        let mut r4 = make_row(4, "ActionRow", "Live Status");
        r4.subtitle = status_summary;

        vec![r0, r1, r2, r3, r4]
    }

    fn on_config_changed(&mut self, ctx: &mut ActionContext, row_idx: usize, change: ConfigChange) {
        if ctx.surface == SlotSurface::Dial {
            match row_idx {
                0 => {
                    if let Some(sel) = change.selected {
                        ctx.set_setting("dial_rotate_mode", json!(sel));
                    }
                }
                1 => {
                    if let Some(sel) = change.selected {
                        let players = MprisService::global().list_identities();
                        if let Some(name) = players.get(sel) {
                            ctx.set_setting("player", json!(name));
                        }
                    }
                }
                2 => {
                    if let Some(sel) = change.selected {
                        ctx.set_setting("dial_layout", json!(sel));
                    }
                }
                3 => {
                    if let Some(act) = change.active {
                        ctx.set_setting("show_progress", json!(act));
                    }
                }
                4 => {
                    if let Some(col) = change.color {
                        ctx.set_setting("accent_color", json!(col));
                    }
                }
                _ => {}
            }
        } else {
            match row_idx {
                0 => {
                    if let Some(sel) = change.selected {
                        ctx.set_setting("mode", json!(sel));
                    }
                }
                1 => {
                    if let Some(sel) = change.selected {
                        let players = MprisService::global().list_identities();
                        if let Some(name) = players.get(sel) {
                            ctx.set_setting("player", json!(name));
                        }
                    }
                }
                2 => {
                    if let Some(act) = change.active {
                        ctx.set_setting("show_art", json!(act));
                    }
                }
                3 => {
                    if let Some(col) = change.color {
                        ctx.set_setting("accent_color", json!(col));
                    }
                }
                _ => {}
            }
        }
        self.sync_from_settings(ctx);
        self.last_sig.clear();
        self.refresh_snapshot(ctx);
        ctx.push(HostCommand::RefreshConfigRows);
        ctx.mark_dirty();
    }

    fn render_visual(&self, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let surface = if w > h * 1.35 {
            SlotSurface::Dial
        } else {
            self.surface
        };
        self.render_visual_for_surface(cr, w, h, surface)
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

        let (ar, ag, ab) = parse_hex(&self.accent_color).unwrap_or((0.569, 0.275, 1.0));
        let is_playing = self
            .cached_player
            .as_ref()
            .map(|p| p.status == "Playing")
            .unwrap_or(false);

        // Square Key Rendering (album art backdrop only when enabled and available)
        let want_art = matches!(self.mode_idx, 0 | 6) && self.show_art_backdrop;
        if want_art {
            if let Some(ref art) = self.cached_art {
                paint_cached_art(cr, art, 0.0, 0.0, w, h, 0.0);
                let scrim = cairo::LinearGradient::new(0.0, 0.0, 0.0, h);
                if self.mode_idx == 6 {
                    scrim.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.45);
                    scrim.add_color_stop_rgba(0.45, 0.0, 0.0, 0.0, 0.10);
                    scrim.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.78);
                } else {
                    scrim.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.48);
                    scrim.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.62);
                }
                let _ = cr.set_source(&scrim);
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();
            }
        }

        if self.mode_idx == 6 {
            if self.cached_art.is_none() {
                let min_d = w.min(h);
                cr.set_source_rgba(ar, ag, ab, 0.22);
                cr.arc(w / 2.0, h / 2.0, min_d * 0.28, 0.0, 2.0 * std::f64::consts::PI);
                let _ = cr.fill();
                draw_transport_glyph(
                    cr,
                    w / 2.0,
                    h / 2.0,
                    min_d * 0.16,
                    0,
                    is_playing,
                    (0.92, 0.92, 0.98),
                );
            }
            if self.cached_player.is_none() {
                let _ = draw_single_line_key_label(
                    cr,
                    w,
                    h,
                    "No Media",
                    TextVerticalAlign::Bottom,
                    "Cantarell",
                    11,
                    "#a1a1aa",
                    0.0,
                );
            }
            return true;
        }

        // Transport modes (0..=5)
        let min_d = w.min(h).max(24.0);
        let cx = w / 2.0;
        let cy = h / 2.0;
        let badge_r = min_d * 0.30;

        if self.mode_idx == 0 && is_playing {
            cr.set_source_rgba(ar, ag, ab, 0.32);
            cr.arc(cx, cy, badge_r * 1.12, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
            cr.set_source_rgba(ar, ag, ab, 0.95);
            cr.set_line_width((min_d * 0.028).clamp(1.5, 3.0));
            cr.arc(cx, cy, badge_r, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.stroke();
        } else {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.09);
            cr.arc(cx, cy, badge_r, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }

        draw_transport_glyph(
            cr,
            cx,
            cy,
            min_d * 0.18,
            self.mode_idx,
            is_playing,
            (0.97, 0.97, 1.0),
        );
        true
    }
}

fn format_time_us(us: i64) -> String {
    let total_secs = (us.max(0) / 1_000_000) as u64;
    let hrs = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hrs > 0 {
        format!("{}:{:02}:{:02}", hrs, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}

fn draw_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

fn draw_transport_glyph(
    cr: &cairo::Context,
    cx: f64,
    cy: f64,
    s: f64,
    mode_idx: usize,
    is_playing: bool,
    rgb: (f64, f64, f64),
) {
    cr.set_source_rgb(rgb.0, rgb.1, rgb.2);
    let draw_pause = mode_idx == 2 || (mode_idx == 0 && is_playing);
    let draw_play = mode_idx == 1 || (mode_idx == 0 && !is_playing);

    if draw_pause {
        let bw = s * 0.42;
        let gap = s * 0.32;
        let bh = s * 1.45;
        cr.rectangle(cx - gap / 2.0 - bw, cy - bh / 2.0, bw, bh);
        cr.rectangle(cx + gap / 2.0, cy - bh / 2.0, bw, bh);
        let _ = cr.fill();
    } else if draw_play {
        let ox = s * 0.14;
        cr.new_path();
        cr.move_to(cx - s * 0.65 + ox, cy - s * 0.80);
        cr.line_to(cx + s * 0.85 + ox, cy);
        cr.line_to(cx - s * 0.65 + ox, cy + s * 0.80);
        cr.close_path();
        let _ = cr.fill();
    } else if mode_idx == 3 {
        cr.new_path();
        cr.move_to(cx - s * 0.82, cy - s * 0.72);
        cr.line_to(cx + s * 0.32, cy);
        cr.line_to(cx - s * 0.82, cy + s * 0.72);
        cr.close_path();
        let _ = cr.fill();
        cr.rectangle(cx + s * 0.42, cy - s * 0.72, s * 0.34, s * 1.44);
        let _ = cr.fill();
    } else if mode_idx == 4 {
        cr.rectangle(cx - s * 0.76, cy - s * 0.72, s * 0.34, s * 1.44);
        let _ = cr.fill();
        cr.new_path();
        cr.move_to(cx + s * 0.82, cy - s * 0.72);
        cr.line_to(cx - s * 0.32, cy);
        cr.line_to(cx + s * 0.82, cy + s * 0.72);
        cr.close_path();
        let _ = cr.fill();
    } else if mode_idx == 5 {
        let sq = s * 1.25;
        cr.rectangle(cx - sq / 2.0, cy - sq / 2.0, sq, sq);
        let _ = cr.fill();
    }
}

fn paint_cached_art(
    cr: &cairo::Context,
    art: &CachedArt,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
) {
    let stride = art.width * 4;
    if art.width <= 0
        || art.height <= 0
        || art.bgra_premul.len() < (stride * art.height) as usize
    {
        return;
    }
    if let Ok(surf) = cairo::ImageSurface::create_for_data(
        art.bgra_premul.clone(),
        cairo::Format::ARgb32,
        art.width,
        art.height,
        stride,
    ) {
        let _ = cr.save();
        if radius > 0.0 {
            draw_rounded_rect(cr, x, y, w, h, radius);
            cr.clip();
        } else {
            cr.rectangle(x, y, w, h);
            cr.clip();
        }
        let sx = w / art.width as f64;
        let sy = h / art.height as f64;
        let scale = sx.max(sy);
        let dw = art.width as f64 * scale;
        let dh = art.height as f64 * scale;
        let ox = x + (w - dw) / 2.0;
        let oy = y + (h - dh) / 2.0;
        cr.translate(ox, oy);
        cr.scale(scale, scale);
        let _ = cr.set_source_surface(&surf, 0.0, 0.0);
        let _ = cr.paint();
        let _ = cr.restore();
    }
}

fn variant_to_i64(v: &glib::Variant) -> Option<i64> {
    v.get::<i64>()
        .or_else(|| v.get::<u64>().map(|x| x as i64))
        .or_else(|| v.get::<i32>().map(|x| x as i64))
        .or_else(|| v.get::<u32>().map(|x| x as i64))
        .or_else(|| v.get::<f64>().map(|x| x as i64))
}

fn poll_mpris_players() -> Vec<MprisPlayerInfo> {
    let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) else {
        return Vec::new();
    };
    let Ok(reply) = conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames",
        None,
        Some(VariantTy::new("(as)").unwrap()),
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    ) else {
        return Vec::new();
    };

    let mut players = Vec::new();
    let names_var = reply.child_value(0);
    for i in 0..names_var.n_children() {
        let Some(bus_name) = names_var.child_value(i).str().map(|s| s.to_string()) else {
            continue;
        };
        if !bus_name.starts_with("org.mpris.MediaPlayer2.") {
            continue;
        }

        let mut info = MprisPlayerInfo {
            bus_name: bus_name.clone(),
            identity: bus_name
                .trim_start_matches("org.mpris.MediaPlayer2.")
                .split('.')
                .next()
                .unwrap_or("Player")
                .to_string(),
            status: "Stopped".to_string(),
            volume: 1.0,
            rate: 1.0,
            polled_at: Instant::now(),
            ..Default::default()
        };

        // Read Identity
        if let Ok(id_rep) = conn.call_sync(
            Some(&bus_name),
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.mpris.MediaPlayer2", "Identity").to_variant()),
            Some(VariantTy::new("(v)").unwrap()),
            gio::DBusCallFlags::NONE,
            500,
            gio::Cancellable::NONE,
        ) {
            if let Some(id_str) = id_rep
                .child_value(0)
                .as_variant()
                .and_then(|v| v.str().map(|s| s.to_string()))
            {
                if !id_str.is_empty() {
                    info.identity = id_str;
                }
            }
        }

        // Read Player Properties (PlaybackStatus, Metadata, Volume, Rate, Position)
        if let Ok(props_rep) = conn.call_sync(
            Some(&bus_name),
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties",
            "GetAll",
            Some(&("org.mpris.MediaPlayer2.Player",).to_variant()),
            Some(VariantTy::new("(a{sv})").unwrap()),
            gio::DBusCallFlags::NONE,
            600,
            gio::Cancellable::NONE,
        ) {
            let dict = props_rep.child_value(0);
            for j in 0..dict.n_children() {
                let entry = dict.child_value(j);
                let key = entry.child_value(0).str().unwrap_or("").to_string();
                let Some(val) = entry.child_value(1).as_variant() else {
                    continue;
                };
                match key.as_str() {
                    "PlaybackStatus" => {
                        if let Some(s) = val.str() {
                            info.status = s.to_string();
                        }
                    }
                    "Volume" => {
                        if let Some(v) = val.get::<f64>() {
                            info.volume = v.clamp(0.0, 1.5);
                        }
                    }
                    "Rate" => {
                        if let Some(r) = val.get::<f64>() {
                            if r > 0.0 {
                                info.rate = r;
                            }
                        }
                    }
                    "Position" => {
                        if let Some(p) = variant_to_i64(&val) {
                            info.position_us = p.max(0);
                        }
                    }
                    "Metadata" => {
                        for k in 0..val.n_children() {
                            let m_entry = val.child_value(k);
                            let mk = m_entry.child_value(0).str().unwrap_or("").to_string();
                            let Some(mv) = m_entry.child_value(1).as_variant() else {
                                continue;
                            };
                            match mk.as_str() {
                                "xesam:title" => {
                                    if let Some(s) = mv.str() {
                                        info.title = s.to_string();
                                    }
                                }
                                "xesam:album" => {
                                    if let Some(s) = mv.str() {
                                        info.album = s.to_string();
                                    }
                                }
                                "mpris:artUrl" => {
                                    if let Some(s) = mv.str() {
                                        info.art_url = s.to_string();
                                    }
                                }
                                "mpris:length" => {
                                    if let Some(len) = variant_to_i64(&mv) {
                                        info.length_us = len.max(0);
                                    }
                                }
                                "xesam:artist" => {
                                    if let Some(s) = mv.str() {
                                        info.artist = s.to_string();
                                    } else if mv.n_children() > 0 {
                                        let mut artists = Vec::new();
                                        for a in 0..mv.n_children() {
                                            if let Some(s) = mv.child_value(a).str() {
                                                if !s.is_empty() {
                                                    artists.push(s.to_string());
                                                }
                                            }
                                        }
                                        info.artist = artists.join(", ");
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Explicitly query `Position` per MPRIS2 spec (Spotify, Chromium, Music Assistant often require Properties.Get)
        if let Ok(pos_rep) = conn.call_sync(
            Some(&bus_name),
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.mpris.MediaPlayer2.Player", "Position").to_variant()),
            Some(VariantTy::new("(v)").unwrap()),
            gio::DBusCallFlags::NONE,
            400,
            gio::Cancellable::NONE,
        ) {
            if let Some(val) = pos_rep.child_value(0).as_variant() {
                if let Some(p) = variant_to_i64(&val) {
                    info.position_us = p.max(0);
                    info.polled_at = Instant::now();
                }
            }
        }

        players.push(info);
    }

    players
}

fn call_mpris_method(bus_name: &str, method: &str) {
    if let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        let _ = conn.call_sync(
            Some(bus_name),
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
            method,
            None,
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        );
    }
}

fn call_mpris_seek(bus_name: &str, offset_us: i64) {
    if let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        let _ = conn.call_sync(
            Some(bus_name),
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
            "Seek",
            Some(&(offset_us,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        );
    }
}

fn set_mpris_volume(bus_name: &str, vol: f64) {
    if let Ok(conn) = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        let val = vol.to_variant();
        let _ = conn.call_sync(
            Some(bus_name),
            "/org/mpris/MediaPlayer2",
            "org.freedesktop.DBus.Properties",
            "Set",
            Some(&("org.mpris.MediaPlayer2.Player", "Volume", val).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        );
    }
}

/// Fetches album art from `file://`, `http://` (e.g. local Music Assistant), or `https://`
/// and decodes it to a crisp 200x200 premultiplied BGRA32 buffer for direct Cairo painting.
fn fetch_and_decode_art(url: &str) -> Option<CachedArt> {
    let bytes: Vec<u8> = if let Some(path_raw) = url.strip_prefix("file://") {
        let decoded_path = percent_decode(path_raw);
        std::fs::read(decoded_path).ok()?
    } else if url.starts_with("http://") || url.starts_with("https://") {
        let cache_dir = glib::user_cache_dir().join("ballista").join("album_art");
        let _ = std::fs::create_dir_all(&cache_dir);
        let hash = fnv1a_hash(url);
        let cache_file: PathBuf = cache_dir.join(format!("{hash:016x}.img"));
        if cache_file.exists() {
            std::fs::read(&cache_file).ok()?
        } else {
            let out = std::process::Command::new("curl")
                .args(["-sSL", "--max-time", "5", "--fail", url])
                .output()
                .ok()?;
            if !out.status.success() || out.stdout.is_empty() {
                return None;
            }
            let _ = std::fs::write(&cache_file, &out.stdout);
            out.stdout
        }
    } else if std::path::Path::new(url).exists() {
        std::fs::read(url).ok()?
    } else {
        return None;
    };

    let dyn_img = image::load_from_memory(&bytes).ok()?;
    let resized = dyn_img.resize_to_fill(200, 200, image::imageops::FilterType::Lanczos3);
    let rgba = resized.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut bgra = Vec::with_capacity((w * h * 4) as usize);
    for px in rgba.pixels() {
        let a = px[3] as u32;
        let r = (px[0] as u32 * a / 255) as u8;
        let g = (px[1] as u32 * a / 255) as u8;
        let b = (px[2] as u32 * a / 255) as u8;
        bgra.push(b);
        bgra.push(g);
        bgra.push(r);
        bgra.push(px[3]);
    }
    Some(CachedArt {
        width: w as i32,
        height: h as i32,
        bgra_premul: bgra,
    })
}

fn fnv1a_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex_str, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
