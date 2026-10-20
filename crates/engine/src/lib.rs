//! The launcher engine: config, cache, downloader, java, accounts, minecraft
//! providers, and the server/instance stores with their launch pipeline.
//! Daemon-only domain logic; front-ends reach it over the socket, not by
//! linking it.

mod accounts;
mod backup;
mod cache;
mod cancel;
mod checksum;
mod config;
mod console;
mod content;
mod download;
mod engine;
mod error;
mod extract;
mod files;
pub mod host;
mod instances;
mod java;
mod jvm;
mod keys;
mod minecraft;
mod net;
mod plugins;
mod process;
mod properties;
mod recent;
mod reclaim;
mod registry;
mod schema;
mod servers;
mod signature;
mod skins;
mod sync;
mod tasks;
mod transfer;
mod update;
mod usage;
mod version;

pub use accounts::{Accounts, LoginChallenge, ReauthRequired};
pub use cache::{Cache, CacheEntry, CacheUsage};
pub use cancel::{is_cancelled, Cancel, Job};
pub use config::{Config, ConfigError, Settings};
pub use content::provider::{ContentProvider, LocalFile, UrlRef};
pub use content::Content;
pub use download::Downloader;
pub use engine::{
    Engine, EntryRef, ExportOutcome, ImportOutcome, LaunchRequest, ModpackOutcome,
    ServerCreateSpec, ServerListWrite, ServerUpdateSpec, VersionChange,
};
pub use error::error_info;
pub use files::FileTree;
pub use instances::{InstanceRecord, Instances};
pub use java::{Java, JavaInstallOutcome};
pub use jvm::JavaSettings;
pub use minecraft::launch::LaunchPlan;
pub use minecraft::profile::{
    AssetIndex, InstanceProfile, Library, Native, ServerJar, ServerProfile,
};
pub use minecraft::{
    FlavorInfo, FlavorProvider, Loads, Minecraft, ResolveRequest, Side, REQUIRED_JAVA_MAJORS,
};
pub use net::Network;
pub use plugins::{
    CallChain, CallError, ChainError, HostCalls, HostError, LoadError, LoadedPlugin, PluginRecord,
    PluginRuntime, PluginStats, Plugins, EVENT_TIMEOUT, HOOK_TIMEOUT, MAX_CHAIN_DEPTH,
    PROVIDER_TIMEOUT, UI_TIMEOUT,
};
pub use process::{ExitObserver, ProcessEvents, ProcessSupervisor, StartError};
pub use schema::notices as quarantined;
pub use servers::{RconConfig, ServerPhase, ServerRecord, Servers};
pub use skins::Skins;
pub use sync::Sync;
pub use tasks::Schedule;
pub use update::Update;
