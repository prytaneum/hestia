//! Developer mode as the CLI sees it: whether the plugin authoring verbs are
//! shown and allowed. The daemon refuses developer calls on its own; this only
//! keeps the verbs out of a user's way.

use std::time::Duration;

use clap::{ArgMatches, Command};
use client::Client;
use proto::config::DEVELOPER_KEY;

pub const ENV_OVERRIDE: &str = "HESTIA_DEVELOPER";
pub const AUTHORING_VERBS: [&str; 6] = ["new", "pack", "dev", "check", "keygen", "publish"];
pub const OFF_MESSAGE: &str =
    "developer mode is off; enable it with: hestia config set developer true";

const DAEMON_TIMEOUT: Duration = Duration::from_millis(800);

/// Whether an invocation can show or run an authoring verb, so only those ask the daemon.
pub fn reaches_authoring(args: &[String]) -> bool {
    args.iter()
        .any(|arg| matches!(arg.as_str(), "plugin" | "help" | "-h" | "--help"))
}

pub fn env_enabled() -> bool {
    std::env::var(ENV_OVERRIDE).is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
}

/// The mode for this invocation: the environment override, else the daemon's
/// setting when the invocation needs it, and off when the daemon is down.
pub fn resolve(args: &[String]) -> bool {
    env_enabled() || (reaches_authoring(args) && daemon_setting())
}

fn daemon_setting() -> bool {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return false;
    };
    runtime.block_on(async {
        let asked = tokio::time::timeout(DAEMON_TIMEOUT, async {
            let client = Client::connect().await.ok()?;
            client.config().get(DEVELOPER_KEY).await.ok().flatten()
        })
        .await;
        matches!(asked, Ok(Some(serde_json::Value::Bool(true))))
    })
}

/// Hides the authoring verbs of `plugin`; a verb this build lacks is skipped.
pub fn hide_authoring(command: Command) -> Command {
    command.mut_subcommand("plugin", |plugin| {
        let present: Vec<&str> = AUTHORING_VERBS
            .into_iter()
            .filter(|verb| plugin.find_subcommand(verb).is_some())
            .collect();
        present.into_iter().fold(plugin, |plugin, verb| {
            plugin.mut_subcommand(verb, |verb| verb.hide(true))
        })
    })
}

/// Whether the parsed invocation is an authoring verb.
pub fn is_authoring(matches: &ArgMatches) -> bool {
    matches
        .subcommand_matches("plugin")
        .and_then(ArgMatches::subcommand_name)
        .is_some_and(|verb| AUTHORING_VERBS.contains(&verb))
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::Cli;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn plugin_help(command: Command) -> String {
        command
            .find_subcommand("plugin")
            .unwrap()
            .clone()
            .render_help()
            .to_string()
    }

    #[test]
    fn only_plugin_and_help_invocations_ask_the_daemon() {
        assert!(reaches_authoring(&args(&["hestia", "plugin", "new", "x"])));
        assert!(reaches_authoring(&args(&["hestia", "--help"])));
        assert!(reaches_authoring(&args(&["hestia", "help"])));
        assert!(!reaches_authoring(&args(&["hestia", "server", "list"])));
    }

    #[test]
    fn every_authoring_verb_exists_and_is_hidden_from_help() {
        let shown = plugin_help(Cli::command());
        let hidden = plugin_help(hide_authoring(Cli::command()));
        for verb in AUTHORING_VERBS {
            let line = format!("\n  {verb} ");
            assert!(shown.contains(&line), "{verb} is not a plugin verb");
            assert!(!hidden.contains(&line), "{verb} is still in help");
        }
        assert!(hidden.contains("\n  list "));
    }

    #[test]
    fn a_hidden_verb_still_parses_and_is_recognised_as_authoring() {
        let command = hide_authoring(Cli::command());
        let authoring = command
            .clone()
            .get_matches_from(["hestia", "plugin", "pack"]);
        let managing = command.get_matches_from(["hestia", "plugin", "list"]);
        assert!(is_authoring(&authoring));
        assert!(!is_authoring(&managing));
    }
}
