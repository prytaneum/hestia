//! Comparing the running build against a version string, for the self-update
//! check.
//!
//! Anything unparsable compares as "no answer" (`None`) and every caller treats
//! that as a refusal — a malformed version cannot trigger an update.

use std::cmp::Ordering;

/// The numeric `x.y.z` triple, ignoring a `v` prefix and any
/// prerelease/build suffix. `1.2` fills the missing patch with 0.
pub fn parse(v: &str) -> Option<(u64, u64, u64)> {
    split(v).map(|(triple, _)| triple)
}

/// Strictly newer by semver precedence; anything unparsable is never newer, so
/// a malformed manifest cannot trigger an update.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    compare(candidate, current) == Some(Ordering::Greater)
}

fn split(v: &str) -> Option<((u64, u64, u64), &str)> {
    let v = v.trim().trim_start_matches('v');
    // Build metadata may itself contain a `-`, so it comes off first.
    let v = v.split('+').next()?;
    let (triple, pre) = match v.split_once('-') {
        Some((triple, pre)) => (triple, pre),
        None => (v, ""),
    };
    let mut parts = triple.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some(((major, minor, patch), pre))
}

fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (a_triple, a_pre) = split(a)?;
    let (b_triple, b_pre) = split(b)?;
    Some(
        a_triple
            .cmp(&b_triple)
            .then_with(|| prerelease(a_pre, b_pre)),
    )
}

/// Semver §11 precedence: an absent prerelease outranks a present one, and two
/// present ones compare identifier by identifier.
fn prerelease(a: &str, b: &str) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        (false, false) => {}
    }
    let mut a = a.split('.');
    let mut b = b.split('.');
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ordering = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_newer, parse};

    #[test]
    fn newer_is_strict_and_rejects_garbage() {
        assert!(is_newer("0.0.2", "0.0.1"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.0.1", "0.0.1"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        assert!(!is_newer("garbage", "0.0.1"));
        assert!(!is_newer("0.0.2", "garbage"));
    }

    #[test]
    fn a_beta_is_offered_the_next_beta() {
        assert!(is_newer("1.3.0-beta.2", "1.3.0-beta.1"));
        assert!(!is_newer("1.3.0-beta.1", "1.3.0-beta.2"));
        assert!(!is_newer("1.3.0-beta.1", "1.3.0-beta.1"));
        assert!(is_newer("1.3.0-beta.10", "1.3.0-beta.9"));
    }

    #[test]
    fn a_release_outranks_its_own_prereleases() {
        assert!(is_newer("1.3.0", "1.3.0-beta.9"));
        assert!(!is_newer("1.3.0-beta.9", "1.3.0"));
        assert!(is_newer("1.3.0-beta.1", "1.2.5"));
    }

    #[test]
    fn build_metadata_does_not_decide() {
        assert!(!is_newer("1.3.0+build.9", "1.3.0+build.2"));
        assert!(is_newer("1.3.0+build.1", "1.3.0-beta.1"));
        assert_eq!(parse("1.3.0-beta.1+build.5"), Some((1, 3, 0)));
    }
}
