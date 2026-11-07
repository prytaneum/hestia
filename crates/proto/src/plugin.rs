//! Plugins: the manifest a package carries, what the daemon reports about an
//! installed one, and the contracts that manage it. The WIT package in
//! `crates/sdk/wit` is the single source of the ABI version; `tests/plugin.rs`
//! fails if [`API_VERSION`] drifts from it.
//!
//! The manifest is parsed from TOML by its callers; this crate stays serde-only
//! so the CLI's `pack` can validate one without a daemon.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::change::Scope;
use crate::contract::{Contract, Empty, Topic};
use crate::error::EntryKind;
use crate::job::{Job, JobStarted};
use crate::ui::{Action, Effect, UiNode};
use crate::warning::WarningInfo;

pub const API_VERSION: &str = "1.0.0";

/// Where a plugin sees its `data/` directory: the host preopens it here and the
/// SDK reads and writes under it.
pub const DATA_MOUNT: &str = "/data";

/// The interceptors a plugin may declare in `contributes.hooks`, with the
/// permissions each needs. A hook is customisation that fails open, never a
/// control ([0095](../../../docs/decisions/0095-interceptors-are-bounded-and-fail-open.md)).
pub const HOOKS: &[(&str, &[Permission])] = &[
    (
        "before-launch",
        &[Permission::LaunchPatch, Permission::AccountRead],
    ),
    (
        "before-server-start",
        &[Permission::LaunchPatch, Permission::AccountRead],
    ),
    ("before-content-install", &[Permission::ContentRead]),
];

/// Ids a plugin may not take: the CLI's top-level verbs, which `hestia <plugin>`
/// would otherwise shadow.
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

/// Command ids a server-scoped command may not take: the verbs of
/// `hestia server <name> <verb>`. A CLI test pins it to the grammar.
pub const RESERVED_SERVER_VERBS: &[&str] = &[
    "attach", "backup", "cmd", "command", "config", "console", "datapack", "files", "help", "info",
    "logs", "modpack", "mod", "monitor", "players", "plugin", "remove", "rename", "restart", "rm",
    "start", "status", "stop", "task", "update",
];

/// Command ids an instance-scoped command may not take: the verbs of
/// `hestia instance <name> <verb>`. A CLI test pins it to the grammar.
pub const RESERVED_INSTANCE_VERBS: &[&str] = &[
    "config",
    "datapack",
    "export",
    "help",
    "info",
    "launch",
    "logs",
    "mod",
    "modpack",
    "monitor",
    "remove",
    "rename",
    "resourcepack",
    "restart",
    "rm",
    "server",
    "servers",
    "shader",
    "stop",
    "sync",
    "update",
    "world",
    "worlds",
];

