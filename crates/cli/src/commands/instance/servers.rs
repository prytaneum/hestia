//! The instance's multiplayer list — the servers the in-game list shows —
//! and joining one straight from a launch (Quick Play).
//!
//! The list is the game's own `servers.dat`, so an edit made while a session is
//! open is answered with a warning rather than refused — the daemon says so on
//! the result and it is shown like any other degraded outcome.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::instance::{QuickPlay, ServerEntry};
use client::Client;
use futures_util::stream::{self, StreamExt};

use crate::ui::{self, View};

/// How many entries are pinged at once when listing. The list is short and each
/// ping has its own timeout, so a handful of connections keeps a stale entry
/// from holding up the ones behind it.
const PING_CONCURRENCY: usize = 8;

#[derive(Subcommand)]
pub enum ServerCmd {
    /// Launch the instance and join a server from its list (needs Minecraft 1.20+)
    Play {
        /// Server name or address from the list (prompts when omitted)
        server: Option<String>,
        #[arg(long, help = "Account name or uuid (default: the switched-to account)")]
        account: Option<String>,
        #[arg(short, long, help = "Return immediately instead of following the logs")]
        detach: bool,
        #[arg(
            long,
            help = "Launch another session even if one is already running (needs 'config set \
                    instance.multi-session true')"
        )]
        new_session: bool,
        #[arg(
            long,
            help = "Play without signing in: singleplayer only, on the last known account"
        )]
        offline: bool,
    },
    /// Add a server to the instance's multiplayer list
    Add {
        /// The name shown in the in-game list
        name: String,
        /// host or host:port (the port defaults to 25565)
        address: String,
        #[arg(long, help = "Auto-accept the server's resource pack")]
        accept_textures: bool,
    },
    /// Change a server already in the list
    Edit {
        /// The entry to change, by name or address
        server: String,
        #[arg(long, help = "New display name")]
        name: Option<String>,
        #[arg(long, help = "New host or host:port")]
        address: Option<String>,
        #[arg(long, help = "Auto-accept the server's resource pack")]
        accept_textures: bool,
    },
    /// Remove a server from the list
    Remove {
        /// The entry to remove, by name or address
        server: String,
    },
    /// Move a server to another position in the list
    Move {
        /// The entry to move, by name or address
        server: String,
        /// Its new position, counting from 1
        position: u32,
    },
}

pub(super) async fn run(client: &Client, instance: &str, cmd: ServerCmd) -> Result<()> {
    match cmd {
        ServerCmd::Play {
            server,
            account,
            detach,
            new_session,
            offline,
        } => {
            let address = match server {
                Some(server) => resolve_address(client, instance, &server).await?,
                None => pick(client, instance).await?,
            };
            super::launch(
                client,
                super::Launch {
                    reference: instance,
                    account: account.as_deref().unwrap_or_default(),
                    new_session,
                    detach,
                    quick_play: Some(QuickPlay::Server(address)),
                    offline,
                    ..super::Launch::default()
                },
            )
            .await
        }
        ServerCmd::Add {
            name,
            address,
            accept_textures,
        } => {
            let written = client
                .instance()
                .server_edit(instance, "", &name, &address, accept_textures)
                .await?;
            ui::show(View::line(format!("'{name}' added to {instance}")))?;
            ui::show_warnings(&written.warnings)
        }
        ServerCmd::Edit {
            server,
            name,
            address,
            accept_textures,
        } => {
            let current = find(client, instance, &server).await?;
            let written = client
                .instance()
                .server_edit(
                    instance,
                    &server,
                    name.as_deref().unwrap_or(&current.name),
                    address.as_deref().unwrap_or(&current.address),
                    accept_textures || current.accept_textures,
                )
                .await?;
            ui::show(View::line(format!("'{server}' updated")))?;
            ui::show_warnings(&written.warnings)
        }
        ServerCmd::Remove { server } => {
            let written = client.instance().server_remove(instance, &server).await?;
            ui::show(View::line(format!("'{server}' removed from {instance}")))?;
            ui::show_warnings(&written.warnings)
        }
        ServerCmd::Move { server, position } => {
            let order = moved(
                &visible(client.instance().servers(instance).await?),
                &server,
                position,
            )?;
            let written = client.instance().servers_arrange(instance, order).await?;
            ui::show(View::line(format!("'{server}' moved to #{position}")))?;
            ui::show_warnings(&written.warnings)
        }
    }
}

