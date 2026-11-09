//! `hestia server <name> world …` — the worlds a server holds, and bringing one
//! in from a `.mcworld` (Bedrock).

use anyhow::{Context, Result};
use clap::Subcommand;
use client::proto::instance::WorldInfo;
use client::proto::server::ServerWorldImportParams;
use client::Client;

use crate::commands::mc;
use crate::ui::{self, ProvisionReporter, View};

const HEADER: [&str; 7] = ["", "NAME", "FOLDER", "MODE", "VERSION", "PLAYED", "SIZE"];

#[derive(Subcommand)]
pub enum WorldCmd {
    /// The server's worlds; the one it runs is marked
    #[command(visible_alias = "ls")]
    List,
    /// Import a .mcworld into a stopped Bedrock server
    Import {
        /// The .mcworld file
        file: String,
        #[arg(long, help = "Make it the world the server runs")]
        r#use: bool,
    },
}

pub(super) async fn run(client: &Client, server: &str, cmd: WorldCmd) -> Result<()> {
    match cmd {
        WorldCmd::List => list(client, server).await,
        WorldCmd::Import { file, r#use } => import(client, server, file, r#use).await,
    }
}

async fn list(client: &Client, server: &str) -> Result<()> {
    let worlds = client.server().worlds(server).await?;
    if worlds.is_empty() {
        return ui::show(View::note(
            "no worlds yet — start the server to generate one",
        ));
    }
    ui::show(View::table(
        "Worlds",
        HEADER,
        worlds
            .iter()
            .enumerate()
            .map(|(i, w)| row(i == 0, w))
            .collect(),
    ))
}

/// The daemon lists the world the server runs first.
fn row(active: bool, world: &WorldInfo) -> Vec<String> {
    vec![
        if active { "*" } else { " " }.into(),
        world.name.clone(),
        world.folder.clone(),
        mc::world_mode_label(world),
        if world.version.is_empty() {
            "-".into()
        } else {
            world.version.clone()
        },
        mc::last_played_label(world.last_played_unix),
        ui::human_bytes(world.size_bytes),
    ]
}

async fn import(client: &Client, server: &str, file: String, activate: bool) -> Result<()> {
    let path = std::fs::canonicalize(&file)
        .with_context(|| format!("cannot read {file}"))?
        .to_string_lossy()
        .into_owned();
    let params = ServerWorldImportParams {
        server: server.to_string(),
        path,
        activate,
        id: String::new(),
    };
    let world = ProvisionReporter::run(None, async |on_progress| {
        client.server().import_world(params, on_progress).await
    })
    .await?;
    ui::show(View::line(format!(
        "imported world '{}' into '{server}'{}",
        world.name,
        if activate { " and set it active" } else { "" }
    )))?;
    ui::show(View::table("World", HEADER, vec![row(activate, &world)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_world_is_marked_and_a_missing_version_is_dashed() {
        let world = WorldInfo {
            name: "Bedrock level".into(),
            folder: "Bedrock level".into(),
            ..WorldInfo::default()
        };
        assert_eq!(row(true, &world)[0], "*");
        assert_eq!(row(false, &world)[0], " ");
        assert_eq!(row(true, &world)[4], "-");
    }
}
