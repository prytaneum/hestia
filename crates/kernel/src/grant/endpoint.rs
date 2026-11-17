//! The spelling of a Windows pipe endpoint, which a grant and a connection judge alike.

pub(super) const PIPE_PREFIX: &str = r"\\.\pipe\";

/// The part of `entry` after `\\.\pipe\`, when it is a pipe name at all.
pub(super) fn pipe_name(entry: &str) -> Option<&str> {
    let prefix = entry.get(..PIPE_PREFIX.len())?;
    let name = &entry[PIPE_PREFIX.len()..];
    (prefix.eq_ignore_ascii_case(PIPE_PREFIX) && !name.is_empty()).then_some(name)
}

/// A name Windows takes as written. It reads `.` and `..` segments, separators
/// and trailing dots or spaces away before it looks the pipe up, so
/// `x\..\hestia-hestiad` names Hestia's pipe while looking like another.
pub(super) fn plain(name: &str) -> bool {
    !(matches!(name, "." | "..")
        || name.ends_with(['.', ' '])
        || name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        || name.chars().any(char::is_control))
}

/// Whether `entry` is a pipe under Hestia's own namespace.
pub(super) fn in_hestia_namespace(entry: &str) -> bool {
    let namespace = common::app::PIPE_NAMESPACE;
    entry
        .get(..namespace.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(namespace))
}
