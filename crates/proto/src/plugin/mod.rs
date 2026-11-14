//! Plugins: the manifest a package carries, what the daemon reports about an
//! installed one, and the contracts that manage it. The WIT package in
//! `crates/sdk/wit` is the single source of the ABI version; `tests/plugin.rs`
//! fails if [`API_VERSION`] drifts from it.
//!
//! The manifest is parsed from TOML by its callers; this crate stays serde-only
//! so the CLI's `pack` can validate one without a daemon.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::change::Scope as Changed;
use crate::contract::{Contract, Empty, Topic};
use crate::error::EntryKind;
use crate::job::{Job, JobStarted};
use crate::ui::{Action, Effect, UiNode};
use crate::warning::WarningInfo;

mod grant;
mod manifest;
mod permission;
mod program;
mod scope;

pub use grant::Grant;
pub use manifest::{
    api_supported, api_supported_by, compare_versions, is_api_version, is_plugin_id, Manifest,
    ManifestError, ManifestErrors,
};
pub use permission::{Permission, Spec, Tier};
pub use program::{
    is_program_env, Invocation, InvocationMismatch, JavaChoiceError, JavaProfile, JavaRange,
    Program, ProgramJava, ProgramPin,
};
#[cfg(feature = "suffix")]
pub use scope::covers_public_suffix;
pub(crate) use scope::stays_inside;
pub use scope::{
    ipc_matches, is_ipc_pattern, is_launch_property, is_network_host, network_matches,
    PermissionScope, ScopeKind, Touched,
};

pub const API_VERSION: &str = "1.0.0";

/// Where a plugin sees its `data/` directory: the host preopens it here and the
/// SDK reads and writes under it.
pub const DATA_MOUNT: &str = "/data";

/// Where a flavor's `build` sees its read-write build area, for that call only.
pub const BUILD_MOUNT: &str = "/build";

/// Where a flavor's `build` sees the directory of the side's vanilla jar,
/// read-only and for that call only.
pub const VANILLA_MOUNT: &str = "/vanilla";

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
    "logs", "modpack", "mod", "monitor", "plugin", "remove", "rename", "restart", "rm", "start",
    "status", "stop", "task", "update",
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

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct CommandContribution {
    /// One to [`MAX_COMMAND_WORDS`] slugs separated by a space (`players allow`).
    /// On the CLI the words follow the scope's grammar, longest match first.
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
    /// The bare words after the command, as a list of strings. At most one per
    /// command, and the last.
    Words,
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

/// The most words a command id may have.
pub const MAX_COMMAND_WORDS: usize = 3;

impl CommandContribution {
    /// The words of the id.
    pub fn words(&self) -> impl Iterator<Item = &str> {
        self.id.split(' ')
    }

    /// How many leading words of `argv` spell this command's id; zero when they
    /// do not all match.
    pub fn matched_words(&self, argv: &[String]) -> usize {
        let words: Vec<&str> = self.words().collect();
        match argv.len() >= words.len() && words.iter().zip(argv).all(|(word, given)| word == given)
        {
            true => words.len(),
            false => 0,
        }
    }
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
    /// Topic globs such as `server.*`: while the page is open, a front-end
    /// asks the plugin to draw it again whenever one of these topics is
    /// published. The plugin is never sent the event.
    pub topics: Vec<String>,
    /// Seconds between redraws while the page is open, from
    /// [`MIN_PAGE_REFRESH_SECS`] to [`MAX_PAGE_REFRESH_SECS`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_every: Option<u64>,
}

pub const MIN_PAGE_REFRESH_SECS: u64 = 5;
pub const MAX_PAGE_REFRESH_SECS: u64 = 3600;
pub const MAX_PAGE_TOPICS: usize = 8;

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
    pub schedules: Vec<ScheduleContribution>,
}

/// A timer the host delivers to this plugin alone as a `plugin.schedule` event
/// ([`ScheduleEvent`]).
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ScheduleContribution {
    pub id: String,
    /// Seconds between ticks, from [`MIN_SCHEDULE_SECS`] to [`MAX_SCHEDULE_SECS`].
    pub every: u64,
}

impl ScheduleContribution {
    /// The entry a grant lists for this schedule, readable and parseable:
    /// `poll: 300s`.
    pub fn entry(&self) -> String {
        format!("{}: {}s", self.id, self.every)
    }

