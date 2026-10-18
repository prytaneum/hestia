//! Minecraft instance (client) contracts: the stored-record management and the
//! launch job; what a server shares is in `entry`. Unlike servers, an
//! instance is a lightweight record at create time — its files (client jar,
//! libraries, assets) are materialised by the launch job.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::content::{ContentFailure, ContentKind};
use crate::contract::{Contract, Empty};
use crate::job::{Job, JobStarted};
use crate::minecraft::{ConfigEntry, ProvisionProgress};
use crate::modpack::PackPolicy;
use crate::process::ProcessInfo;
use crate::warning::WarningInfo;

/// A managed instance: the stored record plus, when launched, its live sessions.
/// An instance can run more than once concurrently (each launch is a session),
/// so this is a list, not a single process.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    pub flavor: String,
    pub game_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub java_major: i32,
    pub created_unix: i64,
    /// Unix time of the most recent launch; `None` until first played — the
    /// signal the desktop uses to show a first-launch progress modal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played_unix: Option<i64>,
    /// Cumulative seconds played across every session the daemon has observed.
    pub playtime_seconds: i64,
    /// The content kinds this instance can take — see [`ServerInfo::accepts`].
    #[serde(default)]
    pub accepts: Vec<ContentKind>,
    pub sessions: Vec<ProcessInfo>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceCreateParams {
    /// Display name; defaults to `<flavor>-<version>` when empty.
    pub name: String,
    pub flavor: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// Create-time settings applied after the record is registered (memory,
    /// jvm-args).
    pub config: Vec<ConfigEntry>,
}

pub struct InstanceCreate;
impl Contract for InstanceCreate {
    const CHANNEL: &'static str = "instance.create";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    const TIMEOUT: Duration = Duration::from_secs(60);
    type Params = InstanceCreateParams;
    type Result = InstanceInfo;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceUpdateParams {
    /// Instance name or id.
    pub instance: String,
    /// The game version to move to (either direction; a downgrade needs
    /// `allow_downgrade`).
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// The caller confirms the user accepted the risk of moving to an older
    /// version (saves do not downgrade).
    pub allow_downgrade: bool,
    /// Whether a modpack the instance runs moves with the game version.
    pub pack: PackPolicy,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceUpdateDone {
    pub instance: InstanceInfo,
    pub warnings: Vec<WarningInfo>,
    /// Content a pack moved along with the update could not install; the
    /// update continues past them.
    pub failures: Vec<ContentFailure>,
}

/// A job, because moving a pack downloads its content. A cancel leaves the
/// instance on the version it had, or on the pack's new one if the cancel came
/// after the version moved.
pub struct InstanceUpdate;
impl Contract for InstanceUpdate {
    const CHANNEL: &'static str = "instance.update";
    type Params = InstanceUpdateParams;
    type Result = JobStarted;
}

impl Job for InstanceUpdate {
    type Progress = ProvisionProgress;
    type Done = InstanceUpdateDone;
}

pub struct InstanceList;
impl Contract for InstanceList {
    const CHANNEL: &'static str = "instance.list";
    type Params = Empty;
    type Result = Vec<InstanceInfo>;
}

/// Names one managed instance by id or name (remove / stop).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceRef {
    pub instance: String,
}

/// An instance's static, informational view: its descriptor, on-disk
/// locations, and footprint — everything independent of the live sessions.
/// The disk figure is a directory walk, so this is fetched on demand.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceDetails {
    pub id: String,
    pub name: String,
    pub flavor: String,
    pub game_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub java_major: i32,
    pub created_unix: i64,
    /// Unix time of the most recent launch; `None` until first played.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played_unix: Option<i64>,
    /// Cumulative seconds played across every observed session.
    pub playtime_seconds: i64,
    /// The entry root (`instances/<id>/`) — hestia's namespace.
    pub entry_dir: String,
    /// The game's working directory (`instances/<id>/data/`).
    pub data_dir: String,
    /// The entry's total on-disk footprint, in bytes.
    pub disk_bytes: u64,
}

pub struct InstanceInfoQuery;
impl Contract for InstanceInfoQuery {
    const CHANNEL: &'static str = "instance.info";
    type Params = InstanceRef;
    type Result = InstanceDetails;
}

/// How a world plays, from its `level.dat` `GameType`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Adventure,
    Spectator,
}

impl fmt::Display for GameMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
            GameMode::Adventure => "adventure",
            GameMode::Spectator => "spectator",
        })
    }
}

/// A world's difficulty, from its `level.dat` `Difficulty`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Peaceful,
    Easy,
    #[default]
    Normal,
    Hard,
}

