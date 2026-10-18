//! Selecting an instance and rendering one: the vocabulary the verb modules
//! share.

use anyhow::{bail, Context, Result};
use client::proto::instance::{InstanceDetails, InstanceInfo};
use client::proto::process::{ProcessInfo, ProcessState};
use client::Client;

use crate::commands::mc;
use crate::ui::{self, View};

pub(super) fn pick_instance(
    instances: Vec<InstanceInfo>,
    provided: Option<String>,
) -> Result<InstanceInfo> {
    if instances.is_empty() {
        bail!("no instances yet (hestia instance create)");
    }
    if let Some(reference) = provided {
        return instances
            .into_iter()
            .find(|i| client::proto::naming::reference_matches(&reference, &i.id, &i.name))
            .with_context(|| format!("no instance matches '{reference}'"));
    }
    let labels: Vec<String> = instances
        .iter()
        .map(|i| format!("{} ({} {})", i.name, i.flavor, i.game_version))
        .collect();
    let index = ui::select("select an instance", &labels)?;
    Ok(instances.into_iter().nth(index).expect("selector index"))
}

pub(super) fn running_process(info: &InstanceInfo) -> Option<ProcessInfo> {
    info.sessions
        .iter()
        .find(|p| p.state == ProcessState::Running)
        .cloned()
}

/// Fetch one instance by name or id (any spelling that slugs the same).
pub(super) async fn fetch(client: &Client, reference: &str) -> Result<InstanceInfo> {
    client
        .instance()
        .list()
        .await?
        .into_iter()
        .find(|i| client::proto::naming::reference_matches(reference, &i.id, &i.name))
        .with_context(|| format!("no instance matches '{reference}'"))
}

/// The short handle for a session: the sequence suffix of its process id
/// (`instance-modded_2` → `2`), which is what the user targets with `--session`.
pub(super) fn session_handle(process: &ProcessInfo) -> &str {
    process.id.rsplit('_').next().unwrap_or(&process.id)
}

/// Resolve a `--session` argument (the short handle or the full process id)
/// against an instance's sessions, returning the full process id.
pub(super) fn resolve_session(info: &InstanceInfo, input: &str) -> Result<String> {
    info.sessions
        .iter()
        .find(|s| s.id == input || session_handle(s) == input)
        .map(|s| s.id.clone())
        .with_context(|| {
            let running: Vec<&str> = info
                .sessions
                .iter()
                .filter(|s| s.state == ProcessState::Running)
                .map(session_handle)
                .collect();
            let listed = if running.is_empty() {
                "none running".to_string()
            } else {
                running.join(", ")
            };
            format!(
                "instance '{}' has no session '{input}' (running: {listed})",
                info.name
            )
        })
}

pub(super) async fn list(client: &Client) -> Result<()> {
    let instances = client.instance().list().await?;
    if instances.is_empty() {
        return ui::show(View::note("no instances yet (hestia instance create)"));
    }
    let rows = instances
        .iter()
        .map(|i| {
            vec![
                i.name.clone(),
                i.flavor.clone(),
                i.game_version.clone(),
                i.loader_version.clone().unwrap_or_else(|| "-".into()),
                mc::sessions_label(&i.sessions),
                mc::last_played_label(i.last_played_unix),
            ]
        })
        .collect();
    ui::show(View::table(
        "instances",
        [
            "NAME",
            "FLAVOR",
            "VERSION",
            "LOADER",
            "STATE",
            "LAST PLAYED",
        ],
        rows,
    ))
}

pub(super) fn show_info(info: &InstanceInfo) -> Result<()> {
    ui::show(View::detail([
        ("name", info.name.clone()),
        ("id", info.id.clone()),
        ("flavor", info.flavor.clone()),
        ("version", info.game_version.clone()),
        (
            "loader",
            info.loader_version.clone().unwrap_or_else(|| "-".into()),
        ),
        ("java", info.java_major.to_string()),
        ("state", mc::sessions_label(&info.sessions)),
        ("last played", mc::last_played_label(info.last_played_unix)),
        ("playtime", mc::playtime_label(info.playtime_seconds)),
    ]))?;
    show_sessions(&info.sessions)
}

/// The `info` command's richer view: the static detail (locations, footprint)
/// alongside the live sessions.
pub(super) fn show_detail(info: &InstanceDetails, sessions: &[ProcessInfo]) -> Result<()> {
    ui::show(View::detail([
        ("name", info.name.clone()),
        ("id", info.id.clone()),
        ("flavor", info.flavor.clone()),
        ("version", info.game_version.clone()),
        (
            "loader",
            info.loader_version.clone().unwrap_or_else(|| "-".into()),
        ),
        ("java", info.java_major.to_string()),
        ("state", mc::sessions_label(sessions)),
        ("last played", mc::last_played_label(info.last_played_unix)),
        ("playtime", mc::playtime_label(info.playtime_seconds)),
        ("disk", ui::human_bytes(info.disk_bytes)),
        ("folder", info.entry_dir.clone()),
        ("data", info.data_dir.clone()),
    ]))?;
    show_sessions(sessions)
}

fn show_sessions(sessions: &[ProcessInfo]) -> Result<()> {
    if !sessions.is_empty() {
        let rows = sessions
            .iter()
            .map(|s| {
                vec![
                    session_handle(s).to_string(),
                    s.pid.to_string(),
                    s.state.to_string(),
                ]
            })
            .collect();
        ui::show(View::table("sessions", ["SESSION", "PID", "STATE"], rows))?;
    }
    Ok(())
}
