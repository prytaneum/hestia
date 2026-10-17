//! The verbs a server and an instance share, one channel each with the side
//! named in the payload. What only one side has stays under `server.*` or
//! `instance.*`.

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::error::{EntryKind, EntryTarget};
use crate::keys::KeyRule;
use crate::minecraft::{ConfigEntry, FlavorsResult, GameVersion};
use crate::process::ProcessLogLine;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryFlavorsParams {
    pub kind: EntryKind,
}

pub struct EntryFlavors;
impl Contract for EntryFlavors {
    const CHANNEL: &'static str = "entry.flavors";
    type Params = EntryFlavorsParams;
    type Result = FlavorsResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryVersionsParams {
    pub kind: EntryKind,
    pub flavor: String,
}

pub struct EntryVersions;
impl Contract for EntryVersions {
    const CHANNEL: &'static str = "entry.versions";
    type Params = EntryVersionsParams;
    type Result = Vec<GameVersion>;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryLoadersParams {
    pub kind: EntryKind,
    pub flavor: String,
    pub version: String,
}

/// Loader builds newest-first; empty for a flavor with no pickable loader
/// version (vanilla, and loaders that pin one build per game version).
pub struct EntryLoaders;
impl Contract for EntryLoaders {
    const CHANNEL: &'static str = "entry.loaders";
    type Params = EntryLoadersParams;
    type Result = Vec<String>;
}

/// Refused while the entry runs.
pub struct EntryRemove;
impl Contract for EntryRemove {
    const CHANNEL: &'static str = "entry.remove";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = EntryTarget;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryRenameParams {
    pub target: EntryTarget,
    /// The new display name; the id (directory slug) is re-derived from it.
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryRenamed {
    pub id: String,
    pub name: String,
}

pub struct EntryRename;
impl Contract for EntryRename {
    const CHANNEL: &'static str = "entry.rename";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = EntryRenameParams;
    type Result = EntryRenamed;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryStopParams {
    pub target: EntryTarget,
    /// One of an instance's sessions; all of them otherwise. A server has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

pub struct EntryStop;
impl Contract for EntryStop {
    const CHANNEL: &'static str = "entry.stop";
    type Params = EntryStopParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryLogsParams {
    pub target: EntryTarget,
    /// One of an instance's sessions; the newest running (else newest) otherwise.
    /// A server has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Return only the last `tail` lines when set; all buffered lines otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<usize>,
}

pub struct EntryLogs;
impl Contract for EntryLogs {
    const CHANNEL: &'static str = "entry.logs";
    type Params = EntryLogsParams;
    type Result = Vec<ProcessLogLine>;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryConfigGetParams {
    pub target: EntryTarget,
    pub key: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryConfigValue {
    pub value: String,
}

pub struct EntryConfigGet;
impl Contract for EntryConfigGet {
    const CHANNEL: &'static str = "entry.config.get";
    type Params = EntryConfigGetParams;
    type Result = EntryConfigValue;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryConfigSetParams {
    pub target: EntryTarget,
    pub key: String,
    pub value: String,
}

pub struct EntryConfigSet;
impl Contract for EntryConfigSet {
    const CHANNEL: &'static str = "entry.config.set";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = EntryConfigSetParams;
    type Result = Empty;
}

/// The entry's settings, and the rules a form validates them with.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryConfigListResult {
    pub entries: Vec<ConfigEntry>,
    pub rules: Vec<KeyRule>,
}

pub struct EntryConfigList;
impl Contract for EntryConfigList {
    const CHANNEL: &'static str = "entry.config.list";
    type Params = EntryTarget;
    type Result = EntryConfigListResult;
}
