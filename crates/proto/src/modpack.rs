//! Modpack contracts: installing a pack into a new or existing entry, what the
//! entry then records about the pack it runs, and moving that pack to another
//! published version.
//!
//! A pack is resolved through the ordinary content vocabulary
//! ([`crate::content::ResolvedModpack`]) and then *installed*: its index files
//! become ordinary managed content tagged `modpack:<name>`, while its
//! `overrides/` land straight in the entry's game directory. Installing keeps
//! one channel per side, because a server install carries an EULA and a port;
//! every verb on an entry that already exists names it with an
//! [`EntryTarget`], so the side is data rather than a second channel.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::content::{ContentFailure, ReleaseChannel};
use crate::contract::Contract;
use crate::error::EntryTarget;
use crate::job::{Job, JobStarted};
use crate::minecraft::ProvisionProgress;
use crate::warning::WarningInfo;

/// Which pack to install: exactly one of `project` (a platform project,
/// optionally pinned by `version`), `url` (a project or version page on a
/// supported source), or `path` (a daemon-local `.mrpack` file).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackRef {
    pub source: String,
    pub project: String,
    pub version: String,
    pub url: String,
    pub path: String,
}

/// Where a pack install lands. Creating is the common case — a pack pins its
/// own loader and game version, so the entry it wants may not exist yet.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ModpackTarget {
    /// Create the entry from the pack: its flavor, game version and loader
    /// version all come from the pack index. An empty `name` takes the pack's.
    Create { name: String },
    /// Install into an existing entry. Its flavor and game version must already
    /// match the pack's — both are baked into the entry's resolved profile, so
    /// a mismatch is refused rather than silently producing an entry that
    /// cannot launch.
    Existing { entry: String },
}

impl Default for ModpackTarget {
    fn default() -> Self {
        ModpackTarget::Create {
            name: String::new(),
        }
    }
}

/// One file the pack wrote straight into the entry's game directory — an index
/// file outside a managed kind directory, or anything from `overrides/`. The
/// `sha1` is what the launcher wrote, so an update can tell a pack-owned file
/// from one the user has since edited.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackOverride {
    /// Path relative to the entry's `data/`.
    pub path: String,
    pub sha1: String,
}

/// The pack an entry runs, as the entry's record stores it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstalledModpack {
    /// A platform id (`modrinth`) or the literal `file` for a local import — an
    /// import carries empty project/version ids and cannot be updated.
    pub source: String,
    pub project_id: String,
    pub slug: String,
    pub name: String,
    pub version_id: String,
    pub version_number: String,
    pub game_version: String,
    /// The loader the pack pins, empty for a vanilla pack.
    pub loader: String,
    pub loader_version: String,
    pub icon_url: String,
    pub installed_unix: i64,
    /// The managed content filenames the pack installed — the pool items it
    /// tagged `modpack:<name>`.
    pub files: Vec<String>,
    /// What it wrote into the game directory, with the hashes it wrote.
    pub overrides: Vec<ModpackOverride>,
}

/// The pack an entry runs, absent when it was not built from one. A pack is not
/// a per-entry requirement, so "none" is an ordinary answer rather than a
/// `not_found`.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackStatusResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<InstalledModpack>,
}

/// Whether the pack an entry runs has a newer published version, and which one
/// an unpinned update would move it to.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackUpdate {
    pub current_version_id: String,
    pub current_version_number: String,
    pub latest_version_id: String,
    pub latest_version_number: String,
    pub updatable: bool,
    /// The pack version built for the game version asked about; may be a pre-release, so it is offered, never applied unpinned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitting: Option<ModpackFit>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackFit {
    pub version_id: String,
    pub version_number: String,
    pub channel: ReleaseChannel,
    pub game_version: String,
}

/// Absent when there is nothing to check: no pack, or one no source published,
/// which has no catalogue behind it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackUpdateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<ModpackUpdate>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceModpackInstallParams {
    #[serde(flatten)]
    pub pack: ModpackRef,
    pub target: ModpackTarget,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerModpackInstallParams {
    #[serde(flatten)]
    pub pack: ModpackRef,
    pub target: ModpackTarget,
    /// The caller confirms the user accepted the Minecraft EULA. Required when
    /// the target creates a server; ignored when it names an existing one.
    pub eula: bool,
    /// Pin the new server's game port; `None` picks the lowest free one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    pub id: String,
}

/// Move an entry's pack to another published version. An empty `version` takes
/// the newest. A pack update carries the entry's game version with it — that is
/// what updating a pack means — so the same `allow_downgrade` gate the plain
/// version update uses applies when the new pack targets an older game version.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryModpackUpdateParams {
    pub target: EntryTarget,
    pub to: ModpackMove,
    pub id: String,
}