const ID_LENGTH: std::ops::RangeInclusive<usize> = 2..=40;
const CONTRIBUTION_ID_LENGTH: std::ops::RangeInclusive<usize> = 1..=40;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Permission {
    #[serde(rename = "entry:read")]
    EntryRead,
    #[serde(rename = "entry:write")]
    EntryWrite,
    #[serde(rename = "process:read")]
    ProcessRead,
    #[serde(rename = "process:control")]
    ProcessControl,
    /// Run only the programs in [`Manifest::programs`].
    #[serde(rename = "process:spawn")]
    ProcessSpawn,
    #[serde(rename = "content:read")]
    ContentRead,
    #[serde(rename = "content:write")]
    ContentWrite,
    /// Profiles only, never tokens.
    #[serde(rename = "account:read")]
    AccountRead,
    #[serde(rename = "config:read")]
    ConfigRead,
    #[serde(rename = "config:write")]
    ConfigWrite,
    #[serde(rename = "java:read")]
    JavaRead,
    #[serde(rename = "java:manage")]
    JavaManage,
    /// Outbound HTTP to the hosts in [`Manifest::network`].
    #[serde(rename = "network:connect")]
    NetworkConnect,
    #[serde(rename = "ui:css")]
    UiCss,
    /// Return an allowlisted patch from a launch or server-start hook.
    #[serde(rename = "launch:patch")]
    LaunchPatch,
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
            Permission::EntryWrite
            | Permission::ContentWrite
            | Permission::ConfigWrite
            | Permission::ProcessControl
            | Permission::ProcessSpawn => Tier::Unrestricted,
            Permission::EntryRead
            | Permission::ProcessRead
            | Permission::ContentRead
            | Permission::AccountRead
            | Permission::ConfigRead
            | Permission::JavaRead
            | Permission::JavaManage
            | Permission::NetworkConnect
            | Permission::UiCss
            | Permission::LaunchPatch => Tier::Sandboxed,
        }
    }

    /// One line for the install prompt.
    pub fn description(self) -> &'static str {
        match self {
            Permission::EntryRead => "See your servers and instances and their settings",
            Permission::EntryWrite => {
                "Create, change and delete servers and instances, including their files"
            }
            Permission::ProcessRead => "See which servers and games are running",
            Permission::ProcessControl => {
                "Start and stop servers and games, and run console commands"
            }
            Permission::ProcessSpawn => "Run the programs it lists",
            Permission::ContentRead => "Search and inspect mods, packs and other content",
            Permission::ContentWrite => "Install, update and remove mods, packs and other content",
            Permission::AccountRead => "See the names of signed-in accounts, never their tokens",
            Permission::ConfigRead => "Read launcher settings",
            Permission::ConfigWrite => "Change launcher settings, including default JVM arguments",
            Permission::JavaRead => "See installed and available Java runtimes",
            Permission::JavaManage => "Install and remove Java runtimes",
            Permission::NetworkConnect => "Connect to the hosts it lists",
            Permission::UiCss => "Restyle the app with its own stylesheet",
            Permission::LaunchPatch => {
                "Adjust launch settings from an allowlist when a game or server starts"
            }
        }
    }
}

/// The `domain:action` name a manifest declares.
impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(name)) => f.write_str(&name),
            _ => Err(fmt::Error),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
    pub description: String,
    pub scope: CommandScope,
    pub args: Vec<CommandArg>,
}

/// What a command acts on. A scoped command receives the entry the caller named,
/// resolved to its id, as its `entry` argument.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum CommandScope {
    #[default]
    Top,
    Server,
    Instance,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum CommandArgKind {
    #[default]
    String,
    Number,
    Bool,
    Entry,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CommandArg {
    pub name: String,
    pub kind: CommandArgKind,
    pub required: bool,
    pub help: String,
}

impl CommandScope {
    /// The built-in verbs a command of this scope may not take; empty for `top`,
    /// whose commands live under the plugin's own id.
    pub fn reserved_ids(self) -> &'static [&'static str] {
        match self {
            CommandScope::Top => &[],
            CommandScope::Server => RESERVED_SERVER_VERBS,
            CommandScope::Instance => RESERVED_INSTANCE_VERBS,
        }
    }
}

/// The `kind` of a [`ProviderContribution`] that adds a content source.
pub const CONTENT_PROVIDER: &str = "content";

/// What any provider needs: to read what it serves and to reach its hosts.
const PROVIDER_PERMISSIONS: [Permission; 2] = [Permission::ContentRead, Permission::NetworkConnect];

/// The argument every scoped command receives; a declared argument may not take it.
pub const ENTRY_ARG: &str = "entry";

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
    pub title: String,
    /// A name from the app's icon set, or a package-relative path of an image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub slot: PageSlot,
    /// The kind of entry an `entry` page appears on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<EntryKind>,
}

/// Where a contributed page appears.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PageSlot {
    #[default]
    Nav,
    Entry,
    Settings,
}

impl PageContribution {
    /// An icon with a `/` or a `.` is a path inside the package; anything else
    /// is a name from the app's icon set.
    pub fn icon_path(&self) -> Option<&str> {
        self.icon
            .as_deref()
            .filter(|icon| icon.contains(['/', '.']))
    }
}

