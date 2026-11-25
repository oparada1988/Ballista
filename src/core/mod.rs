#![allow(dead_code)]

pub mod animation;
pub mod hardware;
pub mod plugin_bridge;
pub mod plugin_manager;

#[allow(unused_imports)]
pub use animation::GifAnimation;
pub use hardware::{ConnectedDevice, HardwareEvent, HardwareManager};
#[allow(unused_imports)]
pub use plugin_bridge::PluginBridge;
#[allow(unused_imports)]
pub use plugin_manager::{ActionConfigRow, ActionSettingType, DiscoveredPlugin, InstalledApp, PluginManager};
