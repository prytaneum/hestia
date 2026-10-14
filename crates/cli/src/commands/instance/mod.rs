//! `hestia instance …` — manage and launch client instances. Creation walks
//! through flavor/version pickers when arguments are omitted; files materialise
//! on first launch.
//!
//! The grammar is entry-first: catalogue verbs (`create`, `list`, `recent`,
//! `versions`, `flavors`) take no entry, while everything that acts on one instance reads
//! as `instance <name> <action>`. This module is the grammar and the dispatch;
//! each verb group lives beside it.

mod config;
mod create;
mod entry;
pub(crate) mod lifecycle;
pub(crate) mod recent;
pub(crate) mod servers;
mod transfer;
mod update;
mod world;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use client::proto::content::ContentKind;
use client::proto::instance::QuickPlay;
use client::Client;

use crate::commands::content::{self, ContentCmd, EntryKind};
use crate::commands::mc;
use crate::commands::modpack::EntryModpackCmd;
use crate::ui::Spinner;

pub use lifecycle::{launch, Launch};

/// What a launch joins on start — the grammar's own translation of three flags
/// into the one thing the wire carries. clap refuses more than one of them, so
/// together they can only spell one target.
#[derive(clap::Args)]
pub struct Join {
    #[arg(long, help = "Open a save world on start, by folder (Minecraft 1.20+)")]
    world: Option<String>,
    #[arg(
        long,
        conflicts_with = "world",
        help = "Join a server on start, by address (Minecraft 1.20+)"
    )]
    server: Option<String>,
    #[arg(
        long,
        conflicts_with_all = ["world", "server"],
        help = "Join a Realm on start, by its numeric id (Minecraft 1.20+)"
    )]
    realm: Option<String>,
}

impl Join {
    pub fn target(self) -> Option<QuickPlay> {
        self.world
            .map(QuickPlay::World)
            .or(self.server.map(QuickPlay::Server))
            .or(self.realm.map(QuickPlay::Realm))
    }
}

#[derive(Subcommand)]
#[command(
    after_help = "Act on one instance with `hestia instance <name> <action>`, e.g.\n  \
        hestia instance modded launch\n  \
        hestia instance modded mod add sodium\n  \
        hestia instance modded config set memory 4G\nRun `hestia instance <name> --help` for every action."
)]
pub enum InstanceCmd {
    /// Create an instance (prompts for anything omitted; files download at first launch)
    Create {
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
        #[arg(long, help = "Set -Xms and -Xmx together (e.g. 4G, 2048M)")]
        memory: Option<String>,
        /// Build the instance from a modpack — a slug, a URL, or a .mrpack
        /// path. The pack names the flavor and version, so both are ignored.
        #[arg(
            long,
            conflicts_with_all = ["flavor", "version", "loader"],
            help = "Build from a modpack (slug, URL, or .mrpack path)"
        )]
        modpack: Option<String>,
    },
    /// Import an instance from an archive (hestia, .mrpack, CurseForge, or Prism/MultiMC)
    Import {
        /// Path to the archive; its format is detected from what is inside it
        path: PathBuf,
        #[arg(
            short,
            long,
            help = "Name the new instance (defaults to the archive's)"
        )]
        name: Option<String>,
    },
    /// Managed instances and their state
    #[command(visible_alias = "ls")]
    List,
    /// The worlds, servers and Realms played lately, across every instance
    Recent,
    /// Game versions a flavor offers (prompts for the flavor when omitted)
    Versions {
        /// Flavor id (e.g. vanilla, fabric)
        flavor: Option<String>,
        #[arg(long, help = "Include snapshots and old versions")]
        all: bool,
    },
    /// The available flavors
    Flavors,
    /// Act on one instance: `hestia instance <name> <launch|stop|mod|…>`
    #[command(external_subcommand)]
    Entry(Vec<String>),
}

