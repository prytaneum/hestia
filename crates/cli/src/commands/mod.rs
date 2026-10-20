//! CLI command groups, each driving the daemon over the client SDK.

pub mod account;
pub mod cache;
pub mod config;
pub mod content;
pub mod daemon;
pub mod instance;
pub mod java;
pub mod job;
pub mod lifecycle;
mod mc;
pub mod modpack;
pub mod play;
pub mod plugin;
pub mod process;
pub mod screenshot;
pub mod server;
pub mod sync;
pub mod update;
mod wizard;

use anyhow::{Context, Result};
use client::Client;

use crate::ui::Spinner;

/// Connect to a running daemon; never spawns it.
pub async fn connect() -> Result<Client> {
    let _spinner = Spinner::start("connecting to the daemon");
    Client::connect()
        .await
        .context("the daemon is not running — start it with `hestia daemon start`")
}

/// Probe for a running daemon; callers that treat "not running" as normal
/// (status, stop) use this.
pub async fn connect_running() -> Result<Client> {
    Client::connect().await.context("the daemon is not running")
}

/// Explicitly start the daemon and connect.
pub async fn start(safe_mode: bool) -> Result<Client> {
    let _spinner = Spinner::start("starting the daemon");
    Client::start_with(safe_mode)
        .await
        .context("cannot start the daemon")
}

/// Wait for a daemon that was asked to stop to go, so a following [`start`]
/// cannot reach it on its way out.
pub async fn await_stopped(client: &Client) -> Result<()> {
    let _spinner = Spinner::start("waiting for the daemon to stop");
    tokio::time::timeout(STOP_WAIT, client.session().closed())
        .await
        .context("the daemon did not stop")
}

const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Run a daemon job, turning Ctrl-C into an explicit cancellation of it.
///
/// Ctrl-C used to kill only this process while the daemon ran the job to
/// completion — a JDK arriving minutes after the user stopped waiting for it.
/// The daemon deliberately never cancels a job because its client vanished (a
/// job outlives the client that started it, like every supervised workload), so
/// the client has to *ask*, and this is where a terminal interrupt is turned
/// into that request.
///
/// The job's own future is still awaited afterwards, so the command exits on
/// the daemon's `cancelled` event rather than guessing that the cancel took.
pub async fn cancellable<T>(
    client: &Client,
    id: &str,
    job: impl std::future::Future<Output = Result<T, client::IpcError>>,
) -> Result<T, client::IpcError> {
    tokio::pin!(job);
    loop {
        tokio::select! {
            outcome = &mut job => return outcome,
            _ = tokio::signal::ctrl_c() => {
                let _ = client.jobs().cancel(id).await;
            }
        }
    }
}
