//! `hestia process …` — the supervisor's own view.
//!
//! Every workload is normally driven through the entry that owns it
//! (`server smp start`, `instance modded logs`), which is the grammar most
//! people want. This is the layer underneath: what the daemon is supervising
//! right now, across every entry, keyed the way the supervisor keys it. It is
//! the one view no entry-scoped verb can give — a server's `status` cannot show
//! you an instance session, and neither shows a process whose entry was removed
//! out from under it.

use anyhow::Result;
use clap::Subcommand;
use client::proto::process::{ProcessInfo, ProcessState};

use crate::commands::connect;
use crate::exit::ExitStatus;
use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum ProcessCmd {
    /// Everything the daemon is supervising
    #[command(visible_alias = "ls")]
    List,
    /// One process by its supervisor id (see `list`)
    Status {
        /// Supervisor id, e.g. `server-<id>` or `instance-<id>_1`
        id: String,
    },
    /// A process's captured output
    Logs {
        /// Supervisor id (see `list`)
        id: String,
        #[arg(short = 'n', long = "tail", help = "Only the last N lines")]
        tail: Option<usize>,
    },
    /// Stop a supervised process (SIGTERM, then a hard kill)
    Stop {
        /// Supervisor id (see `list`)
        id: String,
    },
}

pub async fn run(cmd: ProcessCmd) -> Result<ExitStatus> {
    match cmd {
        ProcessCmd::Status { id } => status(&id).await,
        ProcessCmd::List => list().await.map(|()| ExitStatus::Active),
        ProcessCmd::Logs { id, tail } => logs(&id, tail).await.map(|()| ExitStatus::Active),
        ProcessCmd::Stop { id } => stop(&id).await.map(|()| ExitStatus::Active),
    }
}

async fn list() -> Result<()> {
    let client = connect().await?;
    let processes = client.process().list().await?;
    if processes.is_empty() {
        return ui::show(View::note("nothing is being supervised"));
    }
    let rows = processes
        .iter()
        .map(|p| {
            vec![
                p.id.clone(),
                state_label(p),
                p.pid.to_string(),
                p.program.clone(),
            ]
        })
        .collect();
    ui::show(View::table(
        "Supervised processes",
        ["ID", "STATE", "PID", "PROGRAM"],
        rows,
    ))
}

/// A state query, so it answers through the exit code too (see `exit.rs`).
async fn status(id: &str) -> Result<ExitStatus> {
    let client = connect().await?;
    let info = client.process().status(id).await?;
    ui::show(View::detail([
        ("id", info.id.clone()),
        ("state", state_label(&info)),
        ("pid", info.pid.to_string()),
        ("program", info.program.clone()),
        ("args", info.args.join(" ")),
        (
            "started",
            crate::commands::mc::last_played_label(Some(info.started_unix)),
        ),
    ]))?;
    Ok(ExitStatus::running(info.state == ProcessState::Running))
}

async fn logs(id: &str, tail: Option<usize>) -> Result<()> {
    let client = connect().await?;
    let lines = client.process().logs(id, tail).await?;
    if lines.is_empty() {
        return ui::show(View::note("no output captured"));
    }
    for line in lines {
        ui::show(View::line(line.line))?;
    }
    Ok(())
}

async fn stop(id: &str) -> Result<()> {
    let client = connect().await?;
    client.process().stop(id).await?;
    ui::show(View::line(format!("process '{id}' stopping")))
}

/// The exit code is part of a terminal state's story — an adopted process
/// reports none, which is a known gap, not a missing value.
fn state_label(info: &ProcessInfo) -> String {
    let state = info.state.to_string();
    match info.exit_code {
        Some(code) if info.state != ProcessState::Running => format!("{state} ({code})"),
        _ => state,
    }
}