/// The per-instance grammar reached through `hestia instance <name> …`. The
/// name is captured once here so no action has to repeat it.
#[derive(Parser)]
#[command(no_binary_name = true, name = "hestia instance")]
struct InstanceEntry {
    /// Instance name or id
    name: String,
    #[command(subcommand)]
    action: InstanceAction,
}

#[derive(Subcommand)]
enum InstanceAction {
    /// Prepare (java, client jar, libraries, assets) and launch the instance
    Launch {
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
        #[command(flatten)]
        join: Join,
    },
    /// Kill the instance's sessions (all, or one with --session)
    Stop {
        #[arg(
            long,
            help = "Target one session by its handle (see `info`); default all"
        )]
        session: Option<String>,
    },
    /// Stop the running instance and launch it again
    Restart {
        #[arg(long, help = "Account name or uuid (default: the switched-to account)")]
        account: Option<String>,
        #[arg(short, long, help = "Return immediately instead of following the logs")]
        detach: bool,
        #[arg(
            long,
            help = "Restart one session by its handle (see `info`); default all"
        )]
        session: Option<String>,
    },
    /// The instance's record and running sessions
    Info,
    /// The instance's save worlds (where datapacks install)
    Worlds,
    /// Play one of the instance's save worlds directly
    World {
        #[command(subcommand)]
        cmd: world::WorldCmd,
    },
    /// The instance's multiplayer list, with each server's status
    Servers,
    /// Join or manage the servers in the instance's multiplayer list
    Server {
        #[command(subcommand)]
        cmd: servers::ServerCmd,
    },
    /// Watch a running session's CPU and memory on a fullscreen graph
    Monitor {
        #[arg(
            long,
            help = "Target one session by its handle (see `info`); default newest"
        )]
        session: Option<String>,
    },
    /// Captured instance output
    Logs {
        #[arg(short = 'n', long = "tail", help = "Only the last N lines")]
        tail: Option<usize>,
        #[arg(short, long, help = "Keep streaming new output until Ctrl-C")]
        follow: bool,
        #[arg(
            long,
            help = "Target one session by its handle (see `info`); default newest"
        )]
        session: Option<String>,
    },
    /// Get, set, or list settings (memory, jvm-args)
    Config {
        #[command(subcommand)]
        cmd: mc::ConfigCmd,
    },
    /// Install, list, remove, or update the instance's mods
    Mod {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// Install, list, remove, or update the instance's resource packs
    Resourcepack {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// Install, list, remove, or update the instance's shaders
    Shader {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// Install, list, remove, or update the instance's datapacks (into a world)
    Datapack {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// The modpack this instance runs: status, update, remove
    Modpack {
        #[command(subcommand)]
        cmd: EntryModpackCmd,
    },
    /// Move the instance to another version (prompts for anything omitted)
    Update {
        /// Target game version (prompts when omitted)
        version: Option<String>,
        #[arg(
            short,
            long,
            help = "Pin a loader version (modloaders only; default latest)"
        )]
        loader: Option<String>,
        #[arg(
            long,
            help = "Allow moving to an older version (saves do not downgrade)"
        )]
        downgrade: bool,
        #[arg(
            long,
            help = "Leave the instance's modpack on the version it has (it moves with the game version by default)"
        )]
        keep_pack: bool,
    },
    /// Write the instance out as one archive (must be stopped)
    Export {
        #[arg(
            short,
            long,
            help = "Archive format: hestia (hestia's own) or mrpack (for other launchers)"
        )]
        format: Option<String>,
        #[arg(
            short,
            long,
            help = "Where to write it — a file or a directory (default: the data home's exports/)"
        )]
        output: Option<PathBuf>,
        #[arg(
            long,
            help = "Leave an entry-relative path out (e.g. data/saves); repeatable"
        )]
        exclude: Vec<String>,
    },
    /// Rename the instance (moves its directory; must be stopped)
    Rename {
        /// The new display name
        new_name: String,
    },
    /// What this instance shares with the others (see `hestia sync status`)
    Sync {
        #[command(subcommand)]
        cmd: SyncAction,
    },
    /// Delete the instance (its saves and all)
    #[command(visible_alias = "rm")]
    Remove,
}

