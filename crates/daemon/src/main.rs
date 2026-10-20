//! hestiad — the Hestia daemon.
//!
//!   hestiad [serve]   run the daemon: bind the endpoint, serve until signalled
//!   hestiad ping      connect to a running daemon, report its identity
//!   hestiad stop      ask a running daemon to stop; supervised processes keep running
//!
//! main() only bootstraps: CLI parsing, logging init, and dispatch. The serve
//! loop lives in server.rs; every channel lives in services/.

mod autostart;
mod runtime;
mod server;
mod services;
mod tray;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use common::LogLevel;

#[derive(Parser)]
#[command(name = "hestiad", version = common::app::VERSION_LABEL, about = "hestiad — the Hestia daemon")]
struct Cli {
    #[arg(
        short,
        long,
        action = clap::ArgAction::Count,
        help = "Increase log verbosity (-v debug, -vv trace)"
    )]
    verbose: u8,
    #[arg(
        short,
        long,
        help = "Warnings and errors only",
        conflicts_with = "verbose"
    )]
    quiet: bool,
    /// Start without third-party plugins, for this run only
    #[arg(
        long,
        global = true,
        env = "HESTIA_SAFE_MODE",
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    safe_mode: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon (default)
    Serve {
        /// Release the inherited console (the Windows login task uses this so
        /// no console window lingers). No effect on other platforms.
        #[arg(long, hide = true)]
        detach_console: bool,
    },
    /// Check that a running daemon is reachable
    Ping,
    /// Stop a running daemon; supervised processes keep running
    Stop,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let detach_console = matches!(
        cli.command,
        Some(Command::Serve {
            detach_console: true
        })
    );
    let level = if cli.quiet {
        LogLevel::Warn
    } else {
        match cli.verbose {
            0 => LogLevel::default(),
            1 => LogLevel::Debug,
            _ => LogLevel::Trace,
        }
    };

    let rt = tokio::runtime::Runtime::new().expect("build tokio runtime");

    let code = match cli.command {
        Some(Command::Ping) => {
            // ping is a one-shot foreground tool — stderr only.
            let _guard = common::init_logging(level, None);
            rt.block_on(run_ping())
        }
        Some(Command::Stop) => {
            let _guard = common::init_logging(level, None);
            rt.block_on(run_stop())
        }
        _ => {
            #[cfg(windows)]
            if detach_console {
                // SAFETY: FreeConsole has no preconditions; it releases the console
                // the login task attached so no window stays open.
                unsafe { windows_sys::Win32::System::Console::FreeConsole() };
            }
            #[cfg(not(windows))]
            let _ = detach_console;
            let file = common::FileLog::for_binary("hestiad", None, level).with_firehose();
            let log_path = file.active_path();
            let _guard = common::init_logging(level, Some(file));
            rt.block_on(server::run_daemon(log_path, cli.safe_mode))
        }
    };
    ExitCode::from(code as u8)
}

async fn run_stop() -> i32 {
    match client::Client::connect().await {
        Ok(client) => match client.daemon().stop(false).await {
            Ok(_) => {
                println!("hestiad stopping");
                0
            }
            Err(e) => {
                eprintln!("hestiad stop: {e}");
                1
            }
        },
        // No reachable daemon means there is nothing to stop — succeed so
        // scripted callers (e.g. the Windows installer) can treat this as
        // idempotent.
        Err(_) => {
            println!("hestiad is not running");
            0
        }
    }
}

async fn run_ping() -> i32 {
    match client::Client::connect().await {
        Ok(client) => match client.app().info().await {
            Ok(info) => {
                println!("{} {} — alive", info.name, info.version);
                0
            }
            Err(e) => {
                eprintln!("hestiad ping: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("hestiad ping: {e}");
            1
        }
    }
}
