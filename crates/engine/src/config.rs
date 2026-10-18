//! The typed settings store. The schema is one struct, `Settings`; a setting is a
//! field with its default, persisted as JSON. Internal code reads a `settings()`
//! snapshot and writes through `update()`; the dotted-path get/set serve the
//! `config.*` channels and reject unknown keys and type-mismatched values.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use proto::error::{ErrorInfo, ValueKind};
use proto::naming;
use proto::update::UpdateChannel;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::keys::invalid;
use crate::minecraft::launch::{normalize_memory, parse_jvm_args, JavaSettings};
use crate::schema::{self, Document};

/// The config schema. A setting is a typed field with its default; a nested
/// struct becomes a sub-object. The reserved keys (home, autostart) are routed
/// by the daemon's config service, not stored here.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// JVM defaults applied to any server or instance whose record leaves the
    /// matching per-entry setting unset.
    pub defaults: JvmDefaults,
    /// Credentials for the content sources that need one.
    pub content: ContentSettings,
    /// What an instance is allowed to do beyond the safe default.
    pub instance: InstanceSettings,
    /// Corrections applied over a modpack's own declarations.
    pub modpack: ModpackSettings,
    /// What the launcher publishes to a local Discord client.
    pub discord: DiscordSettings,
    /// Which release feed self-update follows.
    pub update: UpdateSettings,
    /// Whether the launcher may reach the network at all.
    pub network: NetworkSettings,
}

/// Outbound network policy, keyed `network.offline`. Pinned offline, nothing is
/// attempted at all: a metered or deliberately-disconnected machine gets an
/// immediate typed refusal rather than a connect timeout per call.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct NetworkSettings {
    pub offline: bool,
}

/// The self-update feed, keyed `update.channel`. It defaults to the channel
/// this build was shipped on rather than to stable: a beta build pointed at the
/// stable feed is stranded, since a release never outranks the prerelease that
/// precedes it until the *next* one ships.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct UpdateSettings {
    pub channel: UpdateChannel,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        UpdateSettings {
            channel: UpdateChannel::parse(common::app::CHANNEL).unwrap_or_default(),
        }
    }
}

/// Discord Rich Presence, keyed `discord.enabled`. On, the daemon publishes
/// what is being played — and that the launcher is open at all — to whoever can
/// see the user's Discord profile, so the key exists to switch that off.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct DiscordSettings {
    pub enabled: bool,
}

impl Default for DiscordSettings {
    fn default() -> Self {
        DiscordSettings { enabled: true }
    }
}

/// The corrections over what a modpack claims about itself, addressed by the
/// kebab-case keys `modpack.default-excludes`, `modpack.exclude-files`,
/// `modpack.force-include-files` and `modpack.overrides-exclusions`. The three
/// list keys take itzg's own delimiters (comma or newline, `#` comments), so a
/// docker-mc-server user's `MODRINTH_EXCLUDE_FILES` pastes in unchanged.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackSettings {
    /// Whether the shipped client-mod exclude table applies to server installs.
    pub default_excludes: bool,
    pub exclude_files: String,
    pub force_include_files: String,
    pub overrides_exclusions: String,
}

impl Default for ModpackSettings {
    fn default() -> Self {
        ModpackSettings {
            default_excludes: true,
            exclude_files: String::new(),
            force_include_files: String::new(),
            overrides_exclusions: String::new(),
        }
    }
}

/// What an instance may do beyond one session at a time, keyed
/// `instance.multi-session`. Concurrent sessions share a single `data/` — the
/// worlds, the configs, the content mirror — which Minecraft arbitrates only
/// per world, so the capability is off until a user asks for it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceSettings {
    pub multi_session: bool,
}

/// The launcher-wide JVM defaults, addressed by the kebab-case config keys
/// `defaults.memory` / `defaults.jvm-args`. Serializes camelCase like every
/// other struct (`jvmArgs` on disk); the dotted-path get/set translates the
/// kebab key to the camel field via `naming::config_key_to_field`. Plain
/// strings so both keys always resolve.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct JvmDefaults {
    pub memory: String,
    pub jvm_args: String,
}

/// Per-source credentials, keyed `content.curseforge-key`. CurseForge is
/// offered only once a key resolves — this, else the build-time one.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct ContentSettings {
    pub curseforge_key: String,
}

impl Document for Settings {
    const NAME: &'static str = "config.json";
}

