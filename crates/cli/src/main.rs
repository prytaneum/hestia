//! hestia — the Hestia command-line interface. A thin client over the daemon.

mod commands;
mod exit;
mod ui;

use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use common::LogLevel;

use crate::exit::ExitStatus;

#[derive(Parser)]
#[command(name = "hestia", version = common::app::VERSION_LABEL, about = "Hestia command-line interface")]
struct Cli {
    #[arg(
        short,
        long,
        global = true,
        action = clap::ArgAction::Count,
        help = "Increase log verbosity (-v debug, -vv trace)"
    )]
    verbose: u8,
    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "verbose",
        help = "Only show errors on the console"
    )]
    quiet: bool,
    #[arg(
        long,
        global = true,
        help = "Override Hestia's data directory (else $HESTIA_HOME, else the platform default)"
    )]
    home: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Launch an instance (picks interactively when omitted)
    Play(commands::play::Play),
    /// Minecraft accounts (Microsoft sign-in, switching)
    #[command(visible_alias = "auth")]
    Account {
        #[command(subcommand)]
        cmd: commands::account::AccountCmd,
    },
    /// Java runtimes (Eclipse Temurin via the Adoptium API)
    Java {
        #[command(subcommand)]
        cmd: commands::java::JavaCmd,
    },
    /// Minecraft servers (create, start/stop, logs)
    Server {
        #[command(subcommand)]
        cmd: commands::server::ServerCmd,
    },
    /// Minecraft instances (create, launch)
    Instance {
        #[command(subcommand)]
        cmd: commands::instance::InstanceCmd,
    },
    /// Start a server or launch an instance by name
    Start {
        /// Server or instance name or id
        target: String,
        #[arg(
            long,
            help = "Account name or uuid for an instance (default: the switched-to account)"
        )]
        account: Option<String>,
        #[arg(short, long, help = "Return immediately instead of attaching")]
        detach: bool,
        #[arg(
            long,
            help = "Play without signing in: singleplayer only, on the last known account"
        )]
        offline: bool,
    },
    /// Stop a running server or instance by name
    Stop {
        /// Server or instance name or id
        target: String,
        #[arg(long, help = "Target one instance session by its handle; default all")]
        session: Option<String>,
    },
    /// Restart a server or instance by name
    Restart {
        /// Server or instance name or id
        target: String,
        #[arg(
            long,
            help = "Account name or uuid for an instance (default: the switched-to account)"
        )]
        account: Option<String>,
        #[arg(short, long, help = "Return immediately instead of attaching")]
        detach: bool,
        #[arg(long, help = "Restart one instance session by its handle; default all")]
        session: Option<String>,
    },
    /// Tail a server or instance's captured output by name
    Logs {
        /// Server or instance name or id
        target: String,
        #[arg(short = 'n', long = "tail", help = "Only the last N lines")]
        tail: Option<usize>,
        #[arg(short, long, help = "Keep streaming new output until Ctrl-C")]
        follow: bool,
        #[arg(
            long,
            help = "Target one instance session by its handle; default newest"
        )]
        session: Option<String>,
    },
    /// Rename a server or instance by name (must be stopped)
    Rename {
        /// Server or instance name or id
        target: String,
        /// The new display name
        new_name: String,
    },
    /// Browse mods on a content source
    Mod {
        #[command(subcommand)]
        cmd: commands::content::BrowseCmd,
    },
    /// Browse modpacks, and install one into a new or existing entry
    Modpack {
        #[command(subcommand)]
        cmd: commands::modpack::ModpackCmd,
    },
    /// Browse resource packs on a content source
    Resourcepack {
        #[command(subcommand)]
        cmd: commands::content::BrowseCmd,
    },
    /// Browse shaders on a content source
    Shader {
        #[command(subcommand)]
        cmd: commands::content::BrowseCmd,
    },
    /// Browse data packs on a content source
    Datapack {
        #[command(subcommand)]
        cmd: commands::content::BrowseCmd,
    },
    /// Browse server plugins on a content source
    Plugin {
        #[command(subcommand)]
        cmd: commands::content::BrowseCmd,
    },
    /// Search mods on a content source (alias for `mod search`)
    Search {
        /// Search terms
        query: Option<String>,
        #[arg(short, long, help = "Filter by loader (e.g. fabric)")]
        loader: Option<String>,
        #[arg(short = 'g', long = "game-version", help = "Filter by game version")]
        game_version: Option<String>,
        #[arg(
            short = 'S',
            long,
            help = "Content source (default: the first configured one)"
        )]
        source: Option<String>,
    },
    /// The content sources that can serve
    Sources,
    /// Download cache
    Cache {
        #[command(subcommand)]
        cmd: commands::cache::CacheCmd,
    },
    /// Configuration (typed settings, stored as JSON)
    Config {
        #[command(subcommand)]
        cmd: commands::config::ConfigCmd,
    },
    /// Screenshots across your instances
    Screenshot {
        #[command(subcommand)]
        cmd: commands::screenshot::ScreenshotCmd,
    },
    /// Settings/configs shared across instances
    Sync {
        #[command(subcommand)]
        cmd: commands::sync::SyncCmd,
    },
    /// Supervised processes, across every server and instance
    Process {
        #[command(subcommand)]
        cmd: commands::process::ProcessCmd,
    },
    /// Daemon lifecycle
    Daemon {
        #[command(subcommand)]
        cmd: commands::daemon::DaemonCmd,
    },
    /// Update Hestia itself from the release feed
    Update {
        #[arg(short = 'y', long, help = "Apply without the confirmation prompt")]
        yes: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Diagnostics go to an appended logs/hestia.log so they never garble command
    // output; the console only sees warnings and errors unless -v/-vv raise it.
    let (console_level, file_level) = if cli.quiet {
        (LogLevel::Error, LogLevel::Warn)
    } else {
        match cli.verbose {
            0 => (LogLevel::Warn, LogLevel::default()),
            1 => (LogLevel::Debug, LogLevel::Debug),
            _ => (LogLevel::Trace, LogLevel::Trace),
        }
    };
    let home = cli.home.as_deref().filter(|h| !h.is_empty()).map(Path::new);
    let file = common::FileLog::for_binary("hestia", home, file_level);
    let _guard = common::init_logging(console_level, Some(file));

    // The data directory is daemon-global, so --home is exported as
    // $HESTIA_HOME and only takes effect when `daemon start` spawns the daemon;
    // a daemon already running keeps its own directory.
    if let Some(home) = &cli.home {
        if !home.is_empty() {
            tracing::debug!(home, "exporting HESTIA_HOME for a spawned daemon");
            std::env::set_var("HESTIA_HOME", home);
        }
    }

    let Some(command) = cli.command else {
        // No subcommand given: show usage.
        let _ = <Cli as clap::CommandFactory>::command().print_help();
        let _ = std::io::Write::write_all(&mut std::io::stdout(), b"\n");
        return ExitCode::SUCCESS;
    };

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let result = rt.block_on(dispatch(command));

    match result {
        Ok(status) => status.code(),
        Err(e) if is_broken_pipe(&e) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|e| e.downcast_ref::<std::io::Error>())
        .any(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
}

/// Run the command and report how the shell should read it. Only the state
/// queries answer anything but [`ExitStatus::Active`] — see `exit.rs` for the
/// contract.
async fn dispatch(command: Command) -> anyhow::Result<ExitStatus> {
    match command {
        Command::Daemon { cmd } => return commands::daemon::run(cmd).await,
        Command::Server { cmd } => return commands::server::run(cmd).await,
        Command::Process { cmd } => return commands::process::run(cmd).await,
        Command::Play(play) => commands::play::run(play).await,
        Command::Account { cmd } => commands::account::run(cmd).await,
        Command::Java { cmd } => commands::java::run(cmd).await,
        Command::Instance { cmd } => commands::instance::run(cmd).await,
        Command::Start {
            target,
            account,
            detach,
            offline,
        } => commands::lifecycle::start(target, account, detach, offline).await,
        Command::Stop { target, session } => commands::lifecycle::stop(target, session).await,
        Command::Restart {
            target,
            account,
            detach,
            session,
        } => commands::lifecycle::restart(target, session, account, detach).await,
        Command::Logs {
            target,
            tail,
            follow,
            session,
        } => commands::lifecycle::logs(target, session, tail, follow).await,
        Command::Rename { target, new_name } => commands::lifecycle::rename(target, new_name).await,
        Command::Mod { cmd } => {
            commands::content::run_browse(client::proto::content::ContentKind::Mod, cmd).await
        }
        Command::Modpack { cmd } => commands::modpack::run(cmd).await,
        Command::Resourcepack { cmd } => {
            commands::content::run_browse(client::proto::content::ContentKind::ResourcePack, cmd)
                .await
        }
        Command::Shader { cmd } => {
            commands::content::run_browse(client::proto::content::ContentKind::Shader, cmd).await
        }
        Command::Datapack { cmd } => {
            commands::content::run_browse(client::proto::content::ContentKind::DataPack, cmd).await
        }
        Command::Plugin { cmd } => {
            commands::content::run_browse(client::proto::content::ContentKind::Plugin, cmd).await
        }
        Command::Search {
            query,
            loader,
            game_version,
            source,
        } => {
            commands::content::run_browse(
                client::proto::content::ContentKind::Mod,
                commands::content::BrowseCmd::Search {
                    query,
                    loader,
                    game_version,
                    category: Vec::new(),
                    sort: commands::content::SortArg::Relevance,
                    source,
                    limit: 20,
                    offset: 0,
                },
            )
            .await
        }
        Command::Sources => commands::content::run_sources().await,
        Command::Cache { cmd } => commands::cache::run(cmd).await,
        Command::Config { cmd } => commands::config::run(cmd).await,
        Command::Screenshot { cmd } => commands::screenshot::run(cmd).await,
        Command::Sync { cmd } => commands::sync::run(cmd).await,
        Command::Update { yes } => commands::update::run(yes).await,
    }
    .map(|()| ExitStatus::Active)
}
