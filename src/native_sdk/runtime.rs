use crate::core::plugin_bridge::BridgeConfigRow;
use crate::models::Page;
use crate::native_sdk::plugin::{
    ActionContext, ActionDescriptor, BallistaAction, BallistaPlugin, ConfigChange, ConfigContext,
    HostCommand, SlotSurface, TargetCaps,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

struct NativeSlotInstance {
    plugin_id: String,
    action_id: String,
    action_uuid: String,
    action: Box<dyn BallistaAction>,
    settings: HashMap<String, Value>,
    last_tick: Instant,
}

#[derive(Clone)]
pub struct NativePluginHost {
    inner: Arc<Mutex<NativeHostInner>>,
}

#[derive(Clone)]
struct DeckSnapshot {
    available_profiles: Vec<String>,
    active_profile_name: String,
    current_page_idx: usize,
    total_pages: usize,
    current_brightness: u8,
}

struct NativeHostInner {
    plugins: Vec<Box<dyn BallistaPlugin>>,
    instances: HashMap<String, NativeSlotInstance>,
    deck: DeckSnapshot,
}

impl NativePluginHost {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(NativeHostInner {
                plugins: Vec::new(),
                instances: HashMap::new(),
                deck: DeckSnapshot {
                    available_profiles: Vec::new(),
                    active_profile_name: "Default Profile".to_string(),
                    current_page_idx: 0,
                    total_pages: 1,
                    current_brightness: 80,
                },
            })),
        }
    }

    pub fn register_plugin(&self, plugin: Box<dyn BallistaPlugin>) {
        let mut guard = self.inner.lock().unwrap();
        guard.plugins.push(plugin);
    }

    pub fn is_native_plugin(&self, plugin_id: &str) -> bool {
        let guard = self.inner.lock().unwrap();
        guard.plugins.iter().any(|p| p.id() == plugin_id)
    }

    pub fn list_plugins(&self) -> Vec<(String, String, Vec<ActionDescriptor>)> {
        let guard = self.inner.lock().unwrap();
        guard
            .plugins
            .iter()
            .map(|p| (p.id().to_string(), p.name().to_string(), p.actions()))
            .collect()
    }

    pub fn find_descriptor(&self, plugin_id: &str, action_id: &str) -> Option<ActionDescriptor> {
        let guard = self.inner.lock().unwrap();
        guard
            .plugins
            .iter()
            .find(|p| p.id() == plugin_id)
            .and_then(|p| p.actions().into_iter().find(|a| a.id == action_id))
    }

    /// Returns the declared hardware surface capabilities for an action.
    /// External (StreamController) plugins default to `KEYS_AND_DIALS`.
    pub fn action_caps(&self, plugin_id: &str, action_id: &str) -> TargetCaps {
        self.find_descriptor(plugin_id, action_id)
            .map(|d| d.caps)
            .unwrap_or(TargetCaps::KEYS_AND_DIALS)
    }

    pub fn set_deck_state(
        &self,
        available_profiles: Vec<String>,
        active_profile_name: String,
        current_page_idx: usize,
        total_pages: usize,
        current_brightness: u8,
    ) {
        let mut guard = self.inner.lock().unwrap();
        guard.deck.available_profiles = available_profiles;
        guard.deck.active_profile_name = active_profile_name;
        guard.deck.current_page_idx = current_page_idx;
        guard.deck.total_pages = total_pages.max(1);
        guard.deck.current_brightness = current_brightness.min(100);
    }

    pub fn set_current_brightness(&self, brightness: u8) {
        let mut guard = self.inner.lock().unwrap();
        guard.deck.current_brightness = brightness.min(100);
    }

    pub fn set_page_state(&self, active_profile_name: &str, current_page_idx: usize, total_pages: usize) {
        let mut guard = self.inner.lock().unwrap();
        guard.deck.active_profile_name = active_profile_name.to_string();
        guard.deck.current_page_idx = current_page_idx;
        guard.deck.total_pages = total_pages.max(1);
    }

    /// Synchronizes all active native plugin slot instances with the currently visible Page (or Folder).
    /// Reuses unchanged slot instances so live graphs, timers, and clocks never reset unnecessarily!
    pub fn sync_page(&self, page: &Page) -> Vec<(String, Vec<HostCommand>)> {
        let mut guard = self.inner.lock().unwrap();
        let mut seen_slots: HashSet<String> = HashSet::new();
        let mut emitted: Vec<(String, Vec<HostCommand>)> = Vec::new();
        let deck = guard.deck.clone();

        let mut desired: Vec<(String, String, String, String, HashMap<String, Value>)> = Vec::new();
        for (k, act) in &page.keys {
            if guard.plugins.iter().any(|p| p.id() == act.plugin_id) {
                desired.push((
                    format!("key_{k}"),
                    act.plugin_id.clone(),
                    act.action_id.clone(),
                    act.uuid.clone(),
                    act.merged_settings_json(),
                ));
            }
        }
        for (d, act) in &page.dial_actions {
            if guard.plugins.iter().any(|p| p.id() == act.plugin_id) {
                desired.push((
                    format!("dial_{d}"),
                    act.plugin_id.clone(),
                    act.action_id.clone(),
                    act.uuid.clone(),
                    act.merged_settings_json(),
                ));
            }
        }

        for (slot, plugin_id, action_id, action_uuid, settings) in desired {
            seen_slots.insert(slot.clone());
            let same_instance = guard
                .instances
                .get(&slot)
                .map(|inst| {
                    inst.plugin_id == plugin_id
                        && inst.action_id == action_id
                        && inst.action_uuid == action_uuid
                })
                .unwrap_or(false);

            if same_instance {
                if let Some(inst) = guard.instances.get_mut(&slot) {
                    inst.settings = settings;
                    let mut ctx = ActionContext::new(
                        &slot,
                        &mut inst.settings,
                        &deck.available_profiles,
                        &deck.active_profile_name,
                        deck.current_page_idx,
                        deck.total_pages,
                        deck.current_brightness,
                    );
                    inst.action.on_load(&mut ctx);
                    let cmds = ctx.commands;
                    if !cmds.is_empty() {
                        emitted.push((slot, cmds));
                    }
                }
                continue;
            }

            let created = guard
                .plugins
                .iter()
                .find(|p| p.id() == plugin_id)
                .and_then(|p| p.create_action(&action_id));

            if let Some(mut action) = created {
                let mut settings_mut = settings;
                let mut ctx = ActionContext::new(
                    &slot,
                    &mut settings_mut,
                    &deck.available_profiles,
                    &deck.active_profile_name,
                    deck.current_page_idx,
                    deck.total_pages,
                    deck.current_brightness,
                );
                action.on_load(&mut ctx);
                let cmds = ctx.commands;
                guard.instances.insert(
                    slot.clone(),
                    NativeSlotInstance {
                        plugin_id,
                        action_id,
                        action_uuid,
                        action,
                        settings: settings_mut,
                        last_tick: Instant::now(),
                    },
                );
                if !cmds.is_empty() {
                    emitted.push((slot, cmds));
                }
            }
        }

        // Remove stale native instances not on the current page/folder
        guard.instances.retain(|slot, _| seen_slots.contains(slot));
        emitted
    }

    pub fn destroy_slot(&self, slot: &str) {
        let mut guard = self.inner.lock().unwrap();
        guard.instances.remove(slot);
    }

    pub fn has_slot(&self, slot: &str) -> bool {
        let guard = self.inner.lock().unwrap();
        guard.instances.contains_key(slot)
    }

    pub fn get_slot_settings(&self, slot: &str) -> Option<HashMap<String, Value>> {
        let guard = self.inner.lock().unwrap();
        guard.instances.get(slot).map(|i| i.settings.clone())
    }

    pub fn update_slot_settings(
        &self,
        slot: &str,
        updates: HashMap<String, Value>,
    ) -> Vec<HostCommand> {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            for (k, v) in updates {
                inst.settings.insert(k, v);
            }
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            inst.action.on_load(&mut ctx);
            return ctx.commands;
        }
        Vec::new()
    }

    pub fn config_rows(&self, slot: &str) -> Option<Vec<BridgeConfigRow>> {
        let guard = self.inner.lock().unwrap();
        let inst = guard.instances.get(slot)?;
        let ctx = ConfigContext {
            slot,
            surface: SlotSurface::from_slot(slot),
            settings: &inst.settings,
            available_profiles: &guard.deck.available_profiles,
            active_profile_name: &guard.deck.active_profile_name,
            current_page_idx: guard.deck.current_page_idx,
            total_pages: guard.deck.total_pages,
            current_brightness: guard.deck.current_brightness,
        };
        Some(inst.action.config_rows(&ctx))
    }

    pub fn on_config_changed(
        &self,
        slot: &str,
        row_idx: usize,
        change: ConfigChange,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            inst.action.on_config_changed(&mut ctx, row_idx, change);
            let cmds = ctx.commands;
            let updated = inst.settings.clone();
            return (cmds, Some(updated));
        }
        (Vec::new(), None)
    }

    pub fn on_button_clicked(
        &self,
        slot: &str,
        row_idx: usize,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            inst.action.on_button_clicked(&mut ctx, row_idx);
            let cmds = ctx.commands;
            let updated = inst.settings.clone();
            return (cmds, Some(updated));
        }
        (Vec::new(), None)
    }

    pub fn handle_key_event(
        &self,
        slot: &str,
        is_down: bool,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            if is_down {
                inst.action.on_key_down(&mut ctx);
            } else {
                inst.action.on_key_up(&mut ctx);
            }
            let cmds = ctx.commands;
            let updated = inst.settings.clone();
            return (cmds, Some(updated));
        }
        (Vec::new(), None)
    }

    pub fn handle_dial_rotate(
        &self,
        slot: &str,
        delta: i32,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            inst.action.on_dial_rotate(&mut ctx, delta);
            let cmds = ctx.commands;
            let updated = inst.settings.clone();
            return (cmds, Some(updated));
        }
        (Vec::new(), None)
    }

    pub fn handle_dial_press(
        &self,
        slot: &str,
        is_down: bool,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            if is_down {
                inst.action.on_dial_press(&mut ctx);
            } else {
                inst.action.on_dial_release(&mut ctx);
            }
            let cmds = ctx.commands;
            let updated = inst.settings.clone();
            return (cmds, Some(updated));
        }
        (Vec::new(), None)
    }

    pub fn handle_touch_tap(
        &self,
        slot: &str,
        norm_x: f64,
        norm_y: f64,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        if let Some(inst) = guard.instances.get_mut(slot) {
            let mut ctx = ActionContext::new(
                slot,
                &mut inst.settings,
                &deck.available_profiles,
                &deck.active_profile_name,
                deck.current_page_idx,
                deck.total_pages,
                deck.current_brightness,
            );
            inst.action
                .on_touch_tap(&mut ctx, norm_x.clamp(0.0, 1.0), norm_y.clamp(0.0, 1.0));
            let cmds = ctx.commands;
            let updated = inst.settings.clone();
            return (cmds, Some(updated));
        }
        (Vec::new(), None)
    }

    /// Runs `on_tick` for every active native instance whose `tick_interval()` has elapsed.
    pub fn tick_due(&self) -> Vec<(String, Vec<HostCommand>)> {
        let mut guard = self.inner.lock().unwrap();
        let deck = guard.deck.clone();
        let now = Instant::now();
        let mut results = Vec::new();

        for (slot, inst) in guard.instances.iter_mut() {
            if let Some(interval) = inst.action.tick_interval() {
                if now.duration_since(inst.last_tick) >= interval {
                    inst.last_tick = now;
                    let mut ctx = ActionContext::new(
                        slot,
                        &mut inst.settings,
                        &deck.available_profiles,
                        &deck.active_profile_name,
                        deck.current_page_idx,
                        deck.total_pages,
                        deck.current_brightness,
                    );
                    inst.action.on_tick(&mut ctx);
                    if !ctx.commands.is_empty() {
                        results.push((slot.clone(), ctx.commands));
                    }
                }
            }
        }
        results
    }

    /// Directly invokes a native action's surface-aware Cairo renderer for a slot (key_N or dial_N).
    /// Returns `true` if custom vector artwork was painted.
    pub fn render_slot_visual(&self, slot: &str, cr: &cairo::Context, w: f64, h: f64) -> bool {
        let guard = self.inner.lock().unwrap();
        if let Some(inst) = guard.instances.get(slot) {
            inst.action
                .render_visual_for_surface(cr, w, h, SlotSurface::from_slot(slot))
        } else {
            false
        }
    }
}
