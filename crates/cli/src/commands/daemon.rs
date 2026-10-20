//! `hestia daemon …` — daemon lifecycle.

use anyhow::{bail, Result};
use clap::Subcommand;
use client::proto::process::ProcessState;
use client::Client;

use crate::exit::ExitStatus;
use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum DaemonCmd {
    /// Running (pid, uptime, home, log) or stopped
    Status,
    /// Start the daemon
    Start {
        /// Load no third-party plugins, for this run only
        #[arg(long)]
        safe_mode: bool,
    },
    /// Stop the daemon; asks what to do with running workloads (--all stops
    /// them too, --keep leaves them running)
    Stop {
        /// Also stop every supervised process (servers, instances)
        #[arg(long, conflicts_with = "keep")]
        all: bool,
        /// Leave supervised processes running
        #[arg(long)]
        keep: bool,
    },
    /// Stop then start; picks up a newly built hestiad. Supervised processes
    /// keep running and are re-adopted
    Restart {
        /// Load no third-party plugins, for this run only
        #[arg(long)]
        safe_mode: bool,
    },
}

const SAFE_MODE_NOTICE: &str =
    "safe mode: third-party plugins are not loaded; restart the daemon without --safe-mode or HESTIA_SAFE_MODE to leave it";

pub async fn run(cmd: DaemonCmd) -> Result<ExitStatus> {
    match cmd {
        // The one verb whose *answer* is the exit code: a stopped daemon is
        // reported, not an error, so it exits 3 rather than 0 or 1.
        DaemonCmd::Status => {
            return match super::connect_running().await {
                Ok(client) => {
                    let s = client.daemon().status().await?;
                    ui::show(View::line("running"))?;
                    ui::show(View::detail([
                        ("pid", s.pid.to_string()),
                        ("uptime", format!("{}s", s.uptime_seconds)),
                        ("network", network_line(client.net().status().await.ok())),
                        ("home", s.home.display().to_string()),
                        ("log", s.log.display().to_string()),
                    ]))?;
                    if s.safe_mode {
                        ui::show(View::note(SAFE_MODE_NOTICE))?;
                    }
                    ui::show_warnings(&s.quarantined)?;
                    Ok(ExitStatus::Active)
                }
                Err(_) => {
                    ui::show(View::line("stopped"))?;
                    Ok(ExitStatus::Inactive)
                }
            }
        }
        DaemonCmd::Start { safe_mode } => {
            let client = super::start(safe_mode).await?;
            let running_safe = client.daemon().status().await?.safe_mode;
            if safe_mode && !running_safe {
                bail!("the daemon is already running with plugins; restart it with `hestia daemon restart --safe-mode`");
            }
            let info = client.app().info().await?;
            ui::show(View::line(format!(
                "hestiad running ({} {})",
                info.name, info.version
            )))?;
            if running_safe {
                ui::show(View::note(SAFE_MODE_NOTICE))?;
            }
        }
        DaemonCmd::Stop { all, keep } => match super::connect_running().await {
            Ok(client) => {
                let (stop_processes, running) = stop_choice(&client, all, keep).await?;
                client.daemon().stop(stop_processes).await?;
                ui::show(View::line("hestiad stopping"))?;
                if !stop_processes && !running.is_empty() {
                    let verb = if running.len() == 1 { "keeps" } else { "keep" };
                    ui::show(View::note(format!(
                        "{} {verb} running",
                        summarize(&running)
                    )))?;
                }
            }
            Err(_) => ui::show(View::line("hestiad is not running"))?,
        },
        DaemonCmd::Restart { safe_mode } => {
            if let Ok(client) = super::connect_running().await {
                let _ = client.daemon().stop(false).await;
                super::await_stopped(&client).await?;
            }
            let client = super::start(safe_mode).await?;
            let info = client.app().info().await?;
            ui::show(View::line(format!(
                "hestiad restarted ({} {})",
                info.name, info.version
            )))?;
            if client.daemon().status().await?.safe_mode {
                ui::show(View::note(SAFE_MODE_NOTICE))?;
            }
        }
    }
    Ok(ExitStatus::Active)
}

/// Resolve what `daemon stop` means this time. Stopping the daemon has three
/// meanings, not two — stop it alone, stop it with its workloads, or *ask* —
/// and with workloads running the user's intent is genuinely ambiguous, so an
/// unqualified stop never guesses: it prompts on a terminal and refuses when
/// piped, naming both flags. With nothing running there is nothing to decide.
async fn stop_choice(client: &Client, all: bool, keep: bool) -> Result<(bool, Vec<String>)> {
    if all {
        return Ok((true, Vec::new()));
    }
    let running = running_workloads(client).await?;
    if keep || running.is_empty() {
        return Ok((false, running));
    }
    if !ui::is_interactive() {
        bail!(
            "{} still running: {}; pass --all to stop them too or --keep to leave them running",
            summarize(&running),
            running.join(", ")
        );
    }
    let stop = ui::prompt_confirm(&format!(
        "{} still running ({}) — stop them too?",
        summarize(&running),
        running.join(", ")
    ))?;
    Ok((stop, running))
}

async fn running_workloads(client: &Client) -> Result<Vec<String>> {
    let processes = client.process().list().await?;
    let mut running: Vec<String> = processes
        .into_iter()
        .filter(|p| p.state == ProcessState::Running)
        .map(|p| p.id)
        .collect();
    if running.is_empty() {
        return Ok(running);
    }
    let servers = client.server().list().await.unwrap_or_default();
    let instances = client.instance().list().await.unwrap_or_default();
    for id in &mut running {
        if let Some(sid) = id.strip_prefix("server-") {
            if let Some(s) = servers.iter().find(|s| s.id == sid) {
                *id = format!("server \"{}\"", s.name);
            }
        } else if let Some(iid) = id.strip_prefix("instance-") {
            if let Some(i) = instances.iter().find(|i| i.id == iid) {
                *id = format!("instance \"{}\"", i.name);
            }
        }
    }
    Ok(running)
}

fn summarize(running: &[String]) -> String {
    if running.len() == 1 {
        "1 supervised process".to_string()
    } else {
        format!("{} supervised processes", running.len())
    }
}

fn network_line(status: Option<client::proto::net::NetworkStatus>) -> String {
    use client::proto::net::NetworkState;
    let Some(status) = status else {
        return "unknown".to_string();
    };
    match status.state {
        NetworkState::Online => "online".to_string(),
        NetworkState::Offline if status.offline_mode => {
            "offline (pinned by network.offline)".to_string()
        }
        NetworkState::Offline => "offline — upstream is unreachable".to_string(),
        NetworkState::Unknown => "checking".to_string(),
    }
}