/// The colour scheme a theme is written for. The app is dark; a `light` theme
/// switches it to the light palette, `dark` and `both` keep the dark one.
#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    Both,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ThemeContribution {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
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

const SHELLS: [&str; 12] = [
    "sh",
    "bash",
    "zsh",
    "dash",
    "fish",
    "ksh",
    "csh",
    "tcsh",
    "cmd",
    "powershell",
    "pwsh",
    "wscript",
];
const ANY_REST: &str = "**";
const ANY_ONE: &str = "*";

impl Program {
    /// A command with a slash is a path inside the plugin's data directory;
    /// one without is a name resolved on `PATH` when the grant is approved.
    pub fn in_data_dir(&self) -> bool {
        self.command.contains('/')
    }

    /// Whether `args` match the declared patterns: a literal token matches
    /// itself, `*` any one token, and a trailing `**` any rest, including none.
    pub fn permits(&self, args: &[String]) -> bool {
        let (fixed, rest) = match self.args.split_last() {
            Some((last, fixed)) if last == ANY_REST => (fixed, true),
            _ => (&self.args[..], false),
        };
        let length = if rest {
            args.len() >= fixed.len()
        } else {
            args.len() == fixed.len()
        };
        length
            && fixed
                .iter()
                .zip(args)
                .all(|(pattern, arg)| pattern == ANY_ONE || pattern == arg)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        let bad = || ManifestError::BadProgram(self.id.clone());
        let name = self.command.rsplit('/').next().unwrap_or_default();
        let stem = name
            .strip_suffix(".exe")
            .unwrap_or(name)
            .to_ascii_lowercase();
        if SHELLS.contains(&stem.as_str()) || self.command.contains('\0') {
            return Err(bad());
        }
        if self.in_data_dir() {
            if !stays_inside(&self.command) {
                return Err(bad());
            }
            if self.sha256.is_empty() {
                return Err(ManifestError::ProgramNeedsHash(self.id.clone()));
            }
            let hashes_ok = self.sha256.iter().all(|(triple, hash)| {
                !triple.is_empty()
                    && triple
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                    && hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            });
            if !hashes_ok {
                return Err(bad());
            }
        } else if !(1..=64).contains(&name.len())
            || name.starts_with(['.', '-'])
            || !name.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
            })
        {
            return Err(bad());
        }
        let last = self.args.len().saturating_sub(1);
        if self
            .args
            .iter()
            .enumerate()
            .any(|(at, arg)| arg.contains('\0') || (arg == ANY_REST && at != last))
        {
            return Err(bad());
        }
        match self.env.iter().find(|name| !is_program_env(name)) {
            Some(name) => Err(ManifestError::BadProgramEnv(name.clone())),
            None => Ok(()),
        }
    }
}

