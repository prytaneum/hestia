//! Selecting an instance and rendering one: the vocabulary the verb modules
//! share.

use anyhow::{bail, Context, Result};
use client::proto::error::EntryKind;
use client::proto::instance::{InstanceDetails, InstanceInfo};
use client::proto::process::{ProcessInfo, ProcessState};
use client::Client;

use crate::commands::mc;
use crate::commands::names::Names;
use crate::ui::{self, Cell, View};

pub(super) fn pick_instance(
    instances: Vec<InstanceInfo>,
    provided: Option<String>,
) -> Result<InstanceInfo> {
    if instances.is_empty() {
        bail!("no instances yet (hestia instance create)");
    }
    if let Some(reference) = provided {
        return mc::find_by_reference(instances, &reference, |i| (&i.id, &i.name))
            .with_context(|| format!("no instance matches '{reference}'"));
    }
    let labels: Vec<String> = instances
        .iter()
        .map(|i| mc::entry_label(&i.name, &i.flavor, &i.game_version))
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
    let instances = client.instance().list().await?;
    mc::find_by_reference(instances, reference, |i| (&i.id, &i.name))
        .with_context(|| format!("no instance matches '{reference}'"))
}

/// The short handle for a session: the sequence suffix of its process id
/// (`instance-modded_2` → `2`), which is what the user targets with `--session`.
pub(super) fn session_handle(process: &ProcessInfo) -> &str {
    session_handle_of(&process.id)
}

pub(super) fn session_handle_of(process_id: &str) -> &str {
    process_id.rsplit('_').next().unwrap_or(process_id)
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
        return ui::show(View::empty("no instances yet (hestia instance create)"));
    }
    let names = Names::flavors(client, EntryKind::Instance).await?;
    let rows = instances
        .iter()
        .map(|i| {
            vec![
                i.name.clone().into(),
                names.show(&i.flavor).into(),
                i.game_version.clone().into(),
                Cell::maybe(i.loader_version.clone(), "-"),
                mc::sessions_cell(&i.sessions),
                mc::last_played_cell(i.last_played_unix),
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

pub(super) async fn show_info(client: &Client, info: &InstanceInfo) -> Result<()> {
    let names = Names::flavors(client, EntryKind::Instance).await?;
    ui::show(View::detail([
        ("name", Cell::from(info.name.clone())),
        ("id", info.id.clone().into()),
        ("flavor", names.show(&info.flavor).into()),
        ("version", info.game_version.clone().into()),
        ("loader", Cell::maybe(info.loader_version.clone(), "-")),
        ("java", Cell::count(info.java_major)),
        ("state", mc::sessions_cell(&info.sessions)),
        ("last played", mc::last_played_cell(info.last_played_unix)),
        ("playtime", mc::playtime_cell(info.playtime_seconds)),
    ]))?;
    show_sessions(&info.sessions)
}

/// The `info` command's richer view: the static detail (locations, footprint)
/// alongside the live sessions.
pub(super) async fn show_detail(
    client: &Client,
    info: &InstanceDetails,
    sessions: &[ProcessInfo],
) -> Result<()> {
    let names = Names::flavors(client, EntryKind::Instance).await?;
    ui::show(View::detail([
        ("name", Cell::from(info.name.clone())),
        ("id", info.id.clone().into()),
        ("flavor", names.show(&info.flavor).into()),
        ("version", info.game_version.clone().into()),
        ("loader", Cell::maybe(info.loader_version.clone(), "-")),
        ("java", Cell::count(info.java_major)),
        ("state", mc::sessions_cell(sessions)),
        ("last played", mc::last_played_cell(info.last_played_unix)),
        ("playtime", mc::playtime_cell(info.playtime_seconds)),
        ("disk", Cell::bytes(info.disk_bytes)),
        ("folder", info.entry_dir.clone().into()),
        ("data", info.data_dir.clone().into()),
    ]))?;
    show_sessions(sessions)
}

fn show_sessions(sessions: &[ProcessInfo]) -> Result<()> {
    if !sessions.is_empty() {
        let rows = sessions
            .iter()
            .map(|s| {
                vec![
                    Cell::from(session_handle(s)),
                    Cell::count(s.pid),
                    Cell::token(s.state.to_string(), &s.state),
                ]
            })
            .collect();
        ui::show(View::table("sessions", ["SESSION", "PID", "STATE"], rows))?;
    }
    Ok(())
}
