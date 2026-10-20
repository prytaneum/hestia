//! Installed plugins: one directory each under `<data_home>/plugins/`, holding
//! the extracted `package/`, the plugin's private `data/` and its `plugin.json`
//! record. The directory scan is the registry.

mod store;

pub use store::{PluginRecord, Plugins};
