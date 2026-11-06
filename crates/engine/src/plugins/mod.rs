//! Installed plugins: one directory each under `<data_home>/plugins/`, holding
//! the extracted `package/`, the plugin's private `data/` and its `plugin.json`
//! record. The directory scan is the registry. The [`PluginRuntime`] runs an enabled
//! plugin's component sandboxed ([0091](../../../../docs/decisions/0091-plugins-are-wasm-components.md)).

mod cache;
mod chain;
mod command;
mod imports;
mod install;
mod output;
mod remote;
mod runtime;
mod store;

pub use chain::{CallChain, ChainError, MAX_CHAIN_DEPTH};
pub use command::{command_args, find_command};
pub use imports::{CommandRun, HostCalls, HostError};
pub use install::Inspection;
pub use remote::{Remote, Staged};
pub use runtime::{
    CallError, LoadError, LoadedPlugin, PluginRuntime, PluginStats, CANCEL_GRACE, COMMAND_TIMEOUT,
    EVENT_TIMEOUT, HOOK_TIMEOUT, PROVIDER_TIMEOUT, UI_TIMEOUT,
};
pub use store::{PluginPackage, PluginRecord, Plugins};
