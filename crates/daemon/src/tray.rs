//! Spawning the tray helper: whenever hestiad serves, the tray accompanies it
//! — a daemon on an overridden endpoint included, since the spawned tray
//! inherits `HESTIA_SOCK` and follows its daemon. Best-effort: a headless
//! session, a missing binary, or `HESTIA_NO_TRAY` (automated tests) simply
//! means no tray. The tray itself enforces one instance per endpoint, so
//! spawning on every serve needs no duplicate check here.

use std::process::Command;

fn suppressed() -> bool {
    std::env::var_os("HESTIA_NO_TRAY").is_some_and(|value| !value.is_empty() && value != "0")
}

#[cfg(all(unix, not(target_os = "macos")))]
fn display_available() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|var| std::env::var_os(var).is_some_and(|value| !value.is_empty()))
}

#[cfg(any(windows, target_os = "macos"))]
fn display_available() -> bool {
    true
}

pub fn spawn() {
    if suppressed() {
        tracing::debug!("HESTIA_NO_TRAY set; not spawning the tray");
        return;
    }
    if !display_available() {
        tracing::debug!("no interactive display; not spawning the tray");
        return;
    }
    let Some(program) = common::paths::sibling_binary(common::app::TRAY_BIN) else {
        tracing::debug!("tray binary not found in this layout; not spawning");
        return;
    };

    let mut cmd = Command::new(&program);
    common::process::detach(&mut cmd);

    match cmd.spawn() {
        Ok(child) => tracing::info!(pid = child.id(), "tray spawned"),
        Err(e) => tracing::warn!("cannot spawn the tray: {e}"),
    }
}
