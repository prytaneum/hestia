//! Plugins: the manifest a package carries, what the daemon reports about an
//! installed one, and the contracts that manage it. The WIT package in
//! `crates/sdk/wit` is the single source of the ABI version; `tests/plugin.rs`
//! fails if [`API_VERSION`] drifts from it.
//!
//! The manifest is parsed from TOML by its callers; this crate stays serde-only
//! so the CLI's `pack` can validate one without a daemon.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::change::Scope;
use crate::contract::{Contract, Empty, Topic};

pub const API_VERSION: &str = "0.2.0";

/// Ids a plugin may not take: the CLI's top-level verbs, which a contributed
/// command would otherwise shadow.
pub const RESERVED_IDS: &[&str] = &[
    "account",
    "auth",
    "cache",
    "config",
    "daemon",
    "datapack",
    "help",
    "hestia",
    "instance",
    "java",
    "job",
    "logs",
    "mod",
    "modpack",
    "play",
    "plugin",
    "process",
    "rename",
    "resourcepack",
    "restart",
    "screenshot",
    "search",
    "server",
    "server-plugin",
    "shader",
    "sources",
    "start",
    "stop",
    "sync",
    "update",
];

const ID_LENGTH: std::ops::RangeInclusive<usize> = 2..=40;
const CONTRIBUTION_ID_LENGTH: std::ops::RangeInclusive<usize> = 1..=40;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    EntriesRead,
    EntriesWrite,
    ProcessesControl,
    ContentRead,
    ContentWrite,
    /// Profiles only, never tokens.
    AccountsRead,
    ConfigRead,
    ConfigWrite,
    JavaManage,
    /// Outbound HTTP to the hosts in [`Manifest::network`].
    Network,
    UiCss,
    /// Return an allowlisted patch from a launch or server-start hook.
    LaunchPatch,
    /// Run only the programs in [`Manifest::programs`].
    ProcessSpawn,
}

/// How far a permission reaches. An unrestricted one can run native code, so
/// the install prompt states it as full access to the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Sandboxed,
    Unrestricted,
}

impl Permission {
    pub fn tier(self) -> Tier {
        match self {
            Permission::EntriesWrite
            | Permission::ContentWrite
            | Permission::ConfigWrite
            | Permission::ProcessesControl
            | Permission::ProcessSpawn => Tier::Unrestricted,
            Permission::EntriesRead
            | Permission::ContentRead
            | Permission::AccountsRead
            | Permission::ConfigRead
            | Permission::JavaManage
            | Permission::Network
            | Permission::UiCss
            | Permission::LaunchPatch => Tier::Sandboxed,
        }
    }

