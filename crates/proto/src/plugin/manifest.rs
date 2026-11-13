use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::grant::Grant;
use super::permission::Permission;
use super::scope::{stays_inside, PermissionScope, ScopeKind};
use super::{
    CommandArgKind, CommandContribution, CommandScope, Contributions, PageSlot, Program, Setting,
    SettingKind, API_VERSION, CONTRIBUTION_ID_LENGTH, ENTRY_ARG, ID_LENGTH, MAX_COMMAND_WORDS,
    MAX_SCHEDULES, MAX_SCHEDULE_SECS, MIN_SCHEDULE_SECS, RESERVED_IDS,
};

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
    /// Each permission with the scope it reaches. `{}` is a permission with
    /// no narrowing; what a permission may be narrowed by is
    /// [`Permission::spec`].
    pub permissions: BTreeMap<Permission, PermissionScope>,
    /// What `process:spawn` may run, defined here and granted by naming the id
    /// in that permission's `programs` scope.
    pub programs: Vec<Program>,
    pub settings: Vec<Setting>,
    pub contributes: Contributions,
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub retired: Retired,
}

/// Top-level keys of the manifest shape before scoped permissions. Each is
/// read only to be refused, so a manifest written for that shape fails
/// validation instead of installing with nothing granted.
#[derive(Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Retired {
    network: Option<Seen>,
    unhashed: Option<Seen>,
    ipc: Option<Seen>,
    files: Option<Seen>,
    launch: Option<Seen>,
}

impl Retired {
    fn keys(self) -> impl Iterator<Item = (&'static str, &'static str)> {
        [
            (
                self.network,
                "network",
                r#""network:connect" = { hosts = [...] }"#,
            ),
            (
                self.unhashed,
                "unhashed",
                r#""network:connect" = { hosts = [...], unhashed = [...] }"#,
            ),
            (self.ipc, "ipc", r#""ipc:connect" = { endpoints = [...] }"#),
            (self.files, "files", r#""entry:read" = { files = [...] }"#),
            (
                self.launch,
                "launch",
                r#""launch:patch" = { properties = [...] }"#,
            ),
        ]
        .into_iter()
        .filter_map(|(seen, key, replacement)| seen.map(|_| (key, replacement)))
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen;

impl<'de> Deserialize<'de> for Seen {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| Seen)
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
    #[error("schedule `{id}` must tick every {MIN_SCHEDULE_SECS} to {MAX_SCHEDULE_SECS} seconds")]
    BadScheduleInterval { id: String },
    #[error("a plugin declares at most {MAX_SCHEDULES} schedules")]
    TooManySchedules,
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
    #[error("network host `{0}` is a wildcard over a public suffix, which would grant every site registered under it")]
    PublicSuffixWildcard(String),
    #[error("unhashed host `{0}` must be a host listed in the `hosts` of `network:connect`")]
    BadUnhashedHost(String),
    #[error("permission `{permission}` takes no `{kind}` scope")]
    ScopeNotAccepted {
        permission: Permission,
        kind: ScopeKind,
    },
    #[error("permission `{permission}` needs at least one `{kind}` entry")]
    ScopeRequired {
        permission: Permission,
        kind: ScopeKind,
    },
    #[error("`process:spawn` names the program `{0}`, which `[[programs]]` does not define")]
    ProgramNotDefined(String),
    #[error("provider kind `{0}` is not one this host offers")]
    UnknownProviderKind(String),
    #[error("provider `{provider}` needs the permission `{permission}`")]
    ProviderPermissionMissing {
        provider: String,
        permission: Permission,
    },
    #[error("program `{0}` needs a plain command name or a path inside data/, no shell, and well-formed arguments and hashes")]
    BadProgram(String),
    #[error("program `{0}` names exactly one identity: a `command`, or `java`, and a `java` program has no hash")]
    ProgramIdentity(String),
    #[error("program `{0}` declares `java` as a major, a `{{ min, max }}` range or \"profile\", with majors from 1 to 99")]
    BadProgramJava(String),
    #[error("program `{0}` writes `args`, `env` and `path` at the top level or lists named `invocations`, not both, and names each invocation once")]
    BadProgramInvocations(String),
    #[error("program `{0}` needs a `requires` with a name and an https url")]
    BadProgramRequires(String),
    #[error("program `{0}` runs from data/ and must declare a sha256 per target triple")]
    ProgramNeedsHash(String),
    #[error("program environment variable `{0}` is not a valid name, or is one no plugin may set")]
    BadProgramEnv(String),
    #[error("program `{program}` lists `{entry}` in `path`, which must be another `PATH` program of the manifest, named once")]
    BadProgramPath { program: String, entry: String },
    #[error("ipc endpoint `{0}` must be a relative name of letters, digits, `.`, `_` or `-`, optionally ending in `*`")]
    BadIpcEndpoint(String),
    #[error(
        "command `{0}` is one to {MAX_COMMAND_WORDS} words, each a slug, separated by one space"
    )]
    BadCommandId(String),
    #[error(
        "command `{0}` declares a `words` argument that is not the only one of its kind and last"
    )]
    BadWordsArg(String),
    #[error("file `{0}` must be a relative path of plain names, with no `..`, glob or backslash, and may not be `server.properties`")]
    BadEntryFile(String),
    #[error(
        "top-level `{key}` is no longer read; declare it under [permissions] as `{replacement}`"
    )]
    RetiredKey {
        key: &'static str,
        replacement: &'static str,
    },
}

