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

impl Default for DeckSnapshot {
    fn default() -> Self {
        Self {
            available_profiles: Vec::new(),
            active_profile_name: "Default Profile".to_string(),
            current_page_idx: 0,
            total_pages: 1,
            current_brightness: 80,
        }
    }
}

#[derive(Default)]
struct DeviceRuntime {
    instances: HashMap<String, NativeSlotInstance>,
    deck: DeckSnapshot,
}

struct NativeHostInner {
    plugins: Vec<Box<dyn BallistaPlugin>>,
    /// Per-device isolated runtimes keyed by `ConnectedDevice::stable_key()` so EVERY connected
    /// Stream Deck ticks, renders, and executes actions simultaneously regardless of GUI selection.
    decks: HashMap<String, DeviceRuntime>,
}

impl NativePluginHost {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(NativeHostInner {
                plugins: Vec::new(),
                decks: HashMap::new(),
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

    /// Drops runtimes for any disconnected devices not in `active_device_keys`.
    pub fn retain_devices(&self, active_device_keys: &HashSet<String>) {
        let mut guard = self.inner.lock().unwrap();
        guard.decks.retain(|k, _| active_device_keys.contains(k));
    }

    pub fn set_deck_state(
        &self,
        device_key: &str,
        available_profiles: Vec<String>,
        active_profile_name: String,
        current_page_idx: usize,
        total_pages: usize,
        current_brightness: u8,
    ) {
        let mut guard = self.inner.lock().unwrap();
        let rt = guard.decks.entry(device_key.to_string()).or_default();
        rt.deck.available_profiles = available_profiles;
        rt.deck.active_profile_name = active_profile_name;
        rt.deck.current_page_idx = current_page_idx;
        rt.deck.total_pages = total_pages.max(1);
        rt.deck.current_brightness = current_brightness.min(100);
    }

    pub fn set_current_brightness(&self, device_key: &str, brightness: u8) {
        let mut guard = self.inner.lock().unwrap();
        let rt = guard.decks.entry(device_key.to_string()).or_default();
        rt.deck.current_brightness = brightness.min(100);
    }

    #[allow(dead_code)]
    pub fn set_page_state(
        &self,
        device_key: &str,
        active_profile_name: &str,
        current_page_idx: usize,
        total_pages: usize,
    ) {
        let mut guard = self.inner.lock().unwrap();
        let rt = guard.decks.entry(device_key.to_string()).or_default();
        rt.deck.active_profile_name = active_profile_name.to_string();
        rt.deck.current_page_idx = current_page_idx;
        rt.deck.total_pages = total_pages.max(1);
    }

    /// Synchronizes all active native plugin slot instances for `device_key`'s active Page/Folder
    /// without disturbing any other connected Stream Deck's runtime instances.
    /// Reuses unchanged slot instances so live graphs, timers, and clocks never reset unnecessarily!
    pub fn sync_page(&self, device_key: &str, page: &Page) -> Vec<(String, Vec<HostCommand>)> {
        let mut guard = self.inner.lock().unwrap();
        let mut seen_slots: HashSet<String> = HashSet::new();
        let mut emitted: Vec<(String, Vec<HostCommand>)> = Vec::new();

        let deck = guard
            .decks
            .entry(device_key.to_string())
            .or_default()
            .deck
            .clone();

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
                .decks
                .get(device_key)
                .and_then(|rt| rt.instances.get(&slot))
                .map(|inst| {
                    inst.plugin_id == plugin_id
                        && inst.action_id == action_id
                        && inst.action_uuid == action_uuid
                })
                .unwrap_or(false);

            if same_instance {
                if let Some(inst) = guard
                    .decks
                    .get_mut(device_key)
                    .and_then(|rt| rt.instances.get_mut(&slot))
                {
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
                guard
                    .decks
                    .entry(device_key.to_string())
                    .or_default()
                    .instances
                    .insert(
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

        // Remove stale native instances not on THIS device's current page/folder
        if let Some(rt) = guard.decks.get_mut(device_key) {
            rt.instances.retain(|slot, _| seen_slots.contains(slot));
        }
        emitted
    }

    pub fn destroy_slot(&self, device_key: &str, slot: &str) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            rt.instances.remove(slot);
        }
    }

    pub fn has_slot(&self, device_key: &str, slot: &str) -> bool {
        let guard = self.inner.lock().unwrap();
        guard
            .decks
            .get(device_key)
            .map(|rt| rt.instances.contains_key(slot))
            .unwrap_or(false)
    }

    pub fn get_slot_settings(&self, device_key: &str, slot: &str) -> Option<HashMap<String, Value>> {
        let guard = self.inner.lock().unwrap();
        guard
            .decks
            .get(device_key)
            .and_then(|rt| rt.instances.get(slot))
            .map(|i| i.settings.clone())
    }

    pub fn update_slot_settings(
        &self,
        device_key: &str,
        slot: &str,
        updates: HashMap<String, Value>,
    ) -> Vec<HostCommand> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        Vec::new()
    }

    pub fn config_rows(&self, device_key: &str, slot: &str) -> Option<Vec<BridgeConfigRow>> {
        let guard = self.inner.lock().unwrap();
        let rt = guard.decks.get(device_key)?;
        let inst = rt.instances.get(slot)?;
        let ctx = ConfigContext {
            slot,
            surface: SlotSurface::from_slot(slot),
            settings: &inst.settings,
            available_profiles: &rt.deck.available_profiles,
            active_profile_name: &rt.deck.active_profile_name,
            current_page_idx: rt.deck.current_page_idx,
            total_pages: rt.deck.total_pages,
            current_brightness: rt.deck.current_brightness,
        };
        Some(inst.action.config_rows(&ctx))
    }

    pub fn on_config_changed(
        &self,
        device_key: &str,
        slot: &str,
        row_idx: usize,
        change: ConfigChange,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        (Vec::new(), None)
    }

    pub fn on_button_clicked(
        &self,
        device_key: &str,
        slot: &str,
        row_idx: usize,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        (Vec::new(), None)
    }

    pub fn handle_key_event(
        &self,
        device_key: &str,
        slot: &str,
        is_down: bool,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        (Vec::new(), None)
    }

    pub fn handle_dial_rotate(
        &self,
        device_key: &str,
        slot: &str,
        delta: i32,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        (Vec::new(), None)
    }

    pub fn handle_dial_press(
        &self,
        device_key: &str,
        slot: &str,
        is_down: bool,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        (Vec::new(), None)
    }

    pub fn handle_touch_tap(
        &self,
        device_key: &str,
        slot: &str,
        norm_x: f64,
        norm_y: f64,
    ) -> (Vec<HostCommand>, Option<HashMap<String, Value>>) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(rt) = guard.decks.get_mut(device_key) {
            let deck = rt.deck.clone();
            if let Some(inst) = rt.instances.get_mut(slot) {
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
        }
        (Vec::new(), None)
    }

    /// Runs `on_tick` for every active native instance across ALL connected devices
    /// whose `tick_interval()` has elapsed. Returns `(device_key, slot, commands)`.
    pub fn tick_due(&self) -> Vec<(String, String, Vec<HostCommand>)> {
        let mut guard = self.inner.lock().unwrap();
        let now = Instant::now();
        let mut results = Vec::new();

        for (device_key, rt) in guard.decks.iter_mut() {
            let deck = rt.deck.clone();
            for (slot, inst) in rt.instances.iter_mut() {
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
                            results.push((device_key.clone(), slot.clone(), ctx.commands));
                        }
                    }
                }
            }
        }
        results
    }

    /// Directly invokes a native action's surface-aware Cairo renderer for `(device_key, slot)`.
    /// Returns `true` if custom vector artwork was painted.
    pub fn render_slot_visual(
        &self,
        device_key: &str,
        slot: &str,
        cr: &cairo::Context,
        w: f64,
        h: f64,
    ) -> bool {
        let guard = self.inner.lock().unwrap();
        if let Some(inst) = guard
            .decks
            .get(device_key)
            .and_then(|rt| rt.instances.get(slot))
        {
            inst.action
                .render_visual_for_surface(cr, w, h, SlotSurface::from_slot(slot))
        } else {
            false
        }
    }
}
