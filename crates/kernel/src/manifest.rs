use std::collections::BTreeMap;

use proto::plugin::ScopeKind;
use serde::Deserialize;

const NAME_MAX: usize = 64;
const VERSION_MAX: usize = 32;
const SCOPE_VALUES_MAX: usize = 64;
const SCOPE_VALUE_MAX: usize = 255;
const PROGRAM_ARGS_MAX: usize = 32;

/// `plugin.toml`: what the component cannot say about itself. Permissions and
/// contributions are read from the component, never declared here.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The scope of each permission that takes one, by permission name.
    #[serde(default)]
    pub permissions: BTreeMap<String, Scopes>,
}

#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Scopes {
    pub hosts: Vec<String>,
    /// Of `hosts`, those that may serve a body nothing verifies.
    pub unhashed: Vec<String>,
    pub paths: Vec<String>,
    pub endpoints: Vec<String>,
    pub programs: Vec<ProgramEntry>,
}

/// The operating systems a program entry can be limited to, named as
/// `std::env::consts::OS` names them.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Windows,
    Macos,
}

impl Os {
    pub fn is_current(self) -> bool {
        std::env::consts::OS
            == match self {
                Os::Linux => "linux",
                Os::Windows => "windows",
                Os::Macos => "macos",
            }
    }
}

/// One invocation a plugin may run: a program and its exact arguments, for one
/// operating system or, with no `os`, for all of them.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProgramEntry {
    pub path: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub os: Option<Os>,
}

impl Scopes {
    /// The plain values of a scope; `programs` are structured and read from
    /// [`Scopes::programs`].
    pub fn values(&self, kind: ScopeKind) -> &[String] {
        match kind {
            ScopeKind::Hosts => &self.hosts,
            ScopeKind::Paths => &self.paths,
            ScopeKind::Endpoints => &self.endpoints,
            ScopeKind::Programs => &[],
        }
    }

    /// How many entries the manifest lists for `kind`, for any platform.
    pub fn listed(&self, kind: ScopeKind) -> usize {
        match kind {
            ScopeKind::Programs => self.programs.len(),
            _ => self.values(kind).len(),
        }
    }

    pub fn kinds(&self) -> impl Iterator<Item = ScopeKind> + '_ {
        [
            ScopeKind::Hosts,
            ScopeKind::Paths,
            ScopeKind::Endpoints,
            ScopeKind::Programs,
        ]
        .into_iter()
        .filter(|kind| self.listed(*kind) > 0)
    }

    fn within_bounds(&self) -> bool {
        let plain = |values: &[String]| {
            values.len() <= SCOPE_VALUES_MAX && values.iter().all(|v| is_plain(v, SCOPE_VALUE_MAX))
        };
        let program = |entry: &ProgramEntry| {
            is_plain(&entry.path, SCOPE_VALUE_MAX)
                && entry.args.len() <= PROGRAM_ARGS_MAX
                && entry.args.iter().all(|arg| is_plain(arg, SCOPE_VALUE_MAX))
        };
        [&self.hosts, &self.unhashed, &self.paths, &self.endpoints]
            .into_iter()
            .all(|values| plain(values))
            && self.programs.len() <= SCOPE_VALUES_MAX
            && self.programs.iter().all(program)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("plugin.toml: {0}")]
    Syntax(#[from] toml::de::Error),
    #[error("plugin id '{0}' must be 1-64 lowercase letters, digits or '-', not starting with '-', and not a name Windows reserves")]
    Id(String),
    #[error("plugin name must be 1-{NAME_MAX} characters with no control or direction characters")]
    Name,
    #[error("the scope of '{0}' must list at most {SCOPE_VALUES_MAX} values of 1-{SCOPE_VALUE_MAX} characters (a program at most {PROGRAM_ARGS_MAX} arguments), with no control or direction characters")]
    Scope(String),
    #[error("plugin version '{0}' must be 1-{VERSION_MAX} letters, digits or '.+-'")]
    Version(String),
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Manifest = toml::from_str(text)?;
        if !crate::id::is_valid(&manifest.id) {
            return Err(ManifestError::Id(manifest.id));
        }
        if !is_plain(manifest.name.trim(), NAME_MAX) {
            return Err(ManifestError::Name);
        }
        if let Some((permission, _)) = manifest
            .permissions
            .iter()
            .find(|(_, scopes)| !scopes.within_bounds())
        {
            return Err(ManifestError::Scope(permission.clone()));
        }
        let version = &manifest.version;
        let version_ok = (1..=VERSION_MAX).contains(&version.len())
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'));
        if !version_ok {
            return Err(ManifestError::Version(manifest.version));
        }
        Ok(manifest)
    }
}

/// Text the approval prompt prints as is: no control characters, which could
/// move the cursor over other rows, and no direction or zero-width marks, which
/// could reorder or hide what the user reads.
fn is_plain(text: &str, max: usize) -> bool {
    let hidden = |c: char| {
        c.is_control()
            || matches!(c, '\u{061C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}')
            || matches!(c, '\u{2060}'..='\u{2069}' | '\u{FEFF}')
    };
    !text.is_empty() && text.chars().count() <= max && !text.chars().any(hidden)
}
