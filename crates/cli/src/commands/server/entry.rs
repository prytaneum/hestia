//! Selecting a server and rendering one: the vocabulary the verb modules share.

use anyhow::{bail, Context, Result};
use client::proto::process::{ProcessInfo, ProcessState};
use client::proto::server::{JvmArgsSource, ServerDetails, ServerInfo, ServerPingResult};
use client::Client;

use crate::commands::mc;
use crate::ui::{self, View};

pub(super) fn pick_server(
    servers: Vec<ServerInfo>,
    provided: Option<String>,
) -> Result<ServerInfo> {
    if servers.is_empty() {
        bail!("no servers yet (hestia server create)");
    }
    if let Some(reference) = provided {
        return servers
            .into_iter()
            .find(|s| client::proto::naming::reference_matches(&reference, &s.id, &s.name))
            .with_context(|| format!("no server matches '{reference}'"));
    }
    let labels: Vec<String> = servers
        .iter()
        .map(|s| format!("{} ({} {})", s.name, s.flavor, s.game_version))
        .collect();
    let index = ui::select("select a server", &labels)?;
    Ok(servers.into_iter().nth(index).expect("selector index"))
}

pub(super) fn running_process(info: &ServerInfo) -> Option<ProcessInfo> {
    info.process
        .clone()
        .filter(|p| p.state == ProcessState::Running)
}

pub(super) async fn list(client: &Client) -> Result<()> {
    let servers = client.server().list().await?;
    if servers.is_empty() {
        return ui::show(View::note("no servers yet (hestia server create)"));
    }
    let rows = servers
        .iter()
        .map(|s| {
            vec![
                s.name.clone(),
                s.flavor.clone(),
                s.game_version.clone(),
                s.loader_version.clone().unwrap_or_else(|| "-".into()),
                address_label(s),
                state_label(s),
            ]
        })
        .collect();
    ui::show(View::table(
        "servers",
        ["NAME", "FLAVOR", "VERSION", "LOADER", "ADDRESS", "STATE"],
        rows,
    ))
}

pub(super) fn show_status(info: &ServerInfo, ping: Option<&ServerPingResult>) -> Result<()> {
    let mut rows = vec![
        ("name", info.name.clone()),
        ("id", info.id.clone()),
        ("flavor", info.flavor.clone()),
        ("version", info.game_version.clone()),
        (
            "loader",
            info.loader_version.clone().unwrap_or_else(|| "-".into()),
        ),
        ("java", info.java_major.to_string()),
        ("address", address_label(info)),
        (
            "console",
            if info.console { "yes" } else { "on next start" }.into(),
        ),
        ("state", state_label(info)),
    ];
    if let Some(ping) = ping {
        rows.push((
            "players",
            format!("{}/{}", ping.players_online, ping.players_max),
        ));
        if !ping.motd.is_empty() {
            rows.push(("motd", ping.motd.clone()));
        }
    }
    ui::show(View::detail(rows))
}

pub(super) fn show_info(info: &ServerDetails) -> Result<()> {
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
        ("address", format!("localhost:{}", info.game_port)),
        ("disk", ui::human_bytes(info.disk_bytes)),
        ("jvm args", jvm_args_label(info)),
        ("folder", info.entry_dir.clone()),
        ("data", info.data_dir.clone()),
    ]))?;
    ui::show_warnings(&info.warnings)
}

/// The flags the server starts with, named by where they came from. A flavor's
/// own recommendation is the one source the user never typed, so saying which
/// layer won is the point — `config get jvm-args` answers for the setting, this
/// answers for the launch.
fn jvm_args_label(info: &ServerDetails) -> String {
    let source = match info.jvm_args_source {
        JvmArgsSource::None => return "-".into(),
        JvmArgsSource::Entry => "set here",
        JvmArgsSource::Defaults => "from defaults.jvm-args",
        JvmArgsSource::Flavor => "recommended by the flavor",
    };
    format!("{} ({source})", info.jvm_args.join(" "))
}

fn address_label(info: &ServerInfo) -> String {
    format!("localhost:{}", info.game_port)
}

fn state_label(info: &ServerInfo) -> String {
    if !info.ready {
        return "provisioning".into();
    }
    mc::process_state_label(&info.process)
}
