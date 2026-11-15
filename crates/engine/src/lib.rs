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
pub use java::{Java, JavaInstallOutcome, JavaPackage, JavaProvider, JavaTarget};
pub use jvm::JavaSettings;
pub use minecraft::launch::LaunchPlan;
pub use minecraft::profile::{
    AssetIndex, InstanceProfile, Library, Native, ServerJar, ServerLaunch, ServerProfile,
    ServerRuntime,
};
pub use minecraft::{
    FlavorInfo, FlavorProvider, InstallRequest, JavaInstall, Loads, Minecraft, ResolveRequest,
    Side, REQUIRED_JAVA_MAJORS,
};
pub use net::Network;
pub use plugins::{
    approval_digest, build_task_id, command_args, find_command, is_official_signer, key_id_of,
    owner_of, pin_on_path, refuse_official_key, resolve_on_path, revocation, target_triple,
    unregister_providers, verify_build, verify_index, Approver, BeforeLaunch, BundleRefusal,
    CallChain, CallContext, CallError, ChainError, CommandRun, Contributions, GuestError, Hook,
    HookOutcome, Hooks, HostCalls, Inspection, LaunchPatch, Listings, LoadError, LoadedPlugin,
    ManagedJava, MessagesError, Mount, Origin, Participant, PluginContentProvider,
    PluginFlavorProvider, PluginJavaProvider, PluginPack, PluginPackage, PluginRecord,
    PluginRuntime, PluginSettings, PluginStats, Plugins, Pushed, Remote, RepoError, Resolver,
    Roster, Seeded, SettingsError, Staged, UpdateOutcome, UpdateRequest, CANCEL_GRACE,
    COMMAND_TIMEOUT, EVENT_TIMEOUT, HOOK_TIMEOUT, MAX_ASSET_BYTES, MAX_CHAIN_DEPTH,
    MAX_DOWNLOAD_BYTES, PROVIDER_TIMEOUT, UI_TIMEOUT,
};
#[cfg(windows)]
pub use process::run_relay;
pub use process::{ExitObserver, ProcessEvents, ProcessSupervisor, StartError, StdinError};
pub use roster::{RegisterError, Standing};
pub use schema::notices as quarantined;
pub use servers::{RconConfig, ServerPhase, ServerRecord, Servers};
pub use skins::Skins;
pub use sync::Sync;
pub use tasks::Schedule;
pub use update::Update;
pub use usage::dir_size;
