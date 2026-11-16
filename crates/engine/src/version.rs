//! Reading a game version id as a comparable `x.y.z` triple.

/// The release triple an id starts with: `1.21.4`, `1.21-pre1` and
/// `1.14 Pre-Release 1` read as their release, and `1.2` fills the patch with 0.
/// A weekly snapshot (`24w14a`) and an id from before releases have none.
pub fn parse(id: &str) -> Option<(u64, u64, u64)> {
    let id = id.trim();
    let end = id
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(id.len());
    if id[end..].starts_with('w') {
        return None;
    }
    let mut parts = id[..end].split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// Whether the id names a version from before 1.0: alpha (`a1.2.6`), beta
/// (`b1.7.3`), classic (`c0.30_01c`), indev, infdev and pre-classic
/// (`rd-132211`). Every such id starts with a letter; every later one with a digit.
pub fn predates_releases(id: &str) -> bool {
    id.trim().starts_with(|c: char| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_reads_as_its_release_a_snapshot_or_an_old_version() {
        for (id, release) in [
            ("1.21.4", Some((1, 21, 4))),
            ("1.21-pre1", Some((1, 21, 0))),
            ("1.14 Pre-Release 1", Some((1, 14, 0))),
            ("26.1-snapshot-1", Some((26, 1, 0))),
            ("24w14a", None),
            ("b1.7.3", None),
        ] {
            assert_eq!(parse(id), release, "{id}");
        }
        for id in ["b1.7.3", "a1.0.4", "c0.30_01c", "inf-20100618", "rd-132211"] {
            assert!(predates_releases(id), "{id}");
        }
        for id in ["1.21.4", "24w14a"] {
            assert!(!predates_releases(id), "{id}");
        }
    }
}