#[derive(Subcommand)]
pub enum SyncAction {
    /// Where this instance stands on each shared thing
    Status,
    /// Take part in one shared thing again
    On {
        /// What to share
        unit: crate::commands::sync::Unit,
    },
    /// Keep this instance's own copy of one shared thing
    Off {
        /// What to keep to itself
        unit: crate::commands::sync::Unit,
    },
    /// Keep one game setting to this instance (e.g. guiScale)
    Local {
        /// The `options.txt` key
        key: String,
    },
    /// Share one game setting with the other instances again
    Share {
        /// The `options.txt` key
        key: String,
    },
}

pub async fn run(cmd: InstanceCmd) -> Result<()> {
    match cmd {
        InstanceCmd::Entry(argv) => {
            let InstanceEntry { name, action } = match InstanceEntry::try_parse_from(argv) {
                Ok(parsed) => parsed,
                Err(err) => err.exit(),
            };
            let client = super::connect().await?;
            run_action(&client, name, action).await
        }
        catalogue => {
            let client = super::connect().await?;
            match catalogue {
                InstanceCmd::Create {
                    flavor,
                    version,
                    loader,
                    name,
                    memory,
                    modpack: Some(pack),
                } => {
                    let _ = (flavor, version, loader, memory);
                    crate::commands::modpack::install(
                        &client,
                        crate::commands::modpack::InstallArgs {
                            pack: Some(pack),
                            name,
                            ..Default::default()
                        },
                    )
                    .await
                }
                InstanceCmd::Create {
                    flavor,
                    version,
                    loader,
                    name,
                    memory,
                    modpack: None,
                } => create::run(&client, flavor, version, loader, name, memory).await,
                InstanceCmd::Import { path, name } => transfer::import(&client, path, name).await,
                InstanceCmd::List => entry::list(&client).await,
                InstanceCmd::Recent => recent::list(&client).await,
                InstanceCmd::Versions { flavor, all } => versions(&client, flavor, all).await,
                InstanceCmd::Flavors => flavors(&client).await,
                InstanceCmd::Entry(_) => unreachable!("handled above"),
            }
        }
    }
}

async fn run_action(client: &Client, name: String, action: InstanceAction) -> Result<()> {
    match action {
        InstanceAction::Launch {
            account,
            detach,
            new_session,
            offline,
            join,
        } => {
            launch(
                client,
                lifecycle::Launch {
                    reference: &name,
                    account: account.as_deref().unwrap_or_default(),
                    new_session,
                    detach,
                    quick_play: join.target(),
                    offline,
                },
            )
            .await
        }
        InstanceAction::Stop { session } => lifecycle::stop(client, &name, session).await,
        InstanceAction::Restart {
            account,
            detach,
            session,
        } => {
            lifecycle::restart(
                client,
                &name,
                session,
                account.as_deref().unwrap_or_default(),
                detach,
            )
            .await
        }
        InstanceAction::Info => {
            let info = client.instance().info(&name).await?;
            let sessions = entry::fetch(client, &name).await?.sessions;
            entry::show_detail(&info, &sessions)
        }
        InstanceAction::Worlds => world::list(client, &name).await,
        InstanceAction::World { cmd } => world::run(client, &name, cmd).await,
        InstanceAction::Servers => servers::list(client, &name).await,
        InstanceAction::Server { cmd } => servers::run(client, &name, cmd).await,
        InstanceAction::Monitor { session } => lifecycle::monitor(client, &name, session).await,
        InstanceAction::Logs {
            tail,
            follow,
            session,
        } => lifecycle::logs(client, &name, session, tail, follow).await,
        InstanceAction::Config { cmd } => config::run(client, &name, cmd).await,
        InstanceAction::Mod { cmd } => {
            content::run_entry(client, EntryKind::Instance, ContentKind::Mod, &name, cmd).await
        }
        InstanceAction::Resourcepack { cmd } => {
            content::run_entry(
                client,
                EntryKind::Instance,
                ContentKind::ResourcePack,
                &name,
                cmd,
            )
            .await
        }
        InstanceAction::Shader { cmd } => {
            content::run_entry(client, EntryKind::Instance, ContentKind::Shader, &name, cmd).await
        }
        InstanceAction::Datapack { cmd } => {
            content::run_entry(
                client,
                EntryKind::Instance,
                ContentKind::DataPack,
                &name,
                cmd,
            )
            .await
        }
        InstanceAction::Modpack { cmd } => {
            crate::commands::modpack::run_entry(EntryKind::Instance, name, cmd).await
        }
        InstanceAction::Update {
            version,
            loader,
            downgrade,
            keep_pack,
        } => update::run(client, name, version, loader, downgrade, keep_pack).await,
        InstanceAction::Export {
            format,
            output,
            exclude,
        } => transfer::export(client, name, format, output, exclude).await,
        InstanceAction::Rename { new_name } => lifecycle::rename(client, &name, &new_name).await,
        InstanceAction::Sync { cmd } => sync_action(client, &name, cmd).await,
        InstanceAction::Remove => lifecycle::remove(client, &name).await,
    }
}

