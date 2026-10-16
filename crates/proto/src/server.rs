//! Minecraft server contracts: browsing (flavors/versions/resolve), the
//! provision-at-create job, and lifecycle over the daemon's process supervisor.
//! A server is provisioned fully at create time — profile resolved, the Java
//! runtime ensured, the server jar downloaded, the EULA recorded — so `start`
//! is an immediate spawn.

use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::contract::{Contract, Empty, Topic};
use crate::error::ErrorInfo;
use crate::minecraft::{
    ConfigEntry, FlavorsResult, LoadersParams, LoadersResult, ProvisionProgress, VersionsParams,
    VersionsResult,
};
use crate::process::{ProcessInfo, ProcessLogsResult};
use crate::warning::WarningInfo;

pub struct ServerFlavors;
impl Contract for ServerFlavors {
    const CHANNEL: &'static str = "server.flavors";
    type Params = Empty;
    type Result = FlavorsResult;
}

pub struct ServerVersions;
impl Contract for ServerVersions {
    const CHANNEL: &'static str = "server.versions";
    type Params = VersionsParams;
    type Result = VersionsResult;
}

pub struct ServerLoaders;
impl Contract for ServerLoaders {
    const CHANNEL: &'static str = "server.loaders";
    type Params = LoadersParams;
    type Result = LoadersResult;
}

/// A managed server: the stored record plus, when it has been started, the
/// supervised process snapshot.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub flavor: String,
    pub game_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub java_major: i32,
    pub created_unix: i64,
    /// False while the create job is still provisioning files.
    pub ready: bool,
    /// Allocated at create and stable thereafter — players connect to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_port: Option<u16>,
    /// True once RCON is configured (a server started before the console
    /// existed gains one on its next start).
    pub console: bool,
    /// The content kinds this server can take, composed from its flavor's own
    /// loader and its side. Carried rather than re-derived, so a front-end
    /// never has to keep its own copy of the flavor table in step.
    #[serde(default)]
    pub accepts: Vec<ContentKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<ProcessInfo>,
}

/// A server's static, informational view: its descriptor, on-disk locations,
/// and footprint — everything that does not depend on the live process. The
/// disk figure is a directory walk, so this is fetched on demand, never in a
/// list.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerDetails {
    pub id: String,
    pub name: String,
    pub flavor: String,
    pub game_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    pub java_major: i32,
    pub created_unix: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_port: Option<u16>,
    /// The entry root (`servers/<id>/`) — hestia's namespace.
    pub entry_dir: String,
    /// The game's working directory (`servers/<id>/data/`).
    pub data_dir: String,
    /// The entry's total on-disk footprint, in bytes.
    pub disk_bytes: u64,
    /// The JVM flags this server actually starts with, and which layer supplied
    /// them. A flavor can recommend its own (Paper ships a tuned G1GC set), so
    /// `config get jvm-args` reporting "unset" is true of the *setting* while
    /// the server still runs with flags — this is where that is visible.
    #[serde(default)]
    pub jvm_args: Vec<String>,
    pub jvm_args_source: JvmArgsSource,
    /// Standing degraded state — a create's warnings scroll past, so `info`
    /// keeps reporting the ones that are still true.
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}

/// Which layer the effective `jvm-args` came from, innermost first.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum JvmArgsSource {
    /// Nothing set them anywhere; the server runs on the JVM's own defaults.
    #[default]
    None,
    /// The entry's own `jvm-args`.
    Entry,
    /// The launcher-wide `defaults.jvm-args`.
    Defaults,
    /// The flavor's published recommendation.
    Flavor,
}