impl fmt::Display for Difficulty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Difficulty::Peaceful => "peaceful",
            Difficulty::Easy => "easy",
            Difficulty::Normal => "normal",
            Difficulty::Hard => "hard",
        })
    }
}

/// One save world of an instance, described from its own `level.dat` rather than
/// from the directory listing: the folder name is what the game reads, but the
/// *world* is what the player recognises, and only the save itself knows its
/// display name, the version that wrote it, or when it was last opened.
///
/// Every field but `folder` is best-effort — a world whose `level.dat` is
/// missing, truncated, or from a layout we cannot read still lists, carrying
/// only its folder. `read` is false in that case, so a front-end can say so
/// instead of rendering confident defaults.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct WorldInfo {
    /// The directory under `data/saves/` — the identity every other call takes
    /// (a datapack installs by folder, not by display name).
    pub folder: String,
    /// `LevelName`: the in-game name, which need not match the folder.
    pub name: String,
    /// Whether `level.dat` could be read; false leaves the rest at defaults.
    pub read: bool,
    /// `Version.Name`, e.g. `1.21.1`; empty for a save too old to carry it.
    pub version: String,
    pub game_mode: GameMode,
    pub difficulty: Difficulty,
    pub hardcore: bool,
    /// `allowCommands` — cheats.
    pub cheats: bool,
    /// `LastPlayed`, in seconds (the save stores milliseconds); `None` when the
    /// save does not carry it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played_unix: Option<i64>,
    /// The world directory's footprint, in bytes — a directory walk.
    pub size_bytes: u64,
    /// The world's own `icon.png` (the in-game thumbnail), base64-encoded and
    /// empty when the save has none. Inlined rather than served as a path: the
    /// alternative widens the webview's asset-protocol reach to the whole data
    /// home, which also holds `accounts.json`.
    pub icon: String,
}

/// The instance's save worlds, sorted by folder name.
pub struct InstanceWorlds;
impl Contract for InstanceWorlds {
    const CHANNEL: &'static str = "instance.worlds";
    type Params = InstanceRef;
    type Result = Vec<WorldInfo>;
}

/// Something a session joined, as the game's own Quick Play log reported it —
/// the launch that returns to it is `instance` started with `target`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct RecentPlay {
    /// The instance's stable id.
    pub instance: String,
    pub instance_name: String,
    pub target: QuickPlay,
    /// The world's, server's or Realm's display name.
    pub name: String,
    pub game_mode: GameMode,
    pub played_unix: i64,
    /// The world's or the listed server's icon, base64-encoded PNG; empty when
    /// it has none. Inlined for the same reason a world's is — see
    /// [`WorldInfo::icon`].
    #[serde(default)]
    pub icon: String,
}

/// Across every instance, most recent first, capped.
pub struct InstanceRecent;
impl Contract for InstanceRecent {
    const CHANNEL: &'static str = "instance.recent";
    type Params = Empty;
    type Result = Vec<RecentPlay>;
}

/// One entry of the instance's multiplayer list, as `servers.dat` stores it.
/// The file is the game's, not ours: it is read on demand and rewritten whole,
/// so an entry carries every field back that it came with.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerEntry {
    /// The player-given name; the identity a `play` takes, alongside the address.
    pub name: String,
    /// `host` or `host:port` exactly as the game stores it.
    pub address: String,
    /// The server's cached icon, base64-encoded PNG; empty when it has none.
    /// Inlined for the same reason a world's is — see [`WorldInfo::icon`].
    pub icon: String,
    /// Whether the game auto-accepts the server's resource pack.
    pub accept_textures: bool,
    /// Hidden entries are the game's own scratch rows (direct-connect), which
    /// the multiplayer list does not show.
    pub hidden: bool,
}

/// The instance's multiplayer list, in the file's own order — the order the
/// game shows, which the player arranged.
pub struct InstanceServers;
impl Contract for InstanceServers {
    const CHANNEL: &'static str = "instance.servers";
    type Params = InstanceRef;
    type Result = Vec<ServerEntry>;
}

/// Add to, or edit, the instance's multiplayer list. `server` names the entry
/// to edit (by name or address) and is empty when adding.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceServerEditParams {
    pub instance: String,
    /// The existing entry to rewrite; empty appends a new one.
    pub server: String,
    pub name: String,
    pub address: String,
    pub accept_textures: bool,
}

/// A write to the multiplayer list, with what it could not guarantee. A running
/// session owns `servers.dat` and rewrites it wholesale when it exits, so an
/// edit made underneath one is reported as degraded rather than refused.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceServersWriteResult {
    pub servers: Vec<ServerEntry>,
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}