/// The instance's multiplayer list, each entry with what the server itself
/// answers right now — the same status the in-game list shows.
pub(super) async fn list(client: &Client, instance: &str) -> Result<()> {
    let servers = visible(client.instance().servers(instance).await?);
    if servers.is_empty() {
        return ui::show(View::note(
            "no servers in this instance's list — add one with `server add <name> <address>`",
        ));
    }
    let statuses = ping_all(client, &servers).await;
    let rows = servers
        .iter()
        .zip(statuses)
        .enumerate()
        .map(|(index, (server, status))| {
            vec![
                (index + 1).to_string(),
                server.name.clone(),
                server.address.clone(),
                status.unwrap_or_else(|| "offline".to_string()),
            ]
        })
        .collect();
    ui::show(View::table(
        "Servers",
        ["#", "NAME", "ADDRESS", "STATUS"],
        rows,
    ))
}

/// The list's order with `server` lifted to `position`, counting from 1. The
/// whole arrangement is what the daemon takes, so the move is resolved here
/// against the list that was just read.
fn moved(servers: &[ServerEntry], server: &str, position: u32) -> Result<Vec<String>> {
    if position == 0 || position as usize > servers.len() {
        bail!("positions run from 1 to {}", servers.len());
    }
    let index = servers
        .iter()
        .position(|s| {
            s.name.eq_ignore_ascii_case(server.trim())
                || s.address.eq_ignore_ascii_case(server.trim())
        })
        .with_context(|| format!("no server named '{server}' in the list"))?;
    let mut order: Vec<String> = servers
        .iter()
        .map(|s| {
            if s.name.is_empty() {
                s.address.clone()
            } else {
                s.name.clone()
            }
        })
        .collect();
    let entry = order.remove(index);
    order.insert(position as usize - 1, entry);
    Ok(order)
}

/// Ping every entry at once, bounded — an unreachable entry costs its own
/// timeout rather than the whole list's.
async fn ping_all(client: &Client, servers: &[ServerEntry]) -> Vec<Option<String>> {
    stream::iter(servers)
        .map(|server| async move {
            let status = client.instance().ping_address(&server.address).await.ok()?;
            Some(format!(
                "{}/{} online · {}",
                status.players_online, status.players_max, status.version
            ))
        })
        .buffered(PING_CONCURRENCY)
        .collect()
        .await
}

async fn pick(client: &Client, instance: &str) -> Result<String> {
    let servers = visible(client.instance().servers(instance).await?);
    if servers.is_empty() {
        bail!(
            "'{instance}' has no servers in its list — add one with `server add <name> <address>`"
        );
    }
    let labels: Vec<String> = servers
        .iter()
        .map(|s| format!("{} ({})", s.name, s.address))
        .collect();
    let index = ui::select("which server?", &labels)?;
    Ok(servers
        .into_iter()
        .nth(index)
        .expect("selector index")
        .address)
}

/// The address of a list entry, or the argument itself when it names no entry —
/// so `server play mc.example.net` joins an address that was never saved.
async fn resolve_address(client: &Client, instance: &str, reference: &str) -> Result<String> {
    let servers = client.instance().servers(instance).await?;
    Ok(matching(&servers, reference)
        .map(|s| s.address.clone())
        .unwrap_or_else(|| reference.to_string()))
}

async fn find(client: &Client, instance: &str, reference: &str) -> Result<ServerEntry> {
    let servers = client.instance().servers(instance).await?;
    match matching(&servers, reference) {
        Some(server) => Ok(server.clone()),
        None => bail!("no server named '{reference}' in {instance}'s list"),
    }
}

fn matching<'a>(servers: &'a [ServerEntry], reference: &str) -> Option<&'a ServerEntry> {
    let reference = reference.trim();
    servers
        .iter()
        .find(|s| s.name.eq_ignore_ascii_case(reference))
        .or_else(|| {
            servers
                .iter()
                .find(|s| s.address.eq_ignore_ascii_case(reference))
        })
}

/// The rows the in-game list shows: the game keeps hidden scratch entries of
/// its own (direct-connect), which are not the player's servers.
fn visible(servers: Vec<ServerEntry>) -> Vec<ServerEntry> {
    servers.into_iter().filter(|s| !s.hidden).collect()
}
