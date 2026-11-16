//! Reading a game version string as a comparable `x.y.z` triple.

/// The numeric `x.y.z` triple, ignoring a `v` prefix and any
/// prerelease/build suffix. `1.2` fills the missing patch with 0.
pub fn parse(v: &str) -> Option<(u64, u64, u64)> {
    split(v).map(|(triple, _)| triple)
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

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn prerelease_and_build_suffixes_are_ignored() {
        assert_eq!(parse("1.3.0-beta.1+build.5"), Some((1, 3, 0)));
        assert_eq!(parse("v1.2"), Some((1, 2, 0)));
        assert_eq!(parse("garbage"), None);
    }
}