pub struct InstanceServerEdit;
impl Contract for InstanceServerEdit {
    const CHANNEL: &'static str = "instance.server.edit";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = InstanceServerEditParams;
    type Result = InstanceServersWriteResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceServerRef {
    pub instance: String,
    /// The entry to act on, by name or address.
    pub server: String,
}

pub struct InstanceServerRemove;
impl Contract for InstanceServerRemove {
    const CHANNEL: &'static str = "instance.server.remove";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = InstanceServerRef;
    type Result = InstanceServersWriteResult;
}

/// Rewrite the order of the multiplayer list. The file's order is the order
/// the game shows, so this is the player arranging their own list.
///
/// The whole arrangement travels at once rather than one move at a time: the
/// game's file is rewritten wholesale on every write, so a sequence of moves
/// would be a sequence of rewrites, each with its own in-use warning.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceServersArrangeParams {
    pub instance: String,
    /// Every *visible* entry, by name or address, in the order it should sit —
    /// the rows a caller was shown. Naming anything else, or naming one twice,
    /// is refused rather than guessed at: the list moved underneath the caller.
    /// The game's own hidden scratch rows are not named and keep their slots.
    pub order: Vec<String>,
}

pub struct InstanceServersArrange;
impl Contract for InstanceServersArrange {
    const CHANNEL: &'static str = "instance.servers.arrange";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = InstanceServersArrangeParams;
    type Result = InstanceServersWriteResult;
}

/// Status of an arbitrary multiplayer address, over the Server List Ping the
/// in-game list uses. Separate from `server.ping`, which reaches a *managed*
/// server on loopback and takes an entry reference rather than an address.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct AddressPingParams {
    /// `host` or `host:port`; the port defaults to 25565.
    pub address: String,
}

pub struct AddressPing;
impl Contract for AddressPing {
    const CHANNEL: &'static str = "minecraft.ping";
    type Params = AddressPingParams;
    type Result = crate::server::ServerPingResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceLaunchParams {
    pub instance: String,
    /// Account name or uuid; empty picks the sole signed-in account.
    pub account: String,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
    /// Launch another session even when one is already running. Off by default:
    /// a running instance is refused unless the caller opts into concurrency.
    #[serde(default)]
    pub new_session: bool,
    /// Skip the menus and join a world or a server on start. Absent launches to
    /// the title screen; the game only understands this from 1.20 on, so an
    /// older instance refuses rather than silently ignoring it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_play: Option<QuickPlay>,
    /// Launch without contacting Microsoft, using the signed-in account's name
    /// and uuid with an unusable token. Singleplayer works; the game refuses
    /// multiplayer, which is what an unauthenticated session is entitled to.
    #[serde(default)]
    pub offline: bool,
    /// Stop running sessions first and launch once they have exited: a restart
    /// done by the job itself, after the account check, so an expired sign-in
    /// fails before the game is stopped. A launch that replaces the only
    /// running session needs no `new_session`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace: Option<Replace>,
}

/// Which running sessions a launch stops before it starts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case", tag = "kind", content = "session")]
pub enum Replace {
    All,
    /// One session's process id.
    One(String),
}

/// What a launch joins directly. One target or none — a launch cannot open a
/// world *and* connect to a server, so the two are variants rather than two
/// fields that would need a cross-check at every layer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case", tag = "kind", content = "target")]
pub enum QuickPlay {
    /// A save directory under `data/saves/` — the folder name, not the world's
    /// display name, which is what `--quickPlaySingleplayer` takes.
    World(String),
    /// `host` or `host:port`, as `--quickPlayMultiplayer` takes it.
    Server(String),
    /// A Realm's numeric id, as `--quickPlayRealms` takes it.
    Realm(String),
}

/// A job. A cancel stops it before the game is spawned: nothing was started,
/// and whatever it had materialised stays — those files are shared and
/// idempotent, so the next launch resumes from them.
pub struct InstanceLaunch;
impl Contract for InstanceLaunch {
    const CHANNEL: &'static str = "instance.launch";
    type Params = InstanceLaunchParams;
    type Result = JobStarted;
}

impl Job for InstanceLaunch {
    type Progress = ProvisionProgress;
    type Done = InstanceLaunchDone;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct InstanceLaunchDone {
    pub process_id: String,
    pub pid: u32,
    /// Degraded outcomes of a launch that nonetheless started the game — a
    /// sync target left unshared, for instance. The session is running; these
    /// say what it is running against.
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}
