//! Locating and auto-starting the daemon. `hestiad` is found in the running
//! front-end's own layout, else resolved through PATH.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use ipc::Connection;

/// Start the daemon detached from this process's session, so it outlives the
/// front-end that spawned it.
pub fn spawn_daemon(safe_mode: bool) -> std::io::Result<()> {
    let names = common::app::DAEMON_BIN;
    let program = common::paths::sibling_binary(names).unwrap_or_else(|| PathBuf::from(names[0]));
    tracing::debug!(program = %program.display(), "spawning the daemon");
    let mut cmd = Command::new(program);
    cmd.arg("serve");
    if safe_mode {
        cmd.arg("--safe-mode");
    }
    // Started from the AppImage's shell, it would hand that environment on to
    // every game it launches; it links nothing from the image.
    if let Some(env) = common::appimage::sidecar_env() {
        cmd.env_clear().envs(env);
    }

    common::process::detach(&mut cmd);
    cmd.spawn().map(|_| ())
}

/// Poll briefly for a freshly-spawned daemon's endpoint to appear.
pub async fn connect_with_retry(endpoint: &std::path::Path) -> Option<Connection> {
    for attempt in 1..=60 {
        if let Ok(conn) = ipc::connect(endpoint).await {
            tracing::debug!(attempt, "connected to the spawned daemon");
            return Some(conn);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tracing::warn!(
        endpoint = %endpoint.display(),
        "spawned daemon never came up on its endpoint"
    );
    None
}
