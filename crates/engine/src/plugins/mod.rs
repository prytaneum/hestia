//! Installed plugins: one directory each under `<data_home>/plugins/`, holding
//! the extracted `package/`, the plugin's private `data/` and its `plugin.json`
//! record. The directory scan is the registry. The [`PluginRuntime`] runs an enabled
//! plugin's component sandboxed ([0091](../../../../docs/decisions/0091-plugins-are-wasm-components.md)).

mod asset;
mod cache;
mod catalogue;
mod chain;
mod command;
mod context;
mod dev;
mod hooks;
mod http;
mod imports;
mod index;
mod install;
mod ipc;
mod official;
mod output;
mod patch;
mod process;
mod program;
mod providers;
mod remote;
mod repos;
mod repositories;
mod runtime;
mod settings;
mod store;
#[cfg(test)]
mod testkit;

pub use asset::MAX_ASSET_BYTES;
pub use catalogue::Listings;
pub(crate) use catalogue::{build as build_catalogue, pick, Installed, Pick, Source};
pub use chain::{CallChain, ChainError, MAX_CHAIN_DEPTH};
pub use command::{command_args, find_command};
pub use context::{CallContext, Mount};
pub use hooks::{
    BeforeContentInstall, BeforeLaunch, BeforeServerStart, Hook, HookOutcome, Hooks, Participant,
    Roster,
};
pub use imports::{CommandRun, HostCalls, HostError};
pub(crate) use index::{declared_key, expand, open};
pub use install::Inspection;
pub use official::{is_official_signer, refuse_official_key};
pub use patch::LaunchPatch;
pub use process::owner_of;
pub use program::{resolve_on_path, target_triple, ManagedJava};
pub use providers::{
    unregister as unregister_providers, Contributions, PluginContentProvider, PluginFlavorProvider,
    PluginFormat, PluginJavaProvider,
};
pub use remote::{Origin, Remote, Staged};
pub use repos::{key_id_of, revocation, verify_build, verify_index, RepoError};
pub(crate) use repositories::{cache_index, cached_index, Repositories, RepositoryEntry};
pub use runtime::{
    CallError, LoadError, LoadedPlugin, PluginRuntime, PluginStats, CANCEL_GRACE, COMMAND_TIMEOUT,
    EVENT_TIMEOUT, HOOK_TIMEOUT, PROVIDER_TIMEOUT, UI_TIMEOUT,
};
pub use settings::{PluginSettings, SettingsError};
pub use store::{PluginPackage, PluginRecord, Plugins};
