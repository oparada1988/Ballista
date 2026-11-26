#![allow(dead_code)]

pub mod animation;
pub mod autostart;
pub mod hardware;
pub mod plugin_bridge;
pub mod plugin_manager;
pub mod power;
pub mod tray;

#[allow(unused_imports)]
pub use animation::GifAnimation;
pub use hardware::{ConnectedDevice, HardwareEvent, HardwareManager};
#[allow(unused_imports)]
pub use plugin_bridge::PluginBridge;
#[allow(unused_imports)]
pub use plugin_manager::{ActionConfigRow, ActionSettingType, DiscoveredPlugin, InstalledApp, PluginManager};
pub use power::{PowerEvent, PowerMonitor};
pub use tray::{TrayCommand, TrayController};
