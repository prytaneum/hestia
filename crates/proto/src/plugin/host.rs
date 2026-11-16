//! Host matching: which names a download may reach, and the path rule a program's
//! data directory keeps.

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

pub fn stays_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains([':', '\\'])
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}
