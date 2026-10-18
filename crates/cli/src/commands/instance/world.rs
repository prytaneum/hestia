//! The instance's save worlds: listing them as each `level.dat` describes
//! itself, and launching straight into one (Quick Play).

use anyhow::{bail, Result};
use clap::Subcommand;
use client::proto::instance::{QuickPlay, WorldInfo};
use client::Client;

use crate::commands::mc;
use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum WorldCmd {
    /// Launch the instance straight into a save world (needs Minecraft 1.20+)
    Play {
        /// World folder under `data/saves/` (prompts when omitted)
        world: Option<String>,
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
}

pub(super) async fn run(client: &Client, instance: &str, cmd: WorldCmd) -> Result<()> {
    match cmd {
        WorldCmd::Play {
            world,
            account,
            detach,
            new_session,
            offline,
        } => {
            let folder = match world {
                Some(folder) => folder,
                None => pick(client, instance).await?,
            };
            super::launch(
                client,
                super::Launch {
                    reference: instance,
                    account: account.as_deref().unwrap_or_default(),
                    new_session,
                    detach,
                    quick_play: Some(QuickPlay::World(folder)),
                    offline,
                    ..super::Launch::default()
                },
            )
            .await
        }
    }
}

/// The instance's save worlds, as each `level.dat` describes itself. A
/// first-class verb, not just the datapack picker's private read: every daemon
/// capability gets a scriptable form.
pub(super) async fn list(client: &Client, instance: &str) -> Result<()> {
    let worlds = client.instance().worlds(instance).await?;
    if worlds.is_empty() {
        return ui::show(View::note("no worlds yet — create one in-game first"));
    }
    let rows = worlds
        .iter()
        .map(|world| {
            vec![
                world.name.clone(),
                world.folder.clone(),
                world.version.clone(),
                mode_label(world),
                mc::last_played_label(world.last_played_unix),
                ui::human_bytes(world.size_bytes),
            ]
        })
        .collect();
    ui::show(View::table(
        "Worlds",
        ["NAME", "FOLDER", "VERSION", "MODE", "PLAYED", "SIZE"],
        rows,
    ))
}

/// The world's game mode, with the flags that change how it plays.
fn mode_label(world: &WorldInfo) -> String {
    if !world.read {
        return "unreadable".into();
    }
    let mut label = world.game_mode.to_string();
    if world.hardcore {
        label.push_str(" hardcore");
    } else {
        label = format!("{label} ({})", world.difficulty);
    }
    if world.cheats {
        label.push_str(" +cheats");
    }
    label
}

/// A world is named by its folder on the wire, but recognised by its in-game
/// name — so the picker shows both.
async fn pick(client: &Client, instance: &str) -> Result<String> {
    let worlds = client.instance().worlds(instance).await?;
    if worlds.is_empty() {
        bail!("'{instance}' has no worlds yet — create one in-game first");
    }
    let labels: Vec<String> = worlds
        .iter()
        .map(|w| format!("{} ({})", w.name, w.folder))
        .collect();
    let index = ui::select("which world?", &labels)?;
    Ok(worlds
        .into_iter()
        .nth(index)
        .expect("selector index")
        .folder)
}
