//! What was played lately across every instance: listing it, and picking one
//! to go back to.

use anyhow::{bail, Result};
use client::proto::instance::{QuickPlay, RecentPlay};
use client::Client;

use crate::commands::mc;
use crate::ui::{self, View};

pub(super) async fn list(client: &Client) -> Result<()> {
    let plays = client.instance().recent().await?;
    if plays.is_empty() {
        return ui::show(View::note(EMPTY));
    }
    let rows = plays
        .iter()
        .map(|play| {
            vec![
                play.name.clone(),
                kind_label(&play.target).to_string(),
                target(&play.target).to_string(),
                play.instance_name.clone(),
                format!("{:?}", play.game_mode).to_lowercase(),
                mc::last_played_label(Some(play.played_unix)),
            ]
        })
        .collect();
    ui::show(View::table(
        "Recently played",
        ["NAME", "TYPE", "TARGET", "INSTANCE", "MODE", "PLAYED"],
        rows,
    ))
}

pub async fn pick(client: &Client) -> Result<RecentPlay> {
    let plays = client.instance().recent().await?;
    if plays.is_empty() {
        bail!(EMPTY);
    }
    let labels: Vec<String> = plays
        .iter()
        .map(|play| {
            format!(
                "{} ({}, {})",
                play.name,
                kind_label(&play.target),
                play.instance_name
            )
        })
        .collect();
    let index = ui::select("where do you want to go back to?", &labels)?;
    Ok(plays.into_iter().nth(index).expect("selector index"))
}

const EMPTY: &str = "nothing played yet — join a world or a server from a 1.20+ instance first";

/// What the matching `--world` / `--server` / `--realm` flag takes.
fn target(target: &QuickPlay) -> &str {
    match target {
        QuickPlay::World(target) | QuickPlay::Server(target) | QuickPlay::Realm(target) => target,
    }
}

fn kind_label(target: &QuickPlay) -> &'static str {
    match target {
        QuickPlay::World(_) => "world",
        QuickPlay::Server(_) => "server",
        QuickPlay::Realm(_) => "realm",
    }
}
