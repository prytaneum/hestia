//! Reading a game version string as a comparable `x.y.z` triple.

/// The numeric `x.y.z` triple, ignoring any prerelease or build suffix
/// (`1.21-pre1` is `1.21.0`). `1.2` fills the missing patch with 0.
pub fn parse(v: &str) -> Option<(u64, u64, u64)> {
    // Build metadata may itself contain a `-`, so it comes off first.
    let v = v.trim().split('+').next()?;
    let triple = v.split_once('-').map_or(v, |(triple, _)| triple);
    let mut parts = triple.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn prerelease_and_build_suffixes_are_ignored() {
        assert_eq!(parse("1.3.0-beta.1+build.5"), Some((1, 3, 0)));
        assert_eq!(parse("1.21-pre1"), Some((1, 21, 0)));
        assert_eq!(parse("24w14a"), None);
    }
}