    pub fn from_entry(entry: &str) -> Option<ScheduleContribution> {
        let (id, every) = entry.split_once(": ")?;
        Some(ScheduleContribution {
            id: id.to_string(),
            every: every.strip_suffix('s')?.parse().ok()?,
        })
    }
}

pub const MIN_SCHEDULE_SECS: u64 = 1;
pub const MAX_SCHEDULE_SECS: u64 = 30 * 24 * 60 * 60;
pub const MAX_SCHEDULES: usize = 8;

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

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
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
    /// The least a `number` may be.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// The most a `number` may be.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// A regular expression the whole of a `string` must match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Shown in an empty field.
    pub placeholder: String,
    /// The heading the setting is listed under; settings without one come first.
    pub group: String,
    /// An `https` page that explains the setting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help_url: Option<String>,
}

const SETTING_TEXT_LIMIT: usize = 4096;
#[cfg(feature = "pattern")]
const SETTING_PATTERN_LIMIT: usize = 256;
#[cfg(feature = "pattern")]
const SETTING_PATTERN_SIZE: usize = 64 * 1024;

/// `en`, `pt-BR`, `zh-Hant-TW`: a language and optional subtags, which also
/// keeps a locale from naming a path.
pub fn is_locale_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let language = parts.next().unwrap_or_default();
    (2..=3).contains(&language.len())
        && language.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// The package file holding the values a distributor ships for the plugin's
/// settings, by key: used where the user has set none, ahead of the manifest's
/// own default.
pub const SETTING_DEFAULTS: &str = "defaults.json";

/// The environment variable `pack` and `publish` read a distributor's value
/// for the setting `key` of the plugin `id` from.
pub fn setting_default_var(id: &str, key: &str) -> String {
    let shout = |text: &str| -> String {
        text.chars()
            .map(|c| match c.is_ascii_alphanumeric() {
                true => c.to_ascii_uppercase(),
                false => '_',
            })
            .collect()
    };
    format!("HESTIA_SETTING_{}_{}", shout(id), shout(key))
}

impl Setting {
    /// The value `text` stands for when it comes from somewhere untyped, such
    /// as an environment variable; `None` when the kind cannot read it.
    pub fn parse(&self, text: &str) -> Option<serde_json::Value> {
        use serde_json::Value;
        match self.kind {
            SettingKind::String | SettingKind::Secret | SettingKind::Select => {
                Some(Value::String(text.to_string()))
            }
            SettingKind::Number => text
                .trim()
                .parse::<serde_json::Number>()
                .ok()
                .map(Value::Number),
            SettingKind::Bool => text.trim().parse::<bool>().ok().map(Value::Bool),
        }
    }

