//! The version vocabulary a repository index speaks: plugin ids, API
//! versions and the caret requirement a build was written against.

use std::cmp::Ordering;

/// The plugin API this build exposes.
pub const API_VERSION: &str = "1.0.0";

const ID_LENGTH: std::ops::RangeInclusive<usize> = 2..=40;

/// The shape of a plugin id, which is also its directory name.
pub fn is_plugin_id(id: &str) -> bool {
    ID_LENGTH.contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
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

/// Whether a build written against caret requirement `api` runs on a host
/// exposing plugin API `host`: a guest built against `^N.M.P` runs on a host
/// `N.m.p` at or above `N.M.P`.
pub fn api_supported_by(api: &str, host: &str) -> bool {
    let (Some(required), Some(host)) = (
        api.strip_prefix('^').and_then(semver_core),
        semver_core(host),
    ) else {
        return false;
    };
    required.0 == host.0 && required <= host
}

/// Whether `version` is a plugin API version a host can expose.
pub fn is_api_version(version: &str) -> bool {
    semver_core(version).is_some()
}

/// Orders versions by `major.minor.patch`; one that does not parse sorts first.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    semver_core(a).cmp(&semver_core(b))
}