    /// One line for the install prompt.
    pub fn description(self) -> &'static str {
        match self {
            Permission::EntriesRead => "See your servers and instances and their settings",
            Permission::EntriesWrite => {
                "Create, change and delete servers and instances, including their files"
            }
            Permission::ProcessesControl => {
                "Start and stop servers and games, and run console commands"
            }
            Permission::ContentRead => "Search and inspect mods, packs and other content",
            Permission::ContentWrite => "Install, update and remove mods, packs and other content",
            Permission::AccountsRead => "See the names of signed-in accounts, never their tokens",
            Permission::ConfigRead => "Read launcher settings",
            Permission::ConfigWrite => "Change launcher settings, including default JVM arguments",
            Permission::JavaManage => "Install and remove Java runtimes",
            Permission::Network => "Connect to the hosts it lists",
            Permission::UiCss => "Restyle the app with its own stylesheet",
            Permission::LaunchPatch => {
                "Adjust launch settings from an allowlist when a game or server starts"
            }
            Permission::ProcessSpawn => "Run the programs it lists",
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CommandContribution {
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderContribution {
    pub kind: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PageContribution {
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ThemeContribution {
    pub id: String,
    /// Package-relative path of the stylesheet.
    pub file: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Contributions {
    pub commands: Vec<CommandContribution>,
    /// Topic globs such as `server.*`.
    pub events: Vec<String>,
    pub hooks: Vec<String>,
    pub providers: Vec<ProviderContribution>,
    pub pages: Vec<PageContribution>,
    pub themes: Vec<ThemeContribution>,
}

/// A program the plugin may run. `command` is a name resolved on `PATH` or a
/// path inside the plugin's data directory; `args` are argument patterns.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Program {
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeSet<String>,
    /// Per target triple; required for a program inside the data directory.
    pub sha256: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Launch {
    /// `-D` system property keys a launch hook may set.
    pub properties: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SettingKind {
    #[default]
    String,
    Number,
    Bool,
    Select,
    Secret,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Setting {
    pub key: String,
    pub kind: SettingKind,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Caret requirement on the host ABI, `^0.N.x`. Written by `pack`; a
    /// theme-only package has none.
    pub api: Option<String>,
    pub description: String,
    pub authors: Vec<String>,
    pub homepage: Option<String>,
    /// Package-relative path of the component.
    pub wasm: Option<String>,
    pub permissions: Vec<Permission>,
    /// Hosts the `network` permission reaches.
    pub network: Vec<String>,
    pub programs: Vec<Program>,
    pub launch: Launch,
    pub settings: Vec<Setting>,
    pub contributes: Contributions,
}

/// Everything a user approves, canonical and sorted. Approval, widening and
/// re-approval compare grants, never bare permission lists.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Grant {
    pub permissions: BTreeSet<Permission>,
    pub network: BTreeSet<String>,
    pub programs: BTreeSet<Program>,
    pub launch_properties: BTreeSet<String>,
    pub hooks: BTreeSet<String>,
    pub events: BTreeSet<String>,
    pub secret_settings: BTreeSet<String>,
    pub ui_css: BTreeSet<String>,
}

impl Grant {
    /// Hex sha256 of the canonical JSON.
    pub fn digest(&self) -> String {
        let canonical = serde_json::to_vec(self).expect("a grant is plain data");
        Sha256::digest(canonical)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("plugin id `{0}` must be 2-40 lowercase letters, digits or dashes")]
    BadId(String),
    #[error("plugin id `{0}` is reserved")]
    ReservedId(String),
    #[error("version `{0}` is not a semantic version")]
    BadVersion(String),
    #[error("api `{0}` is not a caret requirement like ^0.2.0")]
    BadApi(String),
    #[error("api `{required}` is not satisfied by host API {host}")]
    ApiMismatch { required: String, host: String },
    #[error("a plugin with a component must declare its api")]
    MissingApi,
    #[error("path `{0}` must be relative and stay inside the package")]
    PathEscapes(String),
    #[error("contribution id `{0}` must be 1-40 lowercase letters, digits or dashes")]
    BadContributionId(String),
    #[error("id `{0}` is declared twice in the same list")]
    DuplicateId(String),
}

impl Manifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if !is_plugin_id(&self.id) {
            return Err(ManifestError::BadId(self.id.clone()));
        }
        if RESERVED_IDS.contains(&self.id.as_str()) {
            return Err(ManifestError::ReservedId(self.id.clone()));
        }
        if semver_core(&self.version).is_none() {
            return Err(ManifestError::BadVersion(self.version.clone()));
        }
        match (&self.api, &self.wasm) {
            (Some(api), _) => check_api(api)?,
            (None, Some(_)) => return Err(ManifestError::MissingApi),
            (None, None) => {}
        }
        let paths = self
            .wasm
            .iter()
            .chain(self.contributes.themes.iter().map(|theme| &theme.file));
        for path in paths {
            if !stays_inside(path) {
                return Err(ManifestError::PathEscapes(path.clone()));
            }
        }
        let contributes = &self.contributes;
        let lists: [Vec<&String>; 7] = [
            self.settings.iter().map(|setting| &setting.key).collect(),
            self.programs.iter().map(|program| &program.id).collect(),
            contributes
                .commands
                .iter()
                .map(|command| &command.id)
                .collect(),
            contributes
                .providers
                .iter()
                .map(|provider| &provider.id)
                .collect(),
            contributes.pages.iter().map(|page| &page.id).collect(),
            contributes.themes.iter().map(|theme| &theme.id).collect(),
            contributes.hooks.iter().collect(),
        ];
        for ids in lists {
            let mut seen = BTreeSet::new();
            for id in ids {
                if !is_slug(id, CONTRIBUTION_ID_LENGTH) {
                    return Err(ManifestError::BadContributionId(id.clone()));
                }
                if !seen.insert(id) {
                    return Err(ManifestError::DuplicateId(id.clone()));
                }
            }
        }
        Ok(())
    }

    pub fn grant(&self) -> Grant {
        Grant {
            permissions: self.permissions.iter().copied().collect(),
            network: self.network.iter().cloned().collect(),
            programs: self.programs.iter().cloned().collect(),
            launch_properties: self.launch.properties.iter().cloned().collect(),
            hooks: self.contributes.hooks.iter().cloned().collect(),
            events: self.contributes.events.iter().cloned().collect(),
            secret_settings: self
                .settings
                .iter()
                .filter(|setting| setting.kind == SettingKind::Secret)
                .map(|setting| setting.key.clone())
                .collect(),
            ui_css: self
                .contributes
                .themes
                .iter()
                .map(|theme| theme.id.clone())
                .collect(),
        }
    }
}

/// The shape of a plugin id, which is also its directory name.
pub fn is_plugin_id(id: &str) -> bool {
    is_slug(id, ID_LENGTH)
}

fn is_slug(value: &str, length: std::ops::RangeInclusive<usize>) -> bool {
    length.contains(&value.len())
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// `major.minor.patch`, ignoring any pre-release or build suffix.
fn semver_core(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(triple)
}

/// A guest built against `0.N.x` runs on a host with the same `0.N` and a patch
/// at least `x`: a newer guest patch may import something the host lacks.
fn check_api(api: &str) -> Result<(), ManifestError> {
    let required = api
        .strip_prefix('^')
        .and_then(semver_core)
        .ok_or_else(|| ManifestError::BadApi(api.to_string()))?;
    let host = semver_core(API_VERSION).expect("API_VERSION is a semantic version");
    if required.0 == host.0 && required.1 == host.1 && required.2 <= host.2 {
        Ok(())
    } else {
        Err(ManifestError::ApiMismatch {
            required: api.to_string(),
            host: API_VERSION.to_string(),
        })
    }
}

fn stays_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains([':', '\\'])
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    Enabled,
    Disabled,
    Failed,
    #[default]
    NeedsApproval,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    #[default]
    File,
    Folder,
    Url,
    Git,
    Dev,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInfo {
    pub manifest: Manifest,
    pub state: PluginState,
    pub source: PluginSource,
    /// Of the installed package.
    pub sha256: String,
    /// What the user approved, not what the manifest now asks for.
    pub grant: Grant,
    pub last_error: Option<String>,
    pub calls: u64,
    pub traps: u64,
    pub timeouts: u64,
    /// Size of the plugin's `data/` directory.
    pub data_bytes: u64,
}

/// Published on every state transition of a plugin.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PluginStateEvent {
    pub id: String,
    pub state: PluginState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl Topic for PluginStateEvent {
    const TOPIC: &'static str = "plugin.state";
}

pub struct PluginList;
impl Contract for PluginList {
    const CHANNEL: &'static str = "plugin.list";
    type Params = Empty;
    type Result = Vec<PluginInfo>;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRef {
    pub id: String,
}

pub struct PluginGet;
impl Contract for PluginGet {
    const CHANNEL: &'static str = "plugin.info";
    type Params = PluginRef;
    type Result = PluginInfo;
}

pub struct PluginEnable;
impl Contract for PluginEnable {
    const CHANNEL: &'static str = "plugin.enable";
    const TIMEOUT: Duration = Duration::from_secs(60);
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginRef;
    type Result = PluginInfo;
}

pub struct PluginDisable;
impl Contract for PluginDisable {
    const CHANNEL: &'static str = "plugin.disable";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginRef;
    type Result = PluginInfo;
}

pub struct PluginRemove;
impl Contract for PluginRemove {
    const CHANNEL: &'static str = "plugin.remove";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginRef;
    type Result = Empty;
}