    /// Why `value` cannot be this setting's value, if it cannot.
    pub fn check(&self, value: &serde_json::Value) -> Result<(), String> {
        use serde_json::Value;
        match (self.kind, value) {
            (SettingKind::String | SettingKind::Secret, Value::String(text)) => {
                if text.len() > SETTING_TEXT_LIMIT {
                    return Err(format!("longer than {SETTING_TEXT_LIMIT} bytes"));
                }
                self.check_pattern(text)
            }
            (SettingKind::Number, Value::Number(number)) => {
                let value = number.as_f64().unwrap_or(f64::NAN);
                match (self.min, self.max) {
                    (Some(min), _) if value < min => Err(format!("must be at least {min}")),
                    (_, Some(max)) if value > max => Err(format!("must be at most {max}")),
                    _ if value.is_nan() => Err("must be a number".into()),
                    _ => Ok(()),
                }
            }
            (SettingKind::Bool, Value::Bool(_)) => Ok(()),
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

impl Setting {
    /// The compiled `pattern`, anchored to the whole value. Needs the host-side
    /// `pattern` feature: a build without it (the plugin SDK) cannot check one.
    #[cfg(feature = "pattern")]
    pub fn compiled_pattern(&self) -> Result<Option<regex::Regex>, String> {
        let Some(pattern) = &self.pattern else {
            return Ok(None);
        };
        if pattern.len() > SETTING_PATTERN_LIMIT {
            return Err(format!("longer than {SETTING_PATTERN_LIMIT} bytes"));
        }
        regex::RegexBuilder::new(&format!("^(?:{pattern})$"))
            .size_limit(SETTING_PATTERN_SIZE)
            .build()
            .map(Some)
            .map_err(|error| error.to_string())
    }

    #[cfg(feature = "pattern")]
    fn check_pattern(&self, text: &str) -> Result<(), String> {
        match self.compiled_pattern() {
            Ok(Some(pattern)) if !pattern.is_match(text) => Err(format!(
                "must match {}",
                self.pattern.as_deref().unwrap_or_default()
            )),
            Ok(_) => Ok(()),
            Err(error) => Err(format!("has a bad pattern: {error}")),
        }
    }

    #[cfg(not(feature = "pattern"))]
    fn check_pattern(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
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
    /// Written against a plugin API this Hestia no longer exposes; it stays off
    /// until a build for the current API replaces it.
    NeedsUpdate,
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
    /// Wake-ups the daemon held back to keep the plugin within its call budget.
    pub throttled: u64,
    /// Size of the plugin's `data/` directory, measured only by `plugin.info`:
    /// walking every plugin's tree on each `plugin.list` costs too much.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_bytes: Option<u64>,
    /// Filled only while developer mode is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub developer: Option<PluginDeveloperInfo>,
}

/// What `plugin.info` and `plugin.list` add for a developer.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginDeveloperInfo {
    /// Digest of the grant and program pins the user approved.
    pub grant_digest: String,
    /// Where compiled components are cached; the plugin's own artifact is named
    /// after its `sha256`.
    pub cache_dir: String,
    /// The last error and the causes beneath it, outermost first.
    pub error_chain: Vec<String>,
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

/// Published when an updated plugin trapped or was disabled in its first minute
/// and the previous version was put back, after the `plugin.update` job had
/// already finished. An update that fails to load reports on the job instead.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PluginRolledBackEvent {
    pub id: String,
    /// The version that was taken out.
    pub version: String,
    /// The version that serves again.
    pub restored: String,
}
impl Topic for PluginRolledBackEvent {
    const TOPIC: &'static str = "plugin.rolled_back";
}

/// Published after the scheduled update check applied an official plugin's
/// update on its own.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PluginAutoUpdatedEvent {
    pub id: String,
    pub from: String,
    pub to: String,
}
impl Topic for PluginAutoUpdatedEvent {
    const TOPIC: &'static str = "plugin.auto_updated";
}

/// A tick of one of the plugin's own `contributes.schedules`. The daemon puts it
/// in that plugin's inbox and nowhere else: it is never published to the hub, so
/// no other plugin or client receives it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ScheduleEvent {
    pub id: String,
}
impl Topic for ScheduleEvent {
    const TOPIC: &'static str = "plugin.schedule";
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
    const CHANGES: Option<Changed> = Some(Changed::Plugins);
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
    const CHANGES: Option<Changed> = Some(Changed::Plugins);
    type Params = PluginRef;
    type Result = PluginInfo;
}

pub struct PluginDisable;
impl Contract for PluginDisable {
    const CHANNEL: &'static str = "plugin.disable";
    const CHANGES: Option<Changed> = Some(Changed::Plugins);
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

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginImageParams {
    pub plugin: String,
    pub url: String,
}

/// An image a plugin's page names with `remote_image`, fetched by the daemon
/// like `image.fetch` but only from a host the plugin's `network:connect`
/// grant names, on every redirect hop. Refused for a plugin that is not running.
pub struct PluginImage;
impl Contract for PluginImage {
    const CHANNEL: &'static str = "plugin.image";
    const TIMEOUT: Duration = Duration::from_secs(30);
    type Params = PluginImageParams;
    type Result = crate::image::ImageFetchResult;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginMessagesParams {
    pub plugin: String,
    /// A tag like `pt-BR`; see [`is_locale_tag`].
    pub locale: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginMessagesResult {
    /// Text by key.
    pub messages: std::collections::BTreeMap<String, String>,
}

/// A plugin's message catalog for a locale: the English file, overlaid by the
/// language's, overlaid by the exact locale's, so a key the locale lacks reads
/// in the nearest language that has it. Empty when the plugin ships none.
pub struct PluginMessages;
impl Contract for PluginMessages {
    const CHANNEL: &'static str = "plugin.messages";
    type Params = PluginMessagesParams;
    type Result = PluginMessagesResult;
}

pub struct PluginRemove;
impl Contract for PluginRemove {
    const CHANNEL: &'static str = "plugin.remove";
    const CHANGES: Option<Changed> = Some(Changed::Plugins);
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
    /// this grant and these `program_pins`.
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
    /// Program id to where its `PATH` name resolves on this computer and the
    /// hash of the file there, which an install pins; a program not found is
    /// absent, and cannot run until the plugin is approved again with it found.
    pub program_pins: BTreeMap<String, ProgramPin>,
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

/// An installed plugin with a newer version on offer.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct OutdatedPlugin {
    pub id: String,
    pub installed: String,
    /// The newest build this Hestia can run when one is newer than `installed`;
    /// otherwise the newest listed version, which `api_compatible` says it cannot run.
    pub available: String,
    /// Whether this Hestia can run `available`.
    pub api_compatible: bool,
    /// The plugin API the latest listed version needs, when this Hestia's does not
    /// meet it, even if an older `available` build runs. The index carries the API
    /// requirement, not a Hestia version.
    pub requires: Option<String>,
    /// What the update asks beyond the installed grant. From a repository this
    /// is the permissions its index lists; the install inspection has the full grant.
    pub widened: Grant,
    /// The index URL that offers it; empty for an unsigned URL install.
    pub repository: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct OutdatedParams {
    /// Also re-download every unsigned URL install to read its version. Never
    /// done by the schedule: it asks a server nobody vouched for.
    pub unsigned: bool,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Outdated {
    pub plugins: Vec<OutdatedPlugin>,
    /// The answer comes from cached indexes: the network is down.
    pub offline: bool,
    pub warnings: Vec<WarningInfo>,
}
impl Topic for Outdated {
    const TOPIC: &'static str = "plugin.updates";
}

/// Refreshes every repository once, then compares what each signed plugin was
/// installed from with what those repositories list.
pub struct PluginOutdated;
impl Contract for PluginOutdated {
    const CHANNEL: &'static str = "plugin.outdated";
    const TIMEOUT: Duration = Duration::from_secs(2 * 60);
    type Params = OutdatedParams;
    type Result = Outdated;
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
    /// The digest the user approved; the install refuses a package whose
    /// grant, or whose programs' resolved pins, digest differently.
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
pub struct PluginUpdateParams {
    pub id: String,
    /// The installed plugin to update.
    pub plugin: String,
    /// The `grantDigest` of the inspection of the update the user approved.
    /// Required only when the update asks for more than is installed; a
    /// narrower or equal grant applies without it.
    pub approve: String,
}

/// A job. Resolves, downloads and verifies the newest build against the key
/// that signed the installed one, swaps it in, and loads it. An update that
/// asks for more than was approved is refused with
/// `plugin_update_needs_approval` before anything changes on disk. A build that
/// fails to load is swapped back out and reported as `plugin_update_rolled_back`
/// on the done event, with the plugin still at its old version. A cancel leaves
/// nothing behind.
pub struct PluginUpdate;
impl Contract for PluginUpdate {
    const CHANNEL: &'static str = "plugin.update";
    type Params = PluginUpdateParams;
    type Result = JobStarted;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateDone {
    /// The plugin as it runs now: the new version, or the old one after a rollback.
    pub plugin: PluginInfo,
    /// The version that was installed before the update.
    pub from: String,
    #[serde(default)]
    pub warnings: Vec<WarningInfo>,
}

impl Job for PluginUpdate {
    type Progress = PluginInstallProgress;
    type Done = PluginUpdateDone;
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

/// A command of an enabled plugin and the plugin that contributes it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginCommand {
    pub plugin: String,
    pub command: CommandContribution,
}

/// The commands of every enabled plugin that can run them, for a front-end to
/// offer beside the things they act on.
pub struct PluginCommands;
impl Contract for PluginCommands {
    const CHANNEL: &'static str = "plugin.commands";
    type Params = Empty;
    type Result = Vec<PluginCommand>;
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
    /// The language the user reads, a tag like `pt-BR` (see [`is_locale_tag`]);
    /// absent for a client with none. The host drops a malformed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
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