fn collect_command_errors(command: &CommandContribution, errors: &mut Vec<ManifestError>) {
    let words: Vec<&str> = command.words().collect();
    if words.len() > MAX_COMMAND_WORDS
        || words
            .iter()
            .any(|word| !is_slug(word, CONTRIBUTION_ID_LENGTH))
    {
        errors.push(ManifestError::BadCommandId(command.id.clone()));
    }
    if words
        .first()
        .is_some_and(|first| command.scope.reserved_ids().contains(first))
    {
        errors.push(ManifestError::ReservedCommand(command.id.clone()));
    }
    let words_args = command
        .args
        .iter()
        .filter(|arg| arg.kind == CommandArgKind::Words)
        .count();
    let last_is_words = command
        .args
        .last()
        .is_some_and(|arg| arg.kind == CommandArgKind::Words);
    if words_args > 1 || (words_args == 1 && !last_is_words) {
        errors.push(ManifestError::BadWordsArg(command.id.clone()));
    }
    let mut seen = BTreeSet::new();
    for arg in &command.args {
        if !is_slug(&arg.name, CONTRIBUTION_ID_LENGTH) {
            errors.push(ManifestError::BadContributionId(arg.name.clone()));
        }
        if arg.name == ENTRY_ARG && command.scope != CommandScope::Top {
            errors.push(ManifestError::ReservedArg(command.id.clone()));
        }
        if !seen.insert(&arg.name) {
            errors.push(ManifestError::DuplicateId(arg.name.clone()));
        }
    }
}

/// Every rule a manifest breaks, one per line when displayed. Never empty.
#[derive(Debug, PartialEq, Eq)]
pub struct ManifestErrors(Vec<ManifestError>);

impl ManifestErrors {
    pub fn iter(&self) -> std::slice::Iter<'_, ManifestError> {
        self.0.iter()
    }

    pub fn contains(&self, error: &ManifestError) -> bool {
        self.0.contains(error)
    }

    pub fn into_vec(self) -> Vec<ManifestError> {
        self.0
    }
}

impl std::fmt::Display for ManifestErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (at, error) in self.0.iter().enumerate() {
            if at > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ManifestErrors {}

impl Manifest {
    /// Every rule the manifest breaks, in a stable order; `Ok` when none.
    pub fn validate(&self) -> Result<(), ManifestErrors> {
        let mut errors = Vec::new();
        self.collect_errors(&mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ManifestErrors(errors))
        }
    }

