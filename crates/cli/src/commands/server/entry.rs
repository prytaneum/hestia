//! Selecting a server and rendering one: the vocabulary the verb modules share.

use anyhow::{bail, Context, Result};
use client::proto::error::EntryKind;
use client::proto::process::{ProcessInfo, ProcessState};
use client::proto::server::{JvmArgsSource, ServerDetails, ServerInfo, ServerPingResult};
use client::Client;
use serde_json::json;

use crate::commands::mc;
use crate::commands::names::Names;
use crate::ui::{self, Cell, View};

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
        return ui::show(View::empty("no servers yet (hestia server create)"));
    }
    let names = Names::flavors(client, EntryKind::Server).await?;
    let rows = servers
        .iter()
        .map(|s| {
            vec![
                s.name.clone().into(),
                names.show(&s.flavor).into(),
                s.game_version.clone().into(),
                Cell::maybe(s.loader_version.clone(), "-"),
                address_label(s).into(),
                state_cell(s),
            ]
        })
        .collect();
    ui::show(View::table(
        "servers",
        ["NAME", "FLAVOR", "VERSION", "LOADER", "ADDRESS", "STATE"],
        rows,
    ))
}

pub(super) async fn show_status(
    client: &Client,
    info: &ServerInfo,
    ping: Option<&ServerPingResult>,
) -> Result<()> {
    let names = Names::flavors(client, EntryKind::Server).await?;
    let mut rows = vec![
        ("name", Cell::from(info.name.clone())),
        ("id", info.id.clone().into()),
        ("flavor", names.show(&info.flavor).into()),
        ("version", info.game_version.clone().into()),
        ("loader", Cell::maybe(info.loader_version.clone(), "-")),
        ("java", Cell::count(info.java_major)),
        ("address", address_label(info).into()),
        (
            "console",
            Cell::flag(
                info.console,
                if info.console { "yes" } else { "on next start" },
            ),
        ),
        ("state", state_cell(info)),
    ];
    if let Some(ping) = ping {
        rows.push((
            "players",
            Cell::typed(
                format!("{}/{}", ping.players_online, ping.players_max),
                json!({ "online": ping.players_online, "max": ping.players_max }),
            ),
        ));
        if !ping.motd.is_empty() {
            rows.push(("motd", ping.motd.clone().into()));
        }
    }
    ui::show(View::detail(rows))
}

pub(super) async fn show_info(client: &Client, info: &ServerDetails) -> Result<()> {
    let names = Names::flavors(client, EntryKind::Server).await?;
    ui::show(View::detail([
        ("name", Cell::from(info.name.clone())),
        ("id", info.id.clone().into()),
        ("flavor", names.show(&info.flavor).into()),
        ("version", info.game_version.clone().into()),
        ("loader", Cell::maybe(info.loader_version.clone(), "-")),
        ("java", Cell::count(info.java_major)),
        ("address", format!("localhost:{}", info.game_port).into()),
        ("disk", Cell::bytes(info.disk_bytes)),
        ("jvm args", jvm_args_cell(info)),
        ("folder", info.entry_dir.clone().into()),
        ("data", info.data_dir.clone().into()),
    ]))?;
    ui::show_warnings(&info.warnings)
}

/// The flags the server starts with, named by where they came from. A flavor's
/// own recommendation is the one source the user never typed, so saying which
/// layer won is the point — `config get jvm-args` answers for the setting, this
/// answers for the launch.
fn jvm_args_cell(info: &ServerDetails) -> Cell {
    let source = match info.jvm_args_source {
        JvmArgsSource::None => return Cell::absent("-"),
        JvmArgsSource::Entry => "set here",
        JvmArgsSource::Defaults => "from defaults.jvm-args",
        JvmArgsSource::Flavor => "recommended by the flavor",
    };
    Cell::typed(
        format!("{} ({source})", info.jvm_args.join(" ")),
        json!({ "args": info.jvm_args, "source": info.jvm_args_source }),
    )
}

fn address_label(info: &ServerInfo) -> String {
    format!("localhost:{}", info.game_port)
}

fn state_cell(info: &ServerInfo) -> Cell {
    if !info.ready {
        return Cell::token("provisioning", &"provisioning");
    }
    mc::process_state_cell(&info.process)
}
