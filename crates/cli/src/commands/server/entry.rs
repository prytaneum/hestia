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
        return mc::find_by_reference(servers, &reference, |s| (&s.id, &s.name))
            .with_context(|| format!("no server matches '{reference}'"));
    }
    let labels: Vec<String> = servers
        .iter()
        .map(|s| mc::entry_label(&s.name, &s.flavor, &s.game_version))
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
                mc::edition_label(s.edition).into(),
                s.game_version.clone(),
                s.loader_version.clone().unwrap_or_else(|| "-".into()),
                address_label(s),
                state_label(s),
            ]
        })
        .collect();
    ui::show(View::table(
        "servers",
        [
            "NAME", "FLAVOR", "EDITION", "VERSION", "LOADER", "ADDRESS", "STATE",
        ],
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
        ("address", address_label(info)),
        (
            "console",
            if info.console { "yes" } else { "on next start" }.into(),
        ),
        ("state", state_label(info)),
    ];
    if let Some(java) = info.java_major {
        rows.insert(5, ("java", java.to_string()));
    }
    rows.insert(3, ("edition", mc::edition_label(info.edition).into()));
    if let Some(ports) = ports_label(&info.ports) {
        rows.push(("ports", ports));
    }
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
    let mut rows = vec![
        ("name", info.name.clone()),
        ("id", info.id.clone()),
        ("flavor", info.flavor.clone()),
        ("version", info.game_version.clone()),
        (
            "loader",
            info.loader_version.clone().unwrap_or_else(|| "-".into()),
        ),
        ("address", format!("localhost:{}", info.game_port)),
        ("disk", ui::human_bytes(info.disk_bytes)),
        ("folder", info.entry_dir.clone()),
        ("data", info.data_dir.clone()),
    ];
    if let Some(java) = info.java_major {
        rows.insert(5, ("java", java.to_string()));
        rows.insert(8, ("jvm args", jvm_args_label(info)));
    }
    ui::show(View::detail(rows))?;
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

/// Only a server binding a second port (Bedrock's IPv6) has more than its address to say.
fn ports_label(ports: &[u16]) -> Option<String> {
    match ports {
        [v4, v6] => Some(format!("{v4} (IPv4), {v6} (IPv6)")),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_second_port_is_worth_a_ports_row() {
        assert_eq!(ports_label(&[25565]), None);
        assert_eq!(
            ports_label(&[19132, 19133]).as_deref(),
            Some("19132 (IPv4), 19133 (IPv6)")
        );
    }
}
