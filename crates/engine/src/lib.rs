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
mod edition;
mod engine;
mod error;
mod extract;
mod files;
mod hold;
pub mod host;
mod icon;
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
mod roster;
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
mod vouch;

pub use accounts::{Accounts, LoginChallenge, ReauthRequired};
pub use cache::{Cache, CacheEntry, CacheUsage};
pub use cancel::{is_cancelled, Cancel, Cancelled, Job};
pub use config::{Config, ConfigError, Settings};
pub use content::provider::{ContentProvider, InstalledFile, LatestQuery, LocalFile, UrlRef};
pub use content::Content;
pub use download::Downloader;
pub use edition::{config_rules, has_console, process_io, ProcessIo};
pub use engine::{
    Checked, Engine, EntryRef, ExportOutcome, ImportOutcome, InstanceUpdateSpec, LaunchRequest,
    ModpackOutcome, Repin, ServerCreateSpec, ServerListWrite, ServerUpdateSpec, Swept,
    VersionChange,
};
pub use error::error_info;
pub use files::FileTree;
pub use instances::{InstanceRecord, Instances};
pub use java::{Adoptium, Java, JavaInstallOutcome, JavaPackage, JavaProvider, JavaTarget};
pub use jvm::JavaSettings;
pub use minecraft::launch::LaunchPlan;
pub use minecraft::profile::{
    AssetIndex, InstanceProfile, Library, Native, ServerJar, ServerLaunch, ServerProfile,
    ServerRuntime,
};
pub use minecraft::{
    FlavorInfo, FlavorProvider, InstallRequest, JavaInstall, Loaders, Loads, Minecraft,
    ResolveRequest, Side, BOTH_SIDES, CLIENT_ONLY, REQUIRED_JAVA_MAJORS, SERVER_ONLY,
};
pub use net::Network;
pub use plugins::{Builtin, Busy, Descriptor, PluginError, Plugins};
#[cfg(windows)]
pub use process::run_relay;
pub use process::{ExitObserver, ProcessEvents, ProcessSupervisor, StartError, StdinError};
pub use roster::{RegisterError, Standing};
pub use schema::notices as quarantined;
pub use servers::{RconConfig, ServerPhase, ServerRecord, Servers};
pub use skins::Skins;
pub use sync::Sync;
pub use tasks::Schedule;
pub use transfer::archive::Reader;
pub use transfer::{PackProvider, Vetted};
pub use update::Update;
pub use usage::dir_size;
pub use vouch::{Hosts, Policy};
