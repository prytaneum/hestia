//! `hestia server …` — create fully provisioned servers and drive them through
//! the daemon. Creation walks through flavor/version pickers (and the EULA
//! confirm) when arguments are omitted.
//!
//! The grammar is entry-first: catalogue verbs (`create`, `list`, `versions`,
//! `flavors`) take no entry, while everything that acts on one server reads as
//! `server <name> <action>` so the name always sits in the same slot. This
//! module is the grammar and the dispatch; each verb group lives beside it.

mod backup;
mod config;
pub(crate) mod console;
mod create;
mod entry;
mod files;
pub(crate) mod lifecycle;
mod players;
mod tasks;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use client::proto::content::ContentKind;
use client::proto::error::EntryKind;
use client::Client;

use crate::commands::content::{self, ContentCmd};
use crate::commands::mc;
use crate::commands::modpack::EntryModpackCmd;
use crate::exit::ExitStatus;
use crate::ui::Spinner;

pub use backup::BackupCmd;
pub use create::CreateArgs;

#[derive(Subcommand)]
#[command(
    after_help = "Act on one server with `hestia server <name> <action>`, e.g.\n  \
        hestia server smp start\n  \
        hestia server smp config set memory 4G\n  \
        hestia server smp backup create\nRun `hestia server <name> --help` for every action."
)]
// `Create` flattens the sizeable `CreateArgs`, which clap cannot flatten behind
// a `Box`, so the variant is unavoidably larger than the catalogue ones.
#[allow(clippy::large_enum_variant)]
pub enum ServerCmd {
    /// Create a fully provisioned server (prompts for anything omitted)
    Create {
        #[command(flatten)]
        args: CreateArgs,
    },
    /// Managed servers and their state
    #[command(visible_alias = "ls")]
    List,
    /// Game versions a flavor offers (prompts for the flavor when omitted)
    Versions {
        /// Flavor id (e.g. vanilla, fabric)
        flavor: Option<String>,
        #[arg(long, help = "Include snapshots and old versions")]
        all: bool,
    },
    /// The available flavors
    Flavors,
    /// Loader builds a flavor offers for a version (modloaders only)
    Loaders {
        /// Flavor id (e.g. fabric); prompts when omitted
        flavor: Option<String>,
        /// Game version the loaders must support; prompts when omitted
        version: Option<String>,
    },
    /// Act on one server: `hestia server <name> <start|stop|config|backup|…>`
    #[command(external_subcommand)]
    Entry(Vec<String>),
}

/// The per-server grammar reached through `hestia server <name> …`. The name is
/// captured once here so no action has to repeat it.
#[derive(Parser)]
#[command(no_binary_name = true, name = "hestia server")]
struct ServerEntry {
    /// Server name or id
    name: String,
    #[command(subcommand)]
    action: ServerAction,
}

#[derive(Subcommand)]
enum ServerAction {
    /// Start the server and attach its console
    Start {
        #[arg(short, long, help = "Return immediately instead of attaching")]
        detach: bool,
    },
    /// Stop the running server
    Stop,
    /// Stop the running server, start it again, and attach its console
    Restart {
        #[arg(short, long, help = "Return immediately instead of attaching")]
        detach: bool,
    },
    /// The server's record merged with its live process state (and, when
    /// running, a live server ping)
    Status,
    /// The server's descriptor, on-disk location, and footprint
    Info,
    /// Watch live CPU and memory usage on a fullscreen graph
    Monitor,
    /// Captured server output
    Logs {
        #[arg(short = 'n', long = "tail", help = "Only the last N lines")]
        tail: Option<usize>,
        #[arg(short, long, help = "Keep streaming new output until Ctrl-C")]
        follow: bool,
    },
    /// Attach an interactive console: live logs, type to send commands
    #[command(visible_alias = "console")]
    Attach,
    /// Send one console command and print the reply
    #[command(visible_alias = "cmd")]
    Command {
        /// The command, as it would be typed in the console
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Get, set, or list settings (memory, jvm-args, server.properties)
    Config {
        #[command(subcommand)]
        cmd: mc::ConfigCmd,
    },
    /// Archive, restore, or manage the server's backups
    Backup {
        #[command(subcommand)]
        cmd: BackupCmd,
    },
    /// Browse and edit the files in the server's game directory
    Files {
        #[command(subcommand)]
        cmd: files::FilesCmd,
    },
    /// See who is online and manage the whitelist, operators and bans
    Players {
        #[command(subcommand)]
        cmd: players::PlayersCmd,
    },
    /// Restart the server or send it a command on a schedule
    Task {
        #[command(subcommand)]
        cmd: tasks::TaskCmd,
    },
    /// Install, list, remove, or update the server's mods
    Mod {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// Install, list, remove, or update the server's plugins
    Plugin {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// Install, list, remove, or update the server's datapacks
    Datapack {
        #[command(subcommand)]
        cmd: ContentCmd,
    },
    /// The modpack this server runs: status, update, remove
    Modpack {
        #[command(subcommand)]
        cmd: EntryModpackCmd,
    },
    /// Move the server to another version (prompts for anything omitted)
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
            help = "Allow moving to an older version (worlds do not downgrade)"
        )]
        downgrade: bool,
        #[arg(
            long,
            help = "Stop a running server for the update and start it again after"
        )]
        restart: bool,
        #[arg(
            long,
            help = "Leave the server's modpack on the version it has (it moves with the game version by default)"
        )]
        keep_pack: bool,
    },
    /// Rename the server (moves its directory; must be stopped)
    Rename {
        /// The new display name
        new_name: String,
    },
    /// Delete the server (its jar, world and all)
    #[command(visible_alias = "rm")]
    Remove,
}