pub struct ServerDetail;
impl Contract for ServerDetail {
    const CHANNEL: &'static str = "server.info";
    type Params = ServerRef;
    type Result = ServerDetails;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerCreateParams {
    /// Display name; defaults to `<flavor>-<version>` when empty.
    pub name: String,
    pub flavor: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// The caller confirms the user accepted the Minecraft EULA.
    pub eula: bool,
    /// Pin the game port; `None` picks the lowest free one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Create-time settings applied after the record is registered (memory,
    /// jvm-args, and any `server.properties` key).
    pub config: Vec<ConfigEntry>,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerCreateResult {
    pub id: String,
}

pub struct ServerCreate;
impl Contract for ServerCreate {
    const CHANNEL: &'static str = "server.create";
    type Params = ServerCreateParams;
    type Result = ServerCreateResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerUpdateParams {
    /// Server name or id.
    pub server: String,
    /// The game version to move to (either direction; a downgrade needs
    /// `allow_downgrade`).
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader_version: Option<String>,
    /// The caller confirms the user accepted the risk of moving to an older
    /// version (world data does not downgrade).
    pub allow_downgrade: bool,
    /// Client-supplied job id; empty asks the daemon to allocate one.
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerUpdateResult {
    pub id: String,
}

pub struct ServerUpdate;
impl Contract for ServerUpdate {
    const CHANNEL: &'static str = "server.update";
    type Params = ServerUpdateParams;
    type Result = ServerUpdateResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerListResult {
    pub servers: Vec<ServerInfo>,
}

pub struct ServerList;
impl Contract for ServerList {
    const CHANNEL: &'static str = "server.list";
    type Params = Empty;
    type Result = ServerListResult;
}

/// Names one managed server by id or name (remove / start / stop / status).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerRef {
    pub server: String,
}

pub struct ServerRemove;
impl Contract for ServerRemove {
    const CHANNEL: &'static str = "server.remove";
    type Params = ServerRef;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerRenameParams {
    /// The server to rename, by its current name or id.
    pub server: String,
    /// The new display name; the id (directory slug) is re-derived from it.
    pub name: String,
}

pub struct ServerRename;
impl Contract for ServerRename {
    const CHANNEL: &'static str = "server.rename";
    type Params = ServerRenameParams;
    type Result = ServerInfo;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerStartResult {
    pub process_id: String,
    pub pid: u32,
}

pub struct ServerStart;
impl Contract for ServerStart {
    const CHANNEL: &'static str = "server.start";
    type Params = ServerRef;
    type Result = ServerStartResult;
}

pub struct ServerStop;
impl Contract for ServerStop {
    const CHANNEL: &'static str = "server.stop";
    type Params = ServerRef;
    type Result = Empty;
}

/// Stop a running server, wait for it to exit, and start it again. Answers
/// only once the new process is up, so a caller waits out the stop.
pub struct ServerRestart;
impl Contract for ServerRestart {
    const CHANNEL: &'static str = "server.restart";
    type Params = ServerRef;
    type Result = ServerStartResult;
}

pub struct ServerStatus;
impl Contract for ServerStatus {
    const CHANNEL: &'static str = "server.status";
    type Params = ServerRef;
    type Result = ServerInfo;
}

/// Server List Ping snapshot over the game port; only a running server answers.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerPingResult {
    pub players_online: u32,
    pub players_max: u32,
    pub motd: String,
    pub version: String,
    /// The server's icon as the ping answered it: base64-encoded PNG with the
    /// `data:` prefix stripped, so it reads the same as a cached
    /// [`crate::instance::ServerEntry::icon`]. Empty when the server sends
    /// none, or sends one this build will not inline.
    pub favicon: String,
}

pub struct ServerPing;
impl Contract for ServerPing {
    const CHANNEL: &'static str = "server.ping";
    type Params = ServerRef;
    type Result = ServerPingResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerLogsParams {
    pub server: String,
    /// Return only the last `tail` lines when set; all buffered lines otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<usize>,
}

pub struct ServerLogs;
impl Contract for ServerLogs {
    const CHANNEL: &'static str = "server.logs";
    type Params = ServerLogsParams;
    type Result = ProcessLogsResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerCommandParams {
    pub server: String,
    pub command: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerCommandResult {
    pub response: String,
}

pub struct ServerCommand;
impl Contract for ServerCommand {
    const CHANNEL: &'static str = "server.command";
    type Params = ServerCommandParams;
    type Result = ServerCommandResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerCommandCompleteParams {
    pub server: String,
    pub line: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerCommandCompleteResult {
    /// What the line's last word can become.
    pub completions: Vec<String>,
    /// The arguments accepted in its place, as the server names them (`<targets>`).
    pub arguments: Vec<String>,
}

pub struct ServerCommandComplete;
impl Contract for ServerCommandComplete {
    const CHANNEL: &'static str = "server.command.complete";
    type Params = ServerCommandCompleteParams;
    type Result = ServerCommandCompleteResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfigGetParams {
    pub server: String,
    pub key: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfigGetResult {
    pub value: String,
}

/// One `server.properties` key as this server's version defines it. The key
/// set is the server's own — it moves with the game version — so a front-end
/// renders what it is handed and knows no key by name.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerProperty {
    pub key: String,
    pub value: String,
    /// What the server itself writes for this key; absent for a key its
    /// version does not define, or before a schema has been derived.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Owned by hestia; a write to it is refused.
    pub managed: bool,
    /// A credential, sent blank.
    pub secret: bool,
    /// Pinned by another setting right now (hardcore fixes the difficulty and
    /// the gamemode); a write to it is refused until that one changes.
    pub locked: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerPropertiesResult {
    /// Ordered by key.
    pub properties: Vec<ServerProperty>,
}

pub struct ServerProperties;
impl Contract for ServerProperties {
    const CHANNEL: &'static str = "server.properties";
    type Params = ServerRef;
    type Result = ServerPropertiesResult;
}

pub struct ServerConfigGet;
impl Contract for ServerConfigGet {
    const CHANNEL: &'static str = "server.config.get";
    type Params = ServerConfigGetParams;
    type Result = ServerConfigGetResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfigSetParams {
    pub server: String,
    pub key: String,
    pub value: String,
}

pub struct ServerConfigSet;
impl Contract for ServerConfigSet {
    const CHANNEL: &'static str = "server.config.set";
    type Params = ServerConfigSetParams;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ServerConfigListResult {
    pub entries: Vec<ConfigEntry>,
}

pub struct ServerConfigList;
impl Contract for ServerConfigList {
    const CHANNEL: &'static str = "server.config.list";
    type Params = ServerRef;
    type Result = ServerConfigListResult;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ServerCreateProgressEvent {
    pub id: String,
    #[serde(flatten)]
    pub progress: ProvisionProgress,
}
impl Topic for ServerCreateProgressEvent {
    const TOPIC: &'static str = "server.create.progress";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ServerCreateDoneEvent {
    pub id: String,
    pub server: ServerInfo,
    /// Degraded outcomes of a create that nonetheless succeeded — a front-end
    /// shows these under the success line.
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}
impl Topic for ServerCreateDoneEvent {
    const TOPIC: &'static str = "server.create.done";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ServerCreateErrorEvent {
    pub id: String,
    /// The structured cause a front-end localizes from.
    pub error: ErrorInfo,
}
impl Topic for ServerCreateErrorEvent {
    const TOPIC: &'static str = "server.create.error";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
/// A create stopped at the caller's request. The half-provisioned record is discarded, exactly as a failed create discards it — nothing un-startable is left behind.
pub struct ServerCreateCancelledEvent {
    pub id: String,
}
impl Topic for ServerCreateCancelledEvent {
    const TOPIC: &'static str = "server.create.cancelled";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ServerUpdateProgressEvent {
    pub id: String,
    #[serde(flatten)]
    pub progress: ProvisionProgress,
}
impl Topic for ServerUpdateProgressEvent {
    const TOPIC: &'static str = "server.update.progress";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ServerUpdateDoneEvent {
    pub id: String,
    pub server: ServerInfo,
    /// Degraded outcomes of an update that nonetheless succeeded.
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}
impl Topic for ServerUpdateDoneEvent {
    const TOPIC: &'static str = "server.update.done";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ServerUpdateErrorEvent {
    pub id: String,
    /// The structured cause a front-end localizes from.
    pub error: ErrorInfo,
}
impl Topic for ServerUpdateErrorEvent {
    const TOPIC: &'static str = "server.update.error";
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
/// A version update stopped at the caller's request. The server stays on the phase the update left it in and is recovered by updating again; its pre-update backup is already on disk.
pub struct ServerUpdateCancelledEvent {
    pub id: String,
}
impl Topic for ServerUpdateCancelledEvent {
    const TOPIC: &'static str = "server.update.cancelled";
}