impl Settings {
    /// Validate and canonicalise after a raw dotted-path set — the same rules
    /// the per-entry `memory`/`jvm-args` keys enforce.
    fn normalize(&mut self) -> Result<(), ErrorInfo> {
        if !self.defaults.memory.trim().is_empty() {
            self.defaults.memory =
                normalize_memory(&self.defaults.memory).map_err(invalid("defaults.memory"))?;
        } else {
            self.defaults.memory = String::new();
        }
        self.defaults.jvm_args = parse_jvm_args(&self.defaults.jvm_args)
            .map_err(invalid("defaults.jvm-args"))?
            .join(" ");
        self.content.curseforge_key = self.content.curseforge_key.trim().to_string();
        Ok(())
    }

    /// The JVM defaults as launch settings, for `JavaSettings::or_defaults`.
    pub fn java_defaults(&self) -> JavaSettings {
        JavaSettings {
            memory: (!self.defaults.memory.is_empty()).then(|| self.defaults.memory.clone()),
            jvm_args: self
                .defaults
                .jvm_args
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown config key: {0}")]
    UnknownKey(String),
    #[error("{key} expects a {expected}")]
    TypeMismatch { key: String, expected: ValueKind },
    #[error("invalid value for {key}: {source}")]
    InvalidValue {
        key: String,
        source: serde_json::Error,
    },
    #[error("{0}")]
    Rejected(ErrorInfo),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Save(anyhow::Error),
}

pub struct Config {
    path: PathBuf,
    settings: Mutex<Settings>,
}

impl Config {
    pub fn new(path: PathBuf) -> Self {
        let settings = Mutex::new(load_settings(&path));
        Config { path, settings }
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn update(&self, mutate: impl FnOnce(&mut Settings)) -> Result<(), ConfigError> {
        let mut settings = self.settings.lock().unwrap();
        mutate(&mut settings);
        save_settings(&self.path, &settings)
    }

    /// Return the value at a dotted key path, or `UnknownKey` if absent.
    pub fn get(&self, key: &str) -> Result<Value, ConfigError> {
        let current = self.settings.lock().unwrap();
        let mut doc = serde_json::to_value(&*current).unwrap_or(Value::Null);
        find_node_mut(&mut doc, key)
            .map(|node| node.take())
            .ok_or_else(|| ConfigError::UnknownKey(key.to_string()))
    }

    /// Set the value at a dotted key path, rejecting unknown keys and values of a
    /// different JSON kind than the existing setting.
    pub fn set(&self, key: &str, value: Value) -> Result<(), ConfigError> {
        let mut current = self.settings.lock().unwrap();
        let mut doc = serde_json::to_value(&*current).unwrap_or(Value::Null);
        {
            let node = find_node_mut(&mut doc, key)
                .ok_or_else(|| ConfigError::UnknownKey(key.to_string()))?;
            if !same_json_kind(node, &value) {
                return Err(ConfigError::TypeMismatch {
                    key: key.to_string(),
                    expected: kind_of(node),
                });
            }
            *node = value;
        }
        let mut settings: Settings =
            serde_json::from_value(doc).map_err(|source| ConfigError::InvalidValue {
                key: key.to_string(),
                source,
            })?;
        settings.normalize().map_err(ConfigError::Rejected)?;
        *current = settings;
        save_settings(&self.path, &current)?;
        tracing::info!(key, "config updated");
        Ok(())
    }

    /// The effective settings as a JSON object, keyed by the kebab-case
    /// `config.*` vocabulary the user sets (`defaults.jvm-args`), not the
    /// camelCase serialized fields.
    pub fn all(&self) -> Value {
        let current = self.settings.lock().unwrap();
        let settings =
            serde_json::to_value(&*current).unwrap_or_else(|_| Value::Object(Default::default()));
        naming::settings_to_config_keys(settings)
    }
}

fn load_settings(path: &Path) -> Settings {
    schema::load(path).unwrap_or_default()
}

fn save_settings(path: &Path, settings: &Settings) -> Result<(), ConfigError> {
    schema::save(path, settings).map_err(ConfigError::Save)
}

// The config keys are kebab-case (`defaults.jvm-args`); the settings serialize
// camelCase, so each segment is translated to its field name before lookup.
fn find_node_mut<'a>(root: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    let mut node = root;
    for segment in key.split('.') {
        if segment.is_empty() {
            return None;
        }
        node = node
            .as_object_mut()?
            .get_mut(&naming::config_key_to_field(segment))?;
    }
    Some(node)
}

fn same_json_kind(a: &Value, b: &Value) -> bool {
    if a.is_number() && b.is_number() {
        return true;
    }
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn kind_of(v: &Value) -> ValueKind {
    match v {
        Value::Null => ValueKind::Null,
        Value::Bool(_) => ValueKind::Boolean,
        Value::Number(_) => ValueKind::Number,
        Value::String(_) => ValueKind::String,
        Value::Array(_) => ValueKind::Array,
        Value::Object(_) => ValueKind::Object,
    }
}
