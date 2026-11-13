//! The narrowing a permission carries. A manifest gives a permission a [`PermissionScope`]:
//! for each [`ScopeKind`] the permission accepts, the entries it reaches. The
//! entry rules of every kind live here, beside the kind they validate.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::manifest::ManifestError;

/// What a scope entry names. Each permission accepts some of these
/// ([`Permission::spec`](super::Permission::spec)); the contribution kinds
/// (`Hooks` to `Themes`) are never declared, only derived from what a manifest
/// contributes, and describe what the user approves beside the permissions.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    /// Hosts `network:connect` reaches: a domain, or `*.domain`.
    Hosts,
    /// The hosts a flavor may download from without a checksum.
    Unhashed,
    /// Files of a server's directory `entry:read` may read, as exact paths.
    Files,
    /// Local endpoints `ipc:connect` reaches, such as `discord-ipc-*`.
    Endpoints,
    /// The programs `process:spawn` may run.
    Programs,
    /// The `-D` property keys a launch hook may set.
    Properties,
    Hooks,
    Events,
    Schedules,
    /// Settings whose values only the plugin can read back.
    Secrets,
    Themes,
}

impl std::fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 11] = [
        ScopeKind::Hosts,
        ScopeKind::Unhashed,
        ScopeKind::Files,
        ScopeKind::Endpoints,
        ScopeKind::Programs,
        ScopeKind::Properties,
        ScopeKind::Hooks,
        ScopeKind::Events,
        ScopeKind::Schedules,
        ScopeKind::Secrets,
        ScopeKind::Themes,
    ];

    /// What the entries of this kind are, for the install prompt.
    pub fn label(self) -> &'static str {
        match self {
            ScopeKind::Hosts => "Network",
            ScopeKind::Unhashed => "Unchecked downloads from",
            ScopeKind::Files => "Entry files",
            ScopeKind::Endpoints => "Local endpoints",
            ScopeKind::Programs => "Programs",
            ScopeKind::Properties => "Launch properties",
            ScopeKind::Hooks => "Hooks",
            ScopeKind::Events => "Events",
            ScopeKind::Schedules => "Schedules",
            ScopeKind::Secrets => "Secret settings",
            ScopeKind::Themes => "Themes",
        }
    }

    /// The kind as a manifest spells it.
    pub fn name(self) -> &'static str {
        match self {
            ScopeKind::Hosts => "hosts",
            ScopeKind::Unhashed => "unhashed",
            ScopeKind::Files => "files",
            ScopeKind::Endpoints => "endpoints",
            ScopeKind::Programs => "programs",
            ScopeKind::Properties => "properties",
            ScopeKind::Hooks => "hooks",
            ScopeKind::Events => "events",
            ScopeKind::Schedules => "schedules",
            ScopeKind::Secrets => "secrets",
            ScopeKind::Themes => "themes",
        }
    }

    /// The rule an entry written in a manifest must meet. A program or an
    /// unhashed host is checked against the rest of the manifest instead.
    pub(super) fn check(self, entry: &str) -> Result<(), ManifestError> {
        match self {
            ScopeKind::Hosts => {
                if !is_network_host(entry) {
                    return Err(ManifestError::BadNetworkHost(entry.to_string()));
                }
                #[cfg(feature = "suffix")]
                if covers_public_suffix(entry) {
                    return Err(ManifestError::PublicSuffixWildcard(entry.to_string()));
                }
                Ok(())
            }
            ScopeKind::Files if !is_entry_file(entry) => {
                Err(ManifestError::BadEntryFile(entry.to_string()))
            }
            ScopeKind::Endpoints if !is_ipc_pattern(entry) => {
                Err(ManifestError::BadIpcEndpoint(entry.to_string()))
            }
            ScopeKind::Properties if !is_launch_property(entry) => {
                Err(ManifestError::BadLaunchProperty(entry.to_string()))
            }
            _ => Ok(()),
        }
    }

    /// Whether the granted `entry` covers the concrete `item` a call touches.
    /// A host or endpoint entry may be a pattern; every other kind is exact.
    pub fn covers(self, entry: &str, item: &str) -> bool {
        match self {
            ScopeKind::Hosts | ScopeKind::Unhashed => network_matches(entry, item),
            ScopeKind::Endpoints => ipc_matches(entry, item),
            _ => entry == item,
        }
    }
}

/// The entries a permission reaches, sorted, by kind. A kind with no entries is
/// absent, so an empty scope is the scope of a permission that is not narrowed.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PermissionScope(BTreeMap<ScopeKind, BTreeSet<String>>);

