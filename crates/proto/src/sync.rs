//! Shared settings across instances: a closed catalogue of the files Minecraft
//! writes that are worth keeping the same everywhere. Each unit is turned on by
//! itself, seeded from an instance the user names, and each instance may keep
//! any unit — or any individual option key — to itself.
//!
//! Everything here is copied and merged, never linked.
//!
//! Sync is instance-only: a server's configuration is per-server
//! infrastructure, managed through `server.config.*` and `server.properties`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SyncUnit {
    /// `options.txt`, merged key by key.
    #[default]
    Options,
    /// `servers.dat`, merged entry by entry.
    Servers,
    /// `command_history.txt`, merged as a union.
    Commands,
    /// `hotbar.nbt`, whole-file.
    Hotbars,
    /// The instances' `screenshots/` folders, read as one listing. Nothing is
    /// copied, so this unit has no shared copy and no agreement.
    Screenshots,
    /// Which resource packs the instances hold, installed through the content
    /// pool wherever they fit.
    ResourcePacks,
}

impl SyncUnit {
    /// The subject of every warning and status line about the unit, so the
    /// wording exists once.
    pub fn label(self) -> &'static str {
        match self {
            SyncUnit::Options => "the game options",
            SyncUnit::Servers => "the multiplayer list",
            SyncUnit::Commands => "the command history",
            SyncUnit::Hotbars => "the creative hotbars",
            SyncUnit::Screenshots => "the screenshots",
            SyncUnit::ResourcePacks => "the resource packs",
        }
    }
}

impl std::fmt::Display for SyncUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncUnitConfig {
    pub unit: SyncUnit,
    pub enabled: bool,
    /// The instance the shared copy was seeded from; empty when it started from
    /// nothing.
    pub seeded_from: String,
}

/// The catalogue and where the shared copies live. A unit is off until it is
/// enabled with a source, so there is no launcher-wide switch.
///
/// `unsynced` is the `options.txt` keys no instance shares. It sits beside the
/// units because only the options unit has keys.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncConfig {
    pub shared_dir: PathBuf,
    pub units: Vec<SyncUnitConfig>,
    pub unsynced: BTreeSet<String>,
}

/// What one instance does differently from the catalogue.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncOverrides {
    pub excluded: BTreeSet<SyncUnit>,
    pub unsynced: BTreeSet<String>,
}

impl SyncOverrides {
    pub fn is_empty(&self) -> bool {
        self.excluded.is_empty() && self.unsynced.is_empty()
    }

    pub fn shares(&self, unit: SyncUnit) -> bool {
        !self.excluded.contains(&unit)
    }
}

/// One pack the instances share: an identity, never a file and never a version.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct SharedPack {
    pub kind: crate::content::ContentKind,
    pub source: String,
    /// The project this is an install of; empty for a pack that came from a
    /// file, which is then known by its filename alone.
    pub project: String,
    pub title: String,
    pub filename: String,
    pub enabled: bool,
}

impl SharedPack {
    pub fn identity(&self) -> String {
        match self.project.is_empty() {
            true => format!("{}:file:{}", self.kind, self.filename),
            false => format!("{}:{}:{}", self.kind, self.source, self.project),
        }
    }

