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
mod content;
mod download;
mod engine;
mod error;
mod instances;
mod java;
mod minecraft;
mod net;
mod process;
mod recent;
mod reclaim;
mod registry;
mod schema;
mod servers;
mod signature;
mod skins;
mod sync;
mod transfer;
mod update;
mod usage;
mod version;

pub use accounts::{Accounts, LoginChallenge, ReauthRequired};
pub use backup::BackupSettings;
pub use cache::{Cache, CacheEntry, CacheUsage};
pub use cancel::{is_cancelled, Cancel, Job};
pub use config::{Config, ConfigError, Settings};
pub use content::provider::{ContentProvider, UrlRef};
pub use content::Content;
pub use download::Downloader;
pub use engine::{
    Engine, EntryRef, ExportOutcome, ImportOutcome, LaunchRequest, ModpackOutcome,
    ServerCreateSpec, ServerListWrite, ServerUpdateSpec,
};
pub use error::error_info;
pub use instances::{InstanceRecord, Instances};
pub use java::{Java, JavaInstallOutcome};
pub use minecraft::launch::{JavaSettings, LaunchPlan};
pub use minecraft::{
    InstanceProvider, Loads, Minecraft, ResolveRequest, ServerProvider, REQUIRED_JAVA_MAJORS,
};
pub use net::Network;
pub use process::{ExitObserver, ProcessEvents, ProcessSupervisor, StartError};
pub use schema::notices as quarantined;
pub use servers::{RconConfig, ServerPhase, ServerRecord, Servers};
pub use skins::Skins;
pub use sync::Sync;
pub use update::Update;
