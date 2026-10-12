//! `hestia server <name> players …` — who is online and who is on the
//! whitelist, operator and ban lists. The lists read from a stopped server
//! too; changing one needs it running.

use anyhow::Result;
use clap::Subcommand;
use client::proto::players::{PlayerEntry, PlayerList};
use client::Client;

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum PlayersCmd {
    /// Who is online, and the whitelist, operator and ban lists
    #[command(visible_alias = "ls")]
    List,
    /// Add players to the whitelist
    Allow {
        #[arg(required = true)]
        players: Vec<String>,
    },
    /// Remove players from the whitelist
    Disallow {
        #[arg(required = true)]
        players: Vec<String>,
    },
    /// Make players operators
    Op {
        #[arg(required = true)]
        players: Vec<String>,
    },
    /// Take operator status away
    Deop {
        #[arg(required = true)]
        players: Vec<String>,
    },
    /// Ban players
    Ban {
        #[arg(required = true)]
        players: Vec<String>,
    },
    /// Lift a ban
    Pardon {
        #[arg(required = true)]
        players: Vec<String>,
    },
    /// Disconnect players who are online
    Kick {
        #[arg(required = true)]
        players: Vec<String>,
    },
}

pub(super) async fn run(client: &Client, server: &str, cmd: PlayersCmd) -> Result<()> {
    let (players, list, listed, done) = match cmd {
        PlayersCmd::List => return list(client, server).await,
        PlayersCmd::Kick { players } => {
            for player in &players {
                client.players().kick(server, player).await?;
            }
            return ui::show(View::line(format!("kicked {}", players.join(", "))));
        }
        PlayersCmd::Allow { players } => (players, PlayerList::Whitelist, true, "whitelisted"),
        PlayersCmd::Disallow { players } => (
            players,
            PlayerList::Whitelist,
            false,
            "removed from the whitelist",
        ),
        PlayersCmd::Op { players } => (players, PlayerList::Ops, true, "made operator"),
        PlayersCmd::Deop { players } => (players, PlayerList::Ops, false, "no longer operator"),
        PlayersCmd::Ban { players } => (players, PlayerList::Banned, true, "banned"),
        PlayersCmd::Pardon { players } => (players, PlayerList::Banned, false, "pardoned"),
    };
    for player in &players {
        client.players().set(server, player, list, listed).await?;
    }
    ui::show(View::line(format!("{done}: {}", players.join(", "))))
}

async fn list(client: &Client, server: &str) -> Result<()> {
    let players = client.players().list(server).await?;
    let names = |entries: &[PlayerEntry]| match entries.is_empty() {
        true => "—".to_string(),
        false => entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    };
    let online = match players.online.is_empty() {
        true => "—".to_string(),
        false => players.online.join(", "),
    };
    ui::show(View::detail([
        ("online", online),
        ("whitelist", names(&players.whitelist)),
        ("operators", names(&players.ops)),
        ("banned", names(&players.banned)),
    ]))
}