pub async fn run(cmd: ServerCmd) -> Result<ExitStatus> {
    match cmd {
        ServerCmd::Entry(argv) => {
            let ServerEntry { name, action } = match ServerEntry::try_parse_from(argv) {
                Ok(parsed) => parsed,
                Err(err) => err.exit(),
            };
            let client = super::connect().await?;
            run_action(client, name, action).await
        }
        catalogue => {
            let client = super::connect().await?;
            match catalogue {
                ServerCmd::Create { args } => create::run(&client, args).await,
                ServerCmd::List => entry::list(&client).await,
                ServerCmd::Versions { flavor, all } => versions(&client, flavor, all).await,
                ServerCmd::Flavors => flavors(&client).await,
                ServerCmd::Loaders { flavor, version } => loaders(&client, flavor, version).await,
                ServerCmd::Entry(_) => unreachable!("handled above"),
            }
            .map(|()| ExitStatus::Active)
        }
    }
}

async fn run_action(client: Client, name: String, action: ServerAction) -> Result<ExitStatus> {
    // The state query answers through the exit code too: a stopped server is
    // reported, not an error (see `exit.rs`).
    if let ServerAction::Status = action {
        let info = client.server().status(&name).await?;
        let running = entry::running_process(&info).is_some();
        let ping = if running {
            client.server().ping(&name).await.ok()
        } else {
            None
        };
        entry::show_status(&info, ping.as_ref())?;
        return Ok(ExitStatus::running(running));
    }
    run_unit_action(client, name, action)
        .await
        .map(|()| ExitStatus::Active)
}

async fn run_unit_action(client: Client, name: String, action: ServerAction) -> Result<()> {
    match action {
        ServerAction::Start { detach } => console::start_attached(client, &name, detach).await,
        ServerAction::Stop => lifecycle::stop(&client, &name).await,
        ServerAction::Restart { detach } => console::restart_attached(client, &name, detach).await,
        ServerAction::Status => unreachable!("handled by run_action"),
        ServerAction::Info => {
            let info = client.server().info(&name).await?;
            entry::show_info(&info)
        }
        ServerAction::Monitor => lifecycle::monitor(&client, &name).await,
        ServerAction::Logs { tail, follow } => lifecycle::logs(&client, &name, tail, follow).await,
        ServerAction::Attach => console::attach(client, &name).await,
        ServerAction::Command { command } => {
            let reply = client.server().command(&name, &command.join(" ")).await?;
            console::show_reply(&reply)
        }
        ServerAction::Config { cmd } => config::run(&client, &name, cmd).await,
        ServerAction::Backup { cmd } => backup::run(&client, &name, cmd).await,
        ServerAction::Files { cmd } => files::run(&client, &name, cmd).await,
        ServerAction::Players { cmd } => players::run(&client, &name, cmd).await,
        ServerAction::Task { cmd } => tasks::run(&client, &name, cmd).await,
        ServerAction::Mod { cmd } => {
            content::run_entry(&client, EntryKind::Server, ContentKind::Mod, &name, cmd).await
        }
        ServerAction::Plugin { cmd } => {
            content::run_entry(&client, EntryKind::Server, ContentKind::Plugin, &name, cmd).await
        }
        ServerAction::Datapack { cmd } => {
            content::run_entry(
                &client,
                EntryKind::Server,
                ContentKind::DataPack,
                &name,
                cmd,
            )
            .await
        }
        ServerAction::Modpack { cmd } => {
            crate::commands::modpack::run_entry(EntryKind::Server, name, cmd).await
        }
        ServerAction::Update {
            version,
            loader,
            downgrade,
            restart,
            keep_pack,
        } => {
            update::run(
                &client, name, version, loader, downgrade, restart, keep_pack,
            )
            .await
        }
        ServerAction::Rename { new_name } => lifecycle::rename(&client, &name, &new_name).await,
        ServerAction::Remove => lifecycle::remove(&client, &name).await,
    }
}

async fn versions(client: &Client, flavor: Option<String>, all: bool) -> Result<()> {
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.server().flavors().await?
    };
    let flavor = mc::pick_flavor(flavors, flavor)?;
    let versions = {
        let _spinner = Spinner::start("fetching versions");
        client.server().versions(&flavor).await?
    };
    mc::show_versions(&flavor, versions, all)
}

async fn flavors(client: &Client) -> Result<()> {
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.server().flavors().await?
    };
    mc::show_flavors(&flavors)
}

async fn loaders(client: &Client, flavor: Option<String>, version: Option<String>) -> Result<()> {
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.server().flavors().await?
    };
    let flavor = mc::pick_flavor(flavors, flavor)?;
    let versions = {
        let _spinner = Spinner::start("fetching versions");
        client.server().versions(&flavor).await?
    };
    let version = mc::pick_version(versions, version)?;
    let loaders = {
        let _spinner = Spinner::start("fetching loaders");
        client.server().loaders(&flavor, &version).await?
    };
    if loaders.is_empty() {
        return crate::ui::show(crate::ui::View::note(format!(
            "{flavor} has no pickable loader version for {version} (the latest is used)"
        )));
    }
    crate::ui::show(crate::ui::View::table(
        format!("{flavor} loaders for {version}"),
        ["LOADER"],
        loaders.into_iter().map(|l| vec![l]).collect(),
    ))
}
