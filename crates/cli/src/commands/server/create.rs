//! `hestia server create` — the wizard for whatever is missing, or the fully
//! specified flag path, and the create-time settings.

use anyhow::{bail, Context, Result};
use client::proto::error::EntryKind;
use client::proto::minecraft::ConfigEntry;
use client::proto::server::ServerCreateParams;
use client::Client;

use super::entry;
use crate::commands::mc;
use crate::commands::wizard::{self, Field, WizardOutcome, WizardSeed};
use crate::ui::{self, ProvisionReporter, View};

const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

/// Everything `server create` accepts; folded into the create params and the
/// create-time settings list.
#[derive(clap::Args)]
pub struct CreateArgs {
    /// Flavor id (e.g. vanilla, fabric)
    flavor: Option<String>,
    /// Game version (e.g. 1.21.1)
    version: Option<String>,
    #[arg(
        short,
        long,
        help = "Pin a loader version (modloaders only; default latest)"
    )]
    loader: Option<String>,
    #[arg(short, long, help = "Display name (defaults to <flavor>-<version>)")]
    name: Option<String>,
    #[arg(
        long,
        help = "Accept the Minecraft EULA (https://aka.ms/MinecraftEULA)"
    )]
    eula: bool,
    #[arg(short, long, help = "Pin the game port (default: lowest free)")]
    port: Option<u16>,
    #[arg(long, help = "Set -Xms and -Xmx together (e.g. 4G, 2048M)")]
    memory: Option<String>,
    #[arg(long, help = "The message of the day shown in the server list")]
    motd: Option<String>,
    #[arg(long, help = "Maximum number of players")]
    max_players: Option<u32>,
    #[arg(long, value_parser = ["peaceful", "easy", "normal", "hard"])]
    difficulty: Option<String>,
    #[arg(long, value_parser = ["survival", "creative", "adventure", "spectator"])]
    gamemode: Option<String>,
    #[arg(long, help = "World seed (level-seed)")]
    seed: Option<String>,
    #[arg(
        long = "prop",
        value_name = "KEY=VALUE",
        help = "Set any other server.properties entry (repeatable; wins over the dedicated flags)"
    )]
    prop: Vec<String>,
    /// Build the server from a modpack — a slug, a URL, or a .mrpack path. The
    /// pack names the flavor and version, so both are ignored.
    #[arg(
        long,
        conflicts_with_all = ["flavor", "version", "loader"],
        help = "Build from a modpack (slug, URL, or .mrpack path)"
    )]
    modpack: Option<String>,
}

impl CreateArgs {
    fn is_fully_specified(&self) -> bool {
        self.flavor.is_some() && self.version.is_some() && self.name.is_some() && self.eula
    }
}

pub(super) async fn run(client: &Client, args: CreateArgs) -> Result<()> {
    if let Some(pack) = args.modpack.clone() {
        return crate::commands::modpack::install(
            client,
            crate::commands::modpack::InstallArgs {
                pack: Some(pack),
                server: true,
                name: args.name,
                eula: args.eula,
                port: args.port,
                ..Default::default()
            },
        )
        .await;
    }
    if ui::is_interactive() && !args.is_fully_specified() {
        return run_wizard(client, args).await;
    }
    if !args.eula {
        bail!("pass --eula to accept the Minecraft EULA ({EULA_URL})");
    }
    let flavors = mc::fetch_flavors(client, EntryKind::Server).await?;
    let flavor = mc::pick_flavor(flavors, args.flavor)?;
    let versions = mc::fetch_versions(client, EntryKind::Server, &flavor).await?;
    let version = mc::pick_version(versions, args.version)?;
    let name = args.name.unwrap_or_else(|| format!("{flavor}-{version}"));
    let config = build_config(
        args.memory,
        [
            ("motd", args.motd),
            ("max-players", args.max_players.map(|n| n.to_string())),
            ("difficulty", args.difficulty),
            ("gamemode", args.gamemode),
            ("level-seed", args.seed),
        ],
        args.prop,
    )?;

    let params = ServerCreateParams {
        name,
        flavor,
        version,
        loader_version: args.loader,
        eula: true,
        port: args.port,
        config,
        id: String::new(),
    };
    let done = ProvisionReporter::run(None, async |on_progress| {
        client.server().create(params, on_progress).await
    })
    .await?;
    ui::show(View::line(format!("server '{}' created", done.server.name)))?;
    entry::show_status(&done.server, None)?;
    ui::show_warnings(&done.warnings)
}

/// The interactive path: the fullscreen step wizard, prefilled from whatever
/// flags were given.
async fn run_wizard(client: &Client, args: CreateArgs) -> Result<()> {
    let flavors = mc::fetch_flavors(client, EntryKind::Server).await?;
    if let Some(flavor) = &args.flavor {
        mc::ensure_flavor(&flavors, flavor)?;
    }
    let extra = build_config(None, [], args.prop)?;
    let seed = WizardSeed {
        kind: EntryKind::Server,
        flavors,
        flavor: args.flavor,
        version: args.version,
        name: args.name,
        loader: args.loader,
        eula: args.eula,
        fields: vec![
            Field::text("memory", "memory", "JVM default", args.memory),
            Field::number(
                "port",
                "port",
                "auto — lowest free from 25565",
                args.port.map(|p| p.to_string()),
            ),
            Field::text("motd", "motd", "server default", args.motd),
            Field::number(
                "max-players",
                "max players",
                "server default",
                args.max_players.map(|n| n.to_string()),
            ),
            Field::choice(
                "difficulty",
                "difficulty",
                "server default",
                &["peaceful", "easy", "normal", "hard"],
                args.difficulty,
            ),
            Field::choice(
                "gamemode",
                "gamemode",
                "server default",
                &["survival", "creative", "adventure", "spectator"],
                args.gamemode,
            ),
            Field::text("level-seed", "seed", "random", args.seed),
        ],
        extra,
    };
    match wizard::run(client, seed).await? {
        None => ui::show(View::note("cancelled")),
        Some(WizardOutcome::Server(done)) => {
            ui::show(View::line(format!("server '{}' created", done.server.name)))?;
            entry::show_status(&done.server, None)?;
            ui::show_warnings(&done.warnings)
        }
        Some(WizardOutcome::Instance(_)) => unreachable!("server wizard created an instance"),
    }
}

/// Fold the create-time settings into one entries list: `--memory`, then the
/// dedicated property flags, then `--prop KEY=VALUE` (split on the first `=`;
/// a missing `=` is an error). Entries apply in order, so a `--prop` naming
/// the same key as a dedicated flag wins.
fn build_config(
    memory: Option<String>,
    flags: impl IntoIterator<Item = (&'static str, Option<String>)>,
    prop: Vec<String>,
) -> Result<Vec<ConfigEntry>> {
    let mut config = Vec::new();
    if let Some(memory) = memory {
        config.push(ConfigEntry {
            key: "memory".into(),
            value: memory,
        });
    }
    for (key, value) in flags {
        if let Some(value) = value {
            config.push(ConfigEntry {
                key: key.into(),
                value,
            });
        }
    }
    for entry in prop {
        let (key, value) = entry
            .split_once('=')
            .with_context(|| format!("--prop '{entry}' must be KEY=VALUE"))?;
        config.push(ConfigEntry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(config)
}