    fn collect_errors(&self, errors: &mut Vec<ManifestError>) {
        errors.extend(
            self.retired
                .keys()
                .map(|(key, replacement)| ManifestError::RetiredKey { key, replacement }),
        );
        if !is_plugin_id(&self.id) {
            errors.push(ManifestError::BadId(self.id.clone()));
        }
        if RESERVED_IDS.contains(&self.id.as_str()) {
            errors.push(ManifestError::ReservedId(self.id.clone()));
        }
        if semver_core(&self.version).is_none() {
            errors.push(ManifestError::BadVersion(self.version.clone()));
        }
        match (&self.api, &self.wasm) {
            (Some(api), _) => errors.extend(check_api(api).err()),
            (None, Some(_)) => errors.push(ManifestError::MissingApi),
            (None, None) => {}
        }
        let paths = self
            .wasm
            .iter()
            .chain(self.contributes.themes.iter().map(|theme| &theme.file));
        for path in paths {
            if !stays_inside(path) {
                errors.push(ManifestError::PathEscapes(path.clone()));
            }
        }
        for page in &self.contributes.pages {
            if let Some(icon) = page.icon_path() {
                if !stays_inside(icon) {
                    errors.push(ManifestError::PathEscapes(icon.to_string()));
                }
            }
            if page.slot == PageSlot::Entry && page.kind.is_none() {
                errors.push(ManifestError::PageNeedsKind(page.id.clone()));
            }
        }
        self.collect_scope_errors(errors);
        for setting in &self.settings {
            let bad = |detail: &str| ManifestError::BadSetting {
                key: setting.key.clone(),
                detail: detail.into(),
            };
            if setting.kind == SettingKind::Select && setting.options.is_empty() {
                errors.push(bad("a select needs options"));
            }
            if setting.kind == SettingKind::Secret && setting.default.is_some() {
                errors.push(bad("a secret has no default"));
            }
            if let Some(Err(detail)) = setting.default.as_ref().map(|value| setting.check(value)) {
                errors.push(bad(&format!("default {detail}")));
            }
        }
        for program in &self.programs {
            program.collect_errors(&self.programs, errors);
        }
        let contributes = &self.contributes;
        if contributes.schedules.len() > MAX_SCHEDULES {
            errors.push(ManifestError::TooManySchedules);
        }
        if let Some(schedule) = contributes
            .schedules
            .iter()
            .find(|schedule| !(MIN_SCHEDULE_SECS..=MAX_SCHEDULE_SECS).contains(&schedule.every))
        {
            errors.push(ManifestError::BadScheduleInterval {
                id: schedule.id.clone(),
            });
        }
        let lists: [Vec<&String>; 7] = [
            self.settings.iter().map(|setting| &setting.key).collect(),
            self.programs.iter().map(|program| &program.id).collect(),
            contributes
                .providers
                .iter()
                .map(|provider| &provider.id)
                .collect(),
            contributes.pages.iter().map(|page| &page.id).collect(),
            contributes.themes.iter().map(|theme| &theme.id).collect(),
            contributes.hooks.iter().collect(),
            contributes
                .schedules
                .iter()
                .map(|schedule| &schedule.id)
                .collect(),
        ];
        if let Some(pattern) = contributes
            .events
            .iter()
            .find(|pattern| !crate::glob::is_topic_glob(pattern))
        {
            errors.push(ManifestError::BadEventPattern(pattern.clone()));
        }
        for command in &contributes.commands {
            collect_command_errors(command, errors);
        }
        if let Some(theme) = contributes.themes.first() {
            if !self.permissions.contains_key(&Permission::UiCss) {
                errors.push(ManifestError::ThemeNeedsUiCss(theme.id.clone()));
            }
        }
        for hook in &contributes.hooks {
            let Some(point) = crate::hook::point(hook) else {
                errors.push(ManifestError::UnknownHook(hook.clone()));
                continue;
            };
            if let Some(missing) = point
                .permissions
                .iter()
                .find(|permission| !self.permissions.contains_key(permission))
            {
                errors.push(ManifestError::HookPermissionMissing {
                    hook: hook.clone(),
                    permission: *missing,
                });
            }
        }
        let mut command_ids = BTreeSet::new();
        for command in &contributes.commands {
            if !command_ids.insert(&command.id) {
                errors.push(ManifestError::DuplicateId(command.id.clone()));
            }
        }
        for ids in lists {
            let mut seen = BTreeSet::new();
            for id in ids {
                if !is_slug(id, CONTRIBUTION_ID_LENGTH) {
                    errors.push(ManifestError::BadContributionId(id.clone()));
                }
                if !seen.insert(id) {
                    errors.push(ManifestError::DuplicateId(id.clone()));
                }
            }
        }
        for provider in &contributes.providers {
            let Some(kind) = crate::provider::kind(&provider.kind) else {
                errors.push(ManifestError::UnknownProviderKind(provider.kind.clone()));
                continue;
            };
            let needs = kind.permissions;
            if let Some(missing) = needs
                .iter()
                .find(|permission| !self.permissions.contains_key(permission))
            {
                errors.push(ManifestError::ProviderPermissionMissing {
                    provider: provider.id.clone(),
                    permission: *missing,
                });
            }
        }
    }

