//! Driving a provisioned server through the supervisor: start, stop, restart,
//! remove, and its captured output.

use anyhow::{Context, Result};
use client::proto::error::{EntryKind, EntryTarget};
use client::Client;

use super::entry;
use crate::commands::process;
use crate::ui::{self, Spinner, View};

fn server_target(server: &str) -> EntryTarget {
    EntryTarget::new(EntryKind::Server, server)
}

pub(crate) async fn start(client: &Client, server: &str) -> Result<()> {
    let pid = start_quiet(client, server).await?;
    ui::show(View::line(format!("server '{server}' started (pid {pid})")))
}

/// Start without the stdout line — the attach path prints its outcome only
/// after the console session ends, so nothing lands in the shell between the
/// prompt and the alternate screen (which some terminals duplicate into
/// scrollback).
pub(crate) async fn start_quiet(client: &Client, server: &str) -> Result<u32> {
    let started = {
        let _spinner = Spinner::start(format!("starting '{server}'"));
        client.server().start(server).await?
    };
    Ok(started.pid)
}

pub(crate) async fn stop(client: &Client, server: &str) -> Result<()> {
    {
        let _spinner = Spinner::start(format!("stopping '{server}'"));
        client.entry().stop(&server_target(server), None).await?;
    }
    ui::show(View::line(format!("server '{server}' stopped")))
}

pub(crate) async fn restart(client: &Client, server: &str) -> Result<()> {
    let pid = restart_quiet(client, server).await?;
    ui::show(View::line(format!(
        "server '{server}' restarted (pid {pid})"
    )))
}

/// Restart without the stdout line, for the attach path — see `start_quiet`.
pub(crate) async fn restart_quiet(client: &Client, server: &str) -> Result<u32> {
    let restarted = {
        let _spinner = Spinner::start(format!("restarting '{server}'"));
        client.server().restart(server).await?
    };
    Ok(restarted.pid)
}

/// Read (or follow) the server's captured output. Following is scoped to the
/// **server**, not to the process it happens to be running: a stop leaves the
/// stream open and the next start resumes it, and a stopped server can be
/// followed from the start.
pub(crate) async fn logs(
    client: &Client,
    server: &str,
    tail: Option<usize>,
    follow: bool,
) -> Result<()> {
    let lines = client
        .entry()
        .logs(&server_target(server), None, tail)
        .await?;
    if lines.is_empty() && !follow {
        return ui::show(View::note("no output captured (has it been started?)"));
    }
    for line in lines {
        ui::show(View::line(line.line))?;
    }
    if !follow {
        return Ok(());
    }
    let info = client.server().status(server).await?;
    let key = client::proto::naming::server_process_id(&info.id);
    crate::commands::lifecycle::follow_logs(client, "server", &info.name, &key, false).await
}

/// Run the fullscreen resource monitor over the running server's process,
/// filtering the daemon's metrics stream to it and feeding the graph.
pub(crate) async fn monitor(client: &Client, server: &str) -> Result<()> {
    let info = client.server().status(server).await?;
    let process = entry::running_process(&info)
        .with_context(|| format!("server '{}' is not running", info.name))?;
    let title = format!("{} — resources", info.name);
    process::monitor_process(client, process.id, title).await
}
