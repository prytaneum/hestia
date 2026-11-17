//! Endpoint (socket/pipe path) resolution for the daemon transport. A transport
//! concern, not a data concern: the runtime dir holds the ephemeral socket and is
//! distinct from the engine's persistent data_home.

use std::path::PathBuf;

/// The per-user runtime directory for ephemeral transport state.
#[cfg(unix)]
pub fn runtime_dir() -> PathBuf {
    // Prefer the session runtime dir (tmpfs, auto-cleaned at logout); fall back to
    // a uid-scoped /tmp dir so two users never collide on one socket path.
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("hestia");
        }
    }
    // SAFETY: getuid is always successful and has no preconditions.
    let uid = unsafe { libc::getuid() };
    PathBuf::from("/tmp").join(format!("hestia-{uid}"))
}

#[cfg(windows)]
pub fn runtime_dir() -> PathBuf {
    std::env::temp_dir().join("hestia")
}

/// An explicit endpoint override, shared by bind and connect. Lets tests (and
/// side-by-side daemons) use an isolated socket without touching the per-user
/// default.
fn endpoint_override() -> Option<PathBuf> {
    match std::env::var_os("HESTIA_SOCK") {
        Some(value) if !value.is_empty() => Some(PathBuf::from(value)),
        _ => None,
    }
}

/// An endpoint is scoped exactly as the data home behind it is: a build that
/// resolves its own `data/` must resolve its own socket, or it reaches whichever
/// daemon bound the shared one first and drives *that* daemon's data home.
fn scope_suffix() -> String {
    common::paths::install_scope()
        .map(|scope| format!("-{scope}"))
        .unwrap_or_default()
}

/// The default daemon endpoint. Both the daemon (bind) and clients (connect)
/// resolve the same path here.
#[cfg(unix)]
pub fn default_endpoint() -> PathBuf {
    endpoint_override()
        .unwrap_or_else(|| runtime_dir().join(format!("hestiad{}.sock", scope_suffix())))
}

#[cfg(windows)]
pub fn default_endpoint() -> PathBuf {
    endpoint_override().unwrap_or_else(|| {
        PathBuf::from(format!(
            r"{}{}-hestiad{}",
            common::app::PIPE_NAMESPACE,
            crate::identity::user_tag(),
            scope_suffix()
        ))
    })
}