    /// Every permission against the table: only the scope kinds it accepts,
    /// each entry meeting its kind's rule, and the scope it cannot do without.
    fn collect_scope_errors(&self, errors: &mut Vec<ManifestError>) {
        for (permission, scope) in &self.permissions {
            let spec = permission.spec();
            for (kind, entries) in scope.iter() {
                if !spec.scopes.contains(&kind) {
                    errors.push(ManifestError::ScopeNotAccepted {
                        permission: *permission,
                        kind,
                    });
                    continue;
                }
                errors.extend(entries.iter().filter_map(|entry| kind.check(entry).err()));
            }
            let required = spec.scopes.first().filter(|_| spec.requires_scope);
            if let Some(kind) = required.filter(|kind| scope.entries(**kind).next().is_none()) {
                errors.push(ManifestError::ScopeRequired {
                    permission: *permission,
                    kind: *kind,
                });
            }
        }
        if let Some(network) = self.permissions.get(&Permission::NetworkConnect) {
            errors.extend(
                network
                    .entries(ScopeKind::Unhashed)
                    .filter(|host| {
                        !network
                            .entries(ScopeKind::Hosts)
                            .any(|named| named == *host)
                    })
                    .map(|host| ManifestError::BadUnhashedHost(host.to_string())),
            );
        }
        if let Some(spawn) = self.permissions.get(&Permission::ProcessSpawn) {
            errors.extend(
                spawn
                    .entries(ScopeKind::Programs)
                    .filter(|id| !self.programs.iter().any(|program| program.id == *id))
                    .map(|id| ManifestError::ProgramNotDefined(id.to_string())),
            );
        }
    }

    /// What the user approves for this manifest: the permissions with their
    /// scopes, and what it contributes. The program ids `process:spawn` names
    /// become the programs' whole definitions.
    pub fn grant(&self) -> Grant {
        let mut permissions = self.permissions.clone();
        let mut programs = BTreeMap::new();
        if let Some(spawn) = permissions.get_mut(&Permission::ProcessSpawn) {
            let named: BTreeSet<String> = spawn
                .entries(ScopeKind::Programs)
                .map(str::to_string)
                .collect();
            *spawn = spawn.without(ScopeKind::Programs);
            for program in self
                .programs
                .iter()
                .filter(|program| named.contains(&program.id))
            {
                spawn.extend(ScopeKind::Programs, [program.entry()]);
                programs.insert(program.id.clone(), program.clone());
            }
        }
        let mut contributions = PermissionScope::default();
        let contributes = &self.contributes;
        contributions.extend(ScopeKind::Hooks, contributes.hooks.iter().cloned());
        contributions.extend(ScopeKind::Events, contributes.events.iter().cloned());
        contributions.extend(
            ScopeKind::Schedules,
            contributes
                .schedules
                .iter()
                .map(|schedule| schedule.entry()),
        );
        contributions.extend(
            ScopeKind::Secrets,
            self.settings
                .iter()
                .filter(|setting| setting.kind == SettingKind::Secret)
                .map(|setting| setting.key.clone()),
        );
        contributions.extend(
            ScopeKind::Themes,
            contributes.themes.iter().map(|theme| theme.id.clone()),
        );
        Grant {
            permissions,
            contributions,
            programs,
        }
    }
}

/// The shape of a plugin id, which is also its directory name.
pub fn is_plugin_id(id: &str) -> bool {
    is_slug(id, ID_LENGTH)
}

pub(super) fn is_slug(value: &str, length: std::ops::RangeInclusive<usize>) -> bool {
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

/// Whether a build written against caret requirement `api` runs on this host.
pub fn api_supported(api: &str) -> bool {
    api_supported_by(api, API_VERSION)
}

/// Whether a build written against caret requirement `api` runs on a host
/// exposing plugin API `host`, such as the one a release is about to bring.
pub fn api_supported_by(api: &str, host: &str) -> bool {
    check_api_against(api, host).is_ok()
}

/// Whether `version` is a plugin API version a host can expose.
pub fn is_api_version(version: &str) -> bool {
    semver_core(version).is_some()
}

/// Orders versions by `major.minor.patch`; one that does not parse sorts first.
pub fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    semver_core(a).cmp(&semver_core(b))
}

/// Caret semantics: a guest built against `^N.M.P` runs on a host `N.m.p` at
/// or above `N.M.P`. A newer guest may import something the host lacks.
fn check_api(api: &str) -> Result<(), ManifestError> {
    check_api_against(api, API_VERSION)
}

fn check_api_against(api: &str, host_version: &str) -> Result<(), ManifestError> {
    let required = api
        .strip_prefix('^')
        .and_then(semver_core)
        .ok_or_else(|| ManifestError::BadApi(api.to_string()))?;
    let mismatch = || ManifestError::ApiMismatch {
        required: api.to_string(),
        host: host_version.to_string(),
    };
    let host = semver_core(host_version).ok_or_else(mismatch)?;
    if required.0 == host.0 && required <= host {
        Ok(())
    } else {
        Err(mismatch())
    }
}