/// Whether `name` may be declared in a program's `env`: a plain variable name
/// that is not one that loads code or redirects the program's own lookups.
/// No declaration unlocks a refused one.
pub fn is_program_env(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    (1..=128).contains(&name.len())
        && !name.starts_with(|first: char| first.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !upper.starts_with("LD_")
        && !upper.starts_with("DYLD_")
        && upper != "PATH"
        && !upper
            .find("JAVA")
            .is_some_and(|at| upper[at..].contains("OPTIONS"))
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
    /// Used while the user has set nothing. A secret has none.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub default: Option<serde_json::Value>,
    pub label: String,
    pub help: String,
    /// The choices of a `select`.
    pub options: Vec<String>,
}

const SETTING_TEXT_LIMIT: usize = 4096;

impl Setting {
    /// Why `value` cannot be this setting's value, if it cannot.
    pub fn check(&self, value: &serde_json::Value) -> Result<(), String> {
        use serde_json::Value;
        match (self.kind, value) {
            (SettingKind::String | SettingKind::Secret, Value::String(text)) => {
                if text.len() > SETTING_TEXT_LIMIT {
                    return Err(format!("longer than {SETTING_TEXT_LIMIT} bytes"));
                }
                Ok(())
            }
            (SettingKind::Number, Value::Number(_)) | (SettingKind::Bool, Value::Bool(_)) => Ok(()),
            (SettingKind::Select, Value::String(choice)) => {
                if self.options.contains(choice) {
                    Ok(())
                } else {
                    Err(format!("must be one of: {}", self.options.join(", ")))
                }
            }
            (SettingKind::String | SettingKind::Secret | SettingKind::Select, _) => {
                Err("must be text".into())
            }
            (SettingKind::Number, _) => Err("must be a number".into()),
            (SettingKind::Bool, _) => Err("must be true or false".into()),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Caret requirement on the host ABI, `^N.M.x`. Written by `pack`; a
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

    /// The permissions that reach beyond the sandbox.
    pub fn unrestricted(&self) -> Vec<Permission> {
        self.permissions
            .iter()
            .copied()
            .filter(|permission| permission.tier() == Tier::Unrestricted)
            .collect()
    }

    /// What this grant asks for beyond `installed`.
    pub fn widening_over(&self, installed: &Grant) -> Grant {
        Grant {
            permissions: &self.permissions - &installed.permissions,
            network: &self.network - &installed.network,
            programs: &self.programs - &installed.programs,
            launch_properties: &self.launch_properties - &installed.launch_properties,
            hooks: &self.hooks - &installed.hooks,
            events: &self.events - &installed.events,
            secret_settings: &self.secret_settings - &installed.secret_settings,
            ui_css: &self.ui_css - &installed.ui_css,
        }
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
    #[error("api `{0}` is not a caret requirement like ^1.0.0")]
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
    #[error("command `{0}` declares the argument `entry`, which a scoped command receives itself")]
    ReservedArg(String),
    #[error("command `{0}` is a built-in verb for its scope")]
    ReservedCommand(String),
    #[error("event `{0}` must be a topic like `job` or a prefix glob like `server.*`")]
    BadEventPattern(String),
    #[error("hook `{0}` is not one this host offers")]
    UnknownHook(String),
    #[error("hook `{hook}` needs the permission `{permission}`")]
    HookPermissionMissing {
        hook: String,
        permission: Permission,
    },
    #[error("page `{0}` appears on an entry and must declare its kind")]
    PageNeedsKind(String),
    #[error("theme `{0}` restyles the app and must be granted the permission `ui:css`")]
    ThemeNeedsUiCss(String),
    #[error("launch property `{0}` is not a valid name, or is one no plugin may set")]
    BadLaunchProperty(String),
    #[error("setting `{key}`: {detail}")]
    BadSetting { key: String, detail: String },
    #[error("network host `{0}` must be a domain name or `*.` and a domain, not an address")]
    BadNetworkHost(String),
    #[error("provider kind `{0}` is not one this host offers")]
    UnknownProviderKind(String),
    #[error("provider `{provider}` needs the permission `{permission}`")]
    ProviderPermissionMissing {
        provider: String,
        permission: Permission,
    },
    #[error("provider `{0}` needs at least one network host to fetch from")]
    ProviderNeedsHosts(String),
    #[error("program `{0}` needs a plain command name or a path inside data/, no shell, and well-formed arguments and hashes")]
    BadProgram(String),
    #[error("program `{0}` runs from data/ and must declare a sha256 per target triple")]
    ProgramNeedsHash(String),
    #[error("program environment variable `{0}` is not a valid name, or is one no plugin may set")]
    BadProgramEnv(String),
    #[error("programs need the permission `process:spawn`")]
    ProgramPermissionMissing,
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
        for page in &self.contributes.pages {
            if let Some(icon) = page.icon_path() {
                if !stays_inside(icon) {
                    return Err(ManifestError::PathEscapes(icon.to_string()));
                }
            }
            if page.slot == PageSlot::Entry && page.kind.is_none() {
                return Err(ManifestError::PageNeedsKind(page.id.clone()));
            }
        }
        if let Some(host) = self.network.iter().find(|host| !is_network_host(host)) {
            return Err(ManifestError::BadNetworkHost(host.clone()));
        }
        if let Some(key) = self
            .launch
            .properties
            .iter()
            .find(|key| !is_launch_property(key))
        {
            return Err(ManifestError::BadLaunchProperty(key.clone()));
        }
        for setting in &self.settings {
            let bad = |detail: &str| ManifestError::BadSetting {
                key: setting.key.clone(),
                detail: detail.into(),
            };
            if setting.kind == SettingKind::Select && setting.options.is_empty() {
                return Err(bad("a select needs options"));
            }
            if setting.kind == SettingKind::Secret && setting.default.is_some() {
                return Err(bad("a secret has no default"));
            }
            if let Some(Err(detail)) = setting.default.as_ref().map(|value| setting.check(value)) {
                return Err(bad(&format!("default {detail}")));
            }
        }
        if !self.programs.is_empty() && !self.permissions.contains(&Permission::ProcessSpawn) {
            return Err(ManifestError::ProgramPermissionMissing);
        }
        for program in &self.programs {
            program.validate()?;
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
        if let Some(pattern) = contributes
            .events
            .iter()
            .find(|pattern| !crate::glob::is_topic_glob(pattern))
        {
            return Err(ManifestError::BadEventPattern(pattern.clone()));
        }
        for command in &contributes.commands {
            if command.scope.reserved_ids().contains(&command.id.as_str()) {
                return Err(ManifestError::ReservedCommand(command.id.clone()));
            }
            let mut seen = BTreeSet::new();
            for arg in &command.args {
                if !is_slug(&arg.name, CONTRIBUTION_ID_LENGTH) {
                    return Err(ManifestError::BadContributionId(arg.name.clone()));
                }
                if arg.name == ENTRY_ARG && command.scope != CommandScope::Top {
                    return Err(ManifestError::ReservedArg(command.id.clone()));
                }
                if !seen.insert(&arg.name) {
                    return Err(ManifestError::DuplicateId(arg.name.clone()));
                }
            }
        }
        if let Some(theme) = contributes.themes.first() {
            if !self.permissions.contains(&Permission::UiCss) {
                return Err(ManifestError::ThemeNeedsUiCss(theme.id.clone()));
            }
        }
        for hook in &contributes.hooks {
            let (_, needs) = HOOKS
                .iter()
                .find(|(name, _)| name == hook)
                .ok_or_else(|| ManifestError::UnknownHook(hook.clone()))?;
            if let Some(missing) = needs
                .iter()
                .find(|permission| !self.permissions.contains(permission))
            {
                return Err(ManifestError::HookPermissionMissing {
                    hook: hook.clone(),
                    permission: *missing,
                });
            }
        }
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
        for provider in &contributes.providers {
            if provider.kind != CONTENT_PROVIDER {
                return Err(ManifestError::UnknownProviderKind(provider.kind.clone()));
            }
            if let Some(missing) = PROVIDER_PERMISSIONS
                .iter()
                .find(|permission| !self.permissions.contains(permission))
            {
                return Err(ManifestError::ProviderPermissionMissing {
                    provider: provider.id.clone(),
                    permission: *missing,
                });
            }
            if self.network.is_empty() {
                return Err(ManifestError::ProviderNeedsHosts(provider.id.clone()));
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

/// Whether `key` may be declared in `[launch] properties` and set by a launch
/// hook: a plain property name that is not one a plugin could use to load code,
/// redirect libraries or weaken the JVM. No declaration unlocks a refused one.
pub fn is_launch_property(key: &str) -> bool {
    const REFUSED_NAMES: [&str; 4] = [
        "java.library.path",
        "java.system.class.loader",
        "java.ext.dirs",
        "java.endorsed.dirs",
    ];
    const REFUSED_PREFIXES: [&str; 4] = ["java.security.", "jdk.", "sun.", "log4j"];
    const REFUSED_PARTS: [&str; 4] = ["agent", "classpath", "class.path", "module"];
    let lower = key.to_ascii_lowercase();
    (1..=100).contains(&key.len())
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !REFUSED_NAMES.contains(&lower.as_str())
        && !REFUSED_PREFIXES
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        && !REFUSED_PARTS.iter().any(|part| lower.contains(part))
}

/// A granted host: a domain name, or `*.` and a domain of at least two labels
/// that matches its subdomains only. An address would skip the resolver check
/// the network permission relies on.
pub fn is_network_host(entry: &str) -> bool {
    let domain = entry.strip_prefix("*.").unwrap_or(entry);
    let domain = domain.strip_suffix('.').unwrap_or(domain);
    let labels: Vec<&str> = domain.split('.').collect();
    let named = labels.iter().all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    });
    let numeric = labels
        .last()
        .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()));
    named && !numeric && domain.len() <= 253 && (labels.len() >= 2 || domain == entry)
}

/// `major.minor.patch`, ignoring any pre-release or build suffix.
fn semver_core(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(triple)
}

/// Whether a build written against caret requirement `api` runs on this host.
pub fn api_supported(api: &str) -> bool {
    check_api(api).is_ok()
}

/// Orders versions by `major.minor.patch`; one that does not parse sorts first.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    semver_core(a).cmp(&semver_core(b))
}

/// Caret semantics: a guest built against `^N.M.P` runs on a host `N.m.p` at
/// or above `N.M.P`. A newer guest may import something the host lacks.
fn check_api(api: &str) -> Result<(), ManifestError> {
    let required = api
        .strip_prefix('^')
        .and_then(semver_core)
        .ok_or_else(|| ManifestError::BadApi(api.to_string()))?;
    let host = semver_core(API_VERSION).expect("API_VERSION is a semantic version");
    if required.0 == host.0 && required <= host {
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
    /// From a configured repository's index; `signer` and `repository` say which.
    Repo,
    Dev,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInfo {
    pub manifest: Manifest,
    pub state: PluginState,
    pub source: PluginSource,
    /// Where a `url` or `repo` install downloaded from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Key id of the repository key that signed the package.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer: Option<String>,
    /// Signed by the official-plugins key; derived from `signer` on every read.
    pub official: bool,
    /// The index URL of the repository it was installed from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Signed, but no configured repository with that key lists it any more:
    /// it keeps running and gets no updates.
    pub obsolete: bool,
    /// Of the installed package.
    pub sha256: String,
    /// What the user approved, not what the manifest now asks for.
    pub grant: Grant,
    /// The permissions in `grant` that reach beyond the sandbox, so a front-end
    /// flags full access without a tier table of its own.
    pub unrestricted: Vec<Permission>,
    pub last_error: Option<String>,
    pub calls: u64,
    pub traps: u64,
    pub timeouts: u64,
    /// Size of the plugin's `data/` directory, measured only by `plugin.info`:
    /// walking every plugin's tree on each `plugin.list` costs too much.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_bytes: Option<u64>,
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

/// One line a dev plugin logged, for `hestia plugin dev`. Published only for a
/// plugin installed from a dev folder; never reachable by plugins.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PluginLogEvent {
    /// The plugin's id.
    pub id: String,
    pub level: String,
    pub text: String,
}
impl Topic for PluginLogEvent {
    const TOPIC: &'static str = "plugin.log";
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

/// A declared setting as the daemon reports it. A secret never carries its
/// value: `set` says whether one is stored.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SettingValue {
    pub key: String,
    /// What the user set, else the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub value: Option<serde_json::Value>,
    /// The user has stored a value of their own.
    pub set: bool,
}

pub struct PluginSettingsGet;
impl Contract for PluginSettingsGet {
    const CHANNEL: &'static str = "plugin.settings.get";
    type Params = PluginRef;
    type Result = Vec<SettingValue>;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginSettingsSetParams {
    pub id: String,
    pub key: String,
    /// `null` clears the setting back to its default.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub value: serde_json::Value,
}

pub struct PluginSettingsSet;
impl Contract for PluginSettingsSet {
    const CHANNEL: &'static str = "plugin.settings.set";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginSettingsSetParams;
    type Result = Empty;
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

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginAssetParams {
    pub plugin: String,
    /// Relative to the package and under `assets/` or `themes/`.
    pub path: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginAssetResult {
    pub mime: String,
    /// The file, base64-encoded; at most 8 MiB before encoding.
    pub bytes: String,
}

/// A file of an enabled plugin's `assets/` or `themes/`, for the desktop's
/// `hestia-plugin` scheme. Refuses a path that leaves those directories (links
/// included), an extension outside the served set, and a file over 8 MiB.
pub struct PluginAsset;
impl Contract for PluginAsset {
    const CHANNEL: &'static str = "plugin.asset";
    type Params = PluginAssetParams;
    type Result = PluginAssetResult;
}

pub struct PluginRemove;
impl Contract for PluginRemove {
    const CHANNEL: &'static str = "plugin.remove";
    const CHANGES: Option<Scope> = Some(Scope::Plugins);
    type Params = PluginRef;
    type Result = Empty;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginPackageRef {
    /// A `.hplugin` file or a folder holding `plugin.toml`.
    pub path: String,
    /// An https link to a `.hplugin` file, instead of `path`. The daemon
    /// downloads it once and answers with [`PluginInspection::staged`].
    pub url: String,
    /// The index URL of a configured repository, with `plugin`, instead of
    /// `path` and `url`: the daemon downloads and verifies the newest
    /// compatible build.
    pub repo: String,
    pub plugin: String,
}

/// What installing a package would grant, read without running any of it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInspection {
    pub manifest: Manifest,
    pub grant: Grant,
    /// Hand this back as [`PluginInstallParams::approve`] to approve exactly
    /// this grant.
    pub grant_digest: String,
    /// The installed plugin of the same id, which the install would update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<PluginInfo>,
    /// What the grant adds over the installed one; all of it when new.
    pub widened: Grant,
    /// The unrestricted permissions in what the install asks the user to
    /// approve (`widened`; the whole grant when new), so a front-end states
    /// full access without a tier table of its own.
    pub unrestricted: Vec<Permission>,
    /// Program id to the absolute path its `PATH` name resolves to on this
    /// computer, the one an install pins; a program not found is absent.
    pub program_paths: BTreeMap<String, String>,
    /// For a `url` inspection: names the downloaded file. Hand it back as
    /// [`PluginInstallParams::staged`] so the install uses that file and does
    /// not download again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged: Option<String>,
}

/// With a `url`, downloads the package, which can take a while.
pub struct PluginInspect;
impl Contract for PluginInspect {
    const CHANNEL: &'static str = "plugin.inspect";
    const TIMEOUT: Duration = Duration::from_secs(2 * 60);
    type Params = PluginPackageRef;
    type Result = PluginInspection;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginInstallParams {
    pub id: String,
    /// Instead of `staged`.
    pub path: String,
    /// The token from [`PluginInspection::staged`], instead of `path`.
    pub staged: String,
    /// The grant digest the user approved; the install refuses a package whose
    /// own grant digests differently.
    pub approve: String,
    /// Run the folder at `path` in place instead of copying it, and reload it
    /// when its component or themes change.
    pub dev: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum PluginInstallPhase {
    Extracting,
    Verifying,
    Installing,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallProgress {
    pub phase: PluginInstallPhase,
}

impl PluginInstallProgress {
    /// Overall completion in `0.0..=1.0`.
    pub fn ratio(&self) -> f64 {
        match self.phase {
            PluginInstallPhase::Extracting => 0.0,
            PluginInstallPhase::Verifying => 0.5,
            PluginInstallPhase::Installing => 0.9,
        }
    }
}

/// A job. A cancel leaves nothing behind: the package is staged beside the
/// plugins and only the final renames make it installed.
pub struct PluginInstall;
impl Contract for PluginInstall {
    const CHANNEL: &'static str = "plugin.install";
    type Params = PluginInstallParams;
    type Result = JobStarted;
}

/// Wraps the plugin because a job event's `state` tag would collide with
/// `PluginInfo::state`.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallDone {
    pub plugin: PluginInfo,
    /// The index listed other permissions than the package asks for; approval
    /// always used the package's own.
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}

impl Job for PluginInstall {
    type Progress = PluginInstallProgress;
    type Done = PluginInstallDone;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRunParams {
    pub id: String,
    pub plugin: String,
    pub command: String,
    /// The entry a scoped command acts on; refused for a `top` command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// An object of the command's declared arguments.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub args: serde_json::Value,
}

/// The same shape a plugin reports through `progress.report`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PluginRunProgress {
    /// Completion in `0.0..=1.0`; absent when the plugin cannot tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f32>,
    pub message: String,
}

impl PluginRunProgress {
    pub fn ratio(&self) -> f64 {
        f64::from(self.fraction.unwrap_or(0.0))
    }
}

/// A job running a plugin's command, bounded by the runtime's command deadline.
/// A cancel only asks: the plugin's `progress.cancelled` starts answering true,
/// and the call is dropped if it has not returned shortly after. What the command
/// already did to the entry or its data stays.
pub struct PluginRun;
impl Contract for PluginRun {
    const CHANNEL: &'static str = "plugin.run";
    type Params = PluginRunParams;
    type Result = JobStarted;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PluginRunDone {
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub output: serde_json::Value,
}

impl Job for PluginRun {
    type Progress = PluginRunProgress;
    type Done = PluginRunDone;
}

/// A page of an enabled plugin and the plugin that contributes it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginPage {
    pub plugin: String,
    pub page: PageContribution,
}

/// The pages of every enabled plugin that can answer a render.
pub struct PluginPages;
impl Contract for PluginPages {
    const CHANNEL: &'static str = "plugin.pages";
    type Params = Empty;
    type Result = Vec<PluginPage>;
}

/// A theme of an enabled plugin. `file` is package-relative; the webview
/// builds the `hestia-plugin` URL itself because its form differs per platform.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginTheme {
    pub plugin: String,
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    pub file: String,
}

/// The themes of every enabled plugin.
pub struct PluginThemes;
impl Contract for PluginThemes {
    const CHANNEL: &'static str = "plugin.themes";
    type Params = Empty;
    type Result = Vec<PluginTheme>;
}

/// Where a page is drawn. An `entry` page receives the server or instance it
/// sits on; the daemon replaces the reference with the entry's id.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PageContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginUiRenderParams {
    pub plugin: String,
    pub page: String,
    pub context: PageContext,
}

/// Asks the plugin for the current tree of one of its pages. The tree is checked
/// against the UI vocabulary before it is answered.
pub struct PluginUiRender;
impl Contract for PluginUiRender {
    const CHANNEL: &'static str = "plugin.ui.render";
    const TIMEOUT: Duration = Duration::from_secs(10);
    type Params = PluginUiRenderParams;
    type Result = UiNode;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginUiActionParams {
    pub plugin: String,
    pub page: String,
    pub action: Action,
    /// The submitted form's values by field name; an empty object for a button.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub values: serde_json::Value,
    pub context: PageContext,
}

/// Sends a click or a submit to the plugin; it answers what the front-end
/// should do next.
pub struct PluginUiAction;
impl Contract for PluginUiAction {
    const CHANNEL: &'static str = "plugin.ui.action";
    const TIMEOUT: Duration = Duration::from_secs(10);
    type Params = PluginUiActionParams;
    type Result = Vec<Effect>;
}