impl PermissionScope {
    pub fn of<I, S>(kind: ScopeKind, entries: I) -> PermissionScope
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut scope = PermissionScope::default();
        scope.extend(kind, entries);
        scope
    }

    pub fn extend<I, S>(&mut self, kind: ScopeKind, entries: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut entries = entries.into_iter().map(Into::into).peekable();
        if entries.peek().is_some() {
            self.0.entry(kind).or_default().extend(entries);
        }
    }

    /// This scope without any entry of `kind`.
    pub fn without(&self, kind: ScopeKind) -> PermissionScope {
        let mut scope = self.clone();
        scope.0.remove(&kind);
        scope
    }

    pub fn is_empty(&self) -> bool {
        self.0.values().all(BTreeSet::is_empty)
    }

    pub fn entries(&self, kind: ScopeKind) -> impl Iterator<Item = &str> {
        self.0.get(&kind).into_iter().flatten().map(String::as_str)
    }

    /// The non-empty kinds, in kind order.
    pub fn iter(&self) -> impl Iterator<Item = (ScopeKind, &BTreeSet<String>)> {
        self.0
            .iter()
            .filter(|(_, entries)| !entries.is_empty())
            .map(|(kind, entries)| (*kind, entries))
    }

    /// Whether some entry of `kind` covers `item`.
    pub fn covers(&self, kind: ScopeKind, item: &str) -> bool {
        self.entries(kind).any(|entry| kind.covers(entry, item))
    }

    /// The entries here that `other` lacks, kind by kind.
    pub fn minus(&self, other: &PermissionScope) -> PermissionScope {
        let mut scope = PermissionScope::default();
        for (kind, entries) in self.iter() {
            scope.extend(
                kind,
                entries
                    .iter()
                    .filter(|entry| !other.0.get(&kind).is_some_and(|had| had.contains(*entry)))
                    .cloned(),
            );
        }
        scope
    }
}

/// What one call touches, read from its parameters by the contract that
/// declares a narrowing ([`Contract::NARROWED_BY`](crate::contract::Contract)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Touched {
    /// The channel is not narrowed; its permission alone decides.
    Unscoped,
    /// Every one of these must be covered by the caller's scope of `kind`.
    Items(ScopeKind, Vec<String>),
    /// The channel is narrowed but its parameters do not decode.
    Malformed,
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

const IPC_WILDCARD: char = '*';

fn is_ipc_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// A declarable local endpoint: a relative name of plain segments, resolved
/// under the user's runtime and temp directories (a named pipe on Windows,
/// where a name with `/` never matches). The last segment may end in `*`,
/// which matches any plain characters, such as the digit of `discord-ipc-0`;
/// a bare `*` is refused, since it would grant every endpoint in a directory.
pub fn is_ipc_pattern(pattern: &str) -> bool {
    let body = pattern.strip_suffix(IPC_WILDCARD).unwrap_or(pattern);
    (1..=128).contains(&pattern.len())
        && !body.contains(IPC_WILDCARD)
        && body.split('/').all(is_ipc_segment)
}

/// Whether the concrete endpoint `name` is one `pattern` grants.
pub fn ipc_matches(pattern: &str, name: &str) -> bool {
    if !is_ipc_pattern(name) || name.contains(IPC_WILDCARD) {
        return false;
    }
    match pattern.strip_suffix(IPC_WILDCARD) {
        Some(prefix) => name
            .strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && is_ipc_segment(rest)),
        None => pattern == name,
    }
}

/// A granted host: a domain name, or `*.` and a domain of at least two labels
/// that matches the names one label below it. An address would skip the
/// resolver check the network permission relies on.
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

/// Whether the granted host `entry` reaches `host`, which is lowercase and has
/// no trailing dot: an exact name, or for `*.domain` a name exactly one label
/// below `domain`, never `domain` itself nor anything deeper.
pub fn network_matches(entry: &str, host: &str) -> bool {
    let entry = entry.trim_end_matches('.').to_ascii_lowercase();
    match entry.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .and_then(|rest| rest.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty() && !label.contains('.')),
        None => host == entry,
    }
}

/// Whether `entry` is a wildcard whose domain is itself a public suffix, such
/// as `*.co.uk` or `*.github.io`: the names under one belong to unrelated
/// registrants, so no single plugin can be granted all of them.
#[cfg(feature = "suffix")]
pub fn covers_public_suffix(entry: &str) -> bool {
    let Some(domain) = entry.strip_prefix("*.") else {
        return false;
    };
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    psl::suffix(domain.as_bytes())
        .is_some_and(|suffix| suffix.is_known() && suffix.as_bytes() == domain.as_bytes())
}

/// A file a plugin may name in `files`: matched against a call's path by exact
/// string, so it must already be normalised. `server.properties` holds the
/// RCON password and is never readable by a plugin.
fn is_entry_file(path: &str) -> bool {
    stays_inside(path)
        && !path.contains(['*', '?', '[', ']', '{', '}'])
        && !path.eq_ignore_ascii_case("server.properties")
}

pub(crate) fn stays_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains([':', '\\'])
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}