/// Where an update takes an entry's pack: another published `version` (empty
/// for the newest), or a pack file at `path` — the only way a pack no source
/// published can move at all. Naming both is refused.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackMove {
    pub version: String,
    pub path: String,
    pub allow_downgrade: bool,
    /// Accept a file that is not the pack the entry runs; without it such a
    /// file is refused as `ModpackDifferentPack`.
    pub allow_other_pack: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryModpackStatusParams {
    pub target: EntryTarget,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryModpackCheckParams {
    pub target: EntryTarget,
    /// Empty asks about the game version the entry runs.
    pub game_version: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryModpackFileParams {
    pub target: EntryTarget,
    pub path: String,
}

/// What moving an entry's pack to a local pack file would do, read before
/// anything changes. A pack file carries a name and a free-text version but no
/// id, so `same_pack` is the project where a source recognises both and the
/// name otherwise — and two versions cannot be ordered, only shown.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackFilePreview {
    pub name: String,
    pub version_number: String,
    pub game_version: String,
    pub same_pack: bool,
    /// The file pins an older game version than the entry runs.
    pub downgrade: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryModpackRemoveParams {
    pub target: EntryTarget,
    /// Drop only the pack itself: its content stays installed as the user's
    /// own, and the game-directory files it wrote are left where they are.
    pub keep_content: bool,
}

/// What a pack removal took out. The entry itself stays — a pack is content it
/// carries, not the entry's identity.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackRemoveResult {
    pub removed_files: u32,
    pub removed_overrides: u32,
    /// Content left installed and handed to the user, when the removal kept it.
    pub released_files: u32,
    /// Override files left in place because the user had edited them.
    pub kept: Vec<String>,
}

pub struct InstanceModpackInstall;
impl Contract for InstanceModpackInstall {
    const CHANNEL: &'static str = "instance.modpack.install";
    type Params = InstanceModpackInstallParams;
    type Result = JobStarted;
}

impl Job for InstanceModpackInstall {
    type Progress = ProvisionProgress;
    type Done = ModpackDone;
}

pub struct ServerModpackInstall;
impl Contract for ServerModpackInstall {
    const CHANNEL: &'static str = "server.modpack.install";
    type Params = ServerModpackInstallParams;
    type Result = JobStarted;
}

impl Job for ServerModpackInstall {
    type Progress = ProvisionProgress;
    type Done = ModpackDone;
}

pub struct EntryModpackUpdate;
impl Contract for EntryModpackUpdate {
    const CHANNEL: &'static str = "modpack.update";
    type Params = EntryModpackUpdateParams;
    type Result = JobStarted;
}

impl Job for EntryModpackUpdate {
    type Progress = ProvisionProgress;
    type Done = ModpackDone;
}

pub struct EntryModpackStatus;
impl Contract for EntryModpackStatus {
    const CHANNEL: &'static str = "modpack.status";
    type Params = EntryModpackStatusParams;
    type Result = ModpackStatusResult;
}

pub struct EntryModpackPreviewUpdate;
impl Contract for EntryModpackPreviewUpdate {
    const CHANNEL: &'static str = "modpack.preview_update";
    const TIMEOUT: Duration = Duration::from_secs(120);
    type Params = EntryModpackFileParams;
    type Result = ModpackFilePreview;
}

pub struct EntryModpackCheckUpdate;
impl Contract for EntryModpackCheckUpdate {
    const CHANNEL: &'static str = "modpack.check_update";
    const TIMEOUT: Duration = Duration::from_secs(120);
    type Params = EntryModpackCheckParams;
    type Result = ModpackUpdateResult;
}

pub struct EntryModpackRemove;
impl Contract for EntryModpackRemove {
    const CHANNEL: &'static str = "modpack.remove";
    type Params = EntryModpackRemoveParams;
    type Result = ModpackRemoveResult;
}

/// A finished pack install or update. `entry` is the entry it landed in — for a
/// create that is the id the job just minted, which the caller has no other way
/// to learn. Cancelling one that creates its entry discards the half-built
/// entry, exactly as a failed create does; an install into an existing entry
/// leaves whatever had already been written, which the next install or update
/// reconciles.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ModpackDone {
    pub entry: String,
    pub entry_name: String,
    pub pack: InstalledModpack,
    /// Per-file failures; the install continues past them, so a pack with one
    /// dead download still produces a working entry minus that file.
    #[serde(default)]
    pub failures: Vec<ContentFailure>,
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}