async fn sync_action(client: &Client, name: &str, cmd: SyncAction) -> Result<()> {
    use crate::commands::sync::{state_label, unit_name};

    let info = entry::pick_instance(client.instance().list().await?, Some(name.to_string()))?;
    let status = match cmd {
        SyncAction::Status => {
            let all = client.sync().status().await?;
            all.into_iter()
                .find(|entry| entry.id == info.id)
                .unwrap_or_default()
        }
        SyncAction::On { unit } => {
            client
                .sync()
                .set_instance_unit(&info.id, unit.proto(), Some(true))
                .await?
        }
        SyncAction::Off { unit } => {
            client
                .sync()
                .set_instance_unit(&info.id, unit.proto(), Some(false))
                .await?
        }
        SyncAction::Local { key } => pin_key(client, &info.id, key, false).await?,
        SyncAction::Share { key } => pin_key(client, &info.id, key, true).await?,
    };

    let rows: Vec<Vec<String>> = status
        .units
        .iter()
        .map(|unit| {
            vec![
                unit_name(unit.unit).to_string(),
                state_label(unit.state).to_string(),
            ]
        })
        .collect();
    crate::ui::show(crate::ui::View::table(
        format!("{} — shared settings", status.name),
        ["WHAT", "STATE"],
        rows,
    ))?;
    if !status.unsynced.is_empty() {
        crate::ui::show(crate::ui::View::note(format!(
            "settings it keeps to itself: {}",
            status
                .unsynced
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )))?;
    }
    Ok(())
}

async fn pin_key(
    client: &Client,
    id: &str,
    key: String,
    shared: bool,
) -> Result<client::proto::sync::InstanceSyncStatus> {
    let mut unsynced = client
        .sync()
        .status()
        .await?
        .into_iter()
        .find(|entry| entry.id == id)
        .unwrap_or_default()
        .unsynced;
    match shared {
        true => unsynced.remove(&key),
        false => unsynced.insert(key),
    };
    Ok(client.sync().set_instance_unsynced(id, unsynced).await?)
}

async fn versions(client: &Client, flavor: Option<String>, all: bool) -> Result<()> {
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.instance().flavors().await?
    };
    let flavor = mc::pick_flavor(flavors, flavor)?;
    let versions = {
        let _spinner = Spinner::start("fetching versions");
        client.instance().versions(&flavor).await?
    };
    mc::show_versions(&flavor, versions, all)
}

async fn flavors(client: &Client) -> Result<()> {
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.instance().flavors().await?
    };
    mc::show_flavors(&flavors)
}