    /// Whether the pack answers to what a person typed: its project, its file
    /// or its title.
    pub fn answers_to(&self, reference: &str) -> bool {
        let reference = reference.trim();
        [&self.project, &self.filename, &self.title]
            .iter()
            .any(|field| !field.is_empty() && field.eq_ignore_ascii_case(reference))
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncPacksResult {
    pub packs: Vec<SharedPack>,
}

pub struct SyncPacks;
impl Contract for SyncPacks {
    const CHANNEL: &'static str = "sync.packs.get";
    type Params = Empty;
    type Result = SyncPacksResult;
}

/// Reaches each instance at its next pass; nothing is moved here.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncPackSetParams {
    /// Project, filename or title.
    pub pack: String,
    pub enabled: bool,
}

pub struct SyncPackSet;
impl Contract for SyncPackSet {
    const CHANNEL: &'static str = "sync.packs.set";
    const CHANGES: Option<Scope> = Some(Scope::Sync);
    type Params = SyncPackSetParams;
    type Result = SyncPacksResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncPackRemoveParams {
    pub pack: String,
}

/// Drops it from the library; every instance that has it loses it at its next
/// pass, and one that had already removed it stays as it is.
pub struct SyncPackRemove;
impl Contract for SyncPackRemove {
    const CHANNEL: &'static str = "sync.packs.remove";
    const CHANGES: Option<Scope> = Some(Scope::Sync);
    type Params = SyncPackRemoveParams;
    type Result = SyncPacksResult;
}

pub struct SyncGet;
impl Contract for SyncGet {
    const CHANNEL: &'static str = "sync.get";
    type Params = Empty;
    type Result = SyncConfig;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncSource {
    pub id: String,
    pub name: String,
    /// Whether the instance holds the unit's file at all.
    pub present: bool,
    pub modified_unix: Option<i64>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncSourcesParams {
    pub unit: SyncUnit,
}

/// What a front-end fills its source picker from, before enabling a unit.
pub struct SyncSources;
impl Contract for SyncSources {
    const CHANNEL: &'static str = "sync.sources";
    type Params = SyncSourcesParams;
    type Result = Vec<SyncSource>;
}

/// Turn a unit on, seeded from one instance's copy. `source` may be empty only
/// when at most one instance holds the file.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncEnableParams {
    pub unit: SyncUnit,
    /// Instance name or id.
    pub source: String,
}

pub struct SyncEnable;
impl Contract for SyncEnable {
    const CHANNEL: &'static str = "sync.enable";
    const CHANGES: Option<Scope> = Some(Scope::Sync);
    type Params = SyncEnableParams;
    type Result = SyncConfig;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncDisableParams {
    pub unit: SyncUnit,
}

/// Every instance keeps the copy it has; nothing is moved or deleted.
pub struct SyncDisable;
impl Contract for SyncDisable {
    const CHANNEL: &'static str = "sync.disable";
    const CHANGES: Option<Scope> = Some(Scope::Sync);
    type Params = SyncDisableParams;
    type Result = SyncConfig;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncKeysParams {
    pub unsynced: BTreeSet<String>,
}

pub struct SyncKeys;
impl Contract for SyncKeys {
    const CHANNEL: &'static str = "sync.options.keys";
    type Params = SyncKeysParams;
    type Result = SyncConfig;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncOption {
    pub key: String,
    pub value: String,
    /// False when the key is pinned local launcher-wide.
    pub synced: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncOptionsResult {
    pub options: Vec<SyncOption>,
}

pub struct SyncOptionsGet;
impl Contract for SyncOptionsGet {
    const CHANNEL: &'static str = "sync.options.get";
    type Params = Empty;
    type Result = SyncOptionsResult;
}

/// Edits the shared copy; it reaches each instance at its next launch.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SyncOptionSetParams {
    pub key: String,
    pub value: String,
}

pub struct SyncOptionSet;
impl Contract for SyncOptionSet {
    const CHANNEL: &'static str = "sync.options.set";
    const CHANGES: Option<Scope> = Some(Scope::Sync);
    type Params = SyncOptionSetParams;
    type Result = SyncOptionsResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum UnitState {
    Synced,
    /// Shared, but the two copies have not met yet.
    #[default]
    Pending,
    /// The instance keeps its own copy.
    Overridden,
    /// Off in the catalogue.
    Off,
    /// The instance's game version is older than the unit's file, or older
    /// than the spelling the other instances write.
    Unsupported,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UnitStatus {
    pub unit: SyncUnit,
    pub state: UnitState,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceSyncStatus {
    pub id: String,
    pub name: String,
    pub units: Vec<UnitStatus>,
    pub unsynced: BTreeSet<String>,
}

pub struct SyncStatus;
impl Contract for SyncStatus {
    const CHANNEL: &'static str = "sync.status";
    type Params = Empty;
    type Result = Vec<InstanceSyncStatus>;
}

/// `shared` unset follows the catalogue, which is how a front-end tells "this
/// instance opted out" from "this unit is off".
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceSyncUnitParams {
    /// Instance name or id.
    pub instance: String,
    pub unit: SyncUnit,
    pub shared: Option<bool>,
}

pub struct InstanceSyncUnit;
impl Contract for InstanceSyncUnit {
    const CHANNEL: &'static str = "instance.sync.unit";
    type Params = InstanceSyncUnitParams;
    type Result = InstanceSyncStatus;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceSyncKeysParams {
    /// Instance name or id.
    pub instance: String,
    pub unsynced: BTreeSet<String>,
}

pub struct InstanceSyncKeys;
impl Contract for InstanceSyncKeys {
    const CHANNEL: &'static str = "instance.sync.keys";
    type Params = InstanceSyncKeysParams;
    type Result = InstanceSyncStatus;
}
