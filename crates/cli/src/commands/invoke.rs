//! `hestia <plugin> <command> …` — a plugin's own commands. The arguments are
//! the ones its manifest declares, so they are parsed here from the daemon's
//! list; the run is a job whose notices, questions and links this CLI draws.

use anyhow::{anyhow, bail, Result};
use clap::error::{ContextKind, ErrorKind};
use clap::{Arg, ArgAction, ArgMatches, Command};
use client::proto::plugin::{
    ArgKind, CommandSpec, PluginInfo, PluginLink, PluginNotice, PluginQuestion,
    PluginQuestionClosed, PluginState,
};
use client::proto::Topic;
use client::Event;
use serde_json::{Map, Value};
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

use super::account::open_browser;
use crate::ui::{self, View};

fn command_of(plugin: &PluginInfo) -> Command {
    let mut command = Command::new(plugin.id.clone())
        .about(plugin.name.clone())
        .subcommand_required(true)
        .arg_required_else_help(true);
    for spec in &plugin.commands {
        let mut sub = Command::new(spec.name.clone()).about(spec.summary.clone());
        for arg in &spec.args {
            let option = Arg::new(arg.name.clone())
                .long(arg.name.clone())
                .help(arg.summary.clone())
                .required(arg.required);
            sub = sub.arg(match arg.kind {
                ArgKind::String => option.value_name("TEXT"),
                ArgKind::Integer => option
                    .value_name("NUMBER")
                    .value_parser(clap::value_parser!(i64)),
                ArgKind::Boolean => option.action(ArgAction::SetTrue),
            });
        }
        command = command.subcommand(sub);
    }
    command
}

fn arguments(spec: &CommandSpec, given: &ArgMatches) -> String {
    let mut args = Map::new();
    for arg in &spec.args {
        let value = match arg.kind {
            ArgKind::String => given
                .get_one::<String>(&arg.name)
                .map(|v| Value::from(v.clone())),
            ArgKind::Integer => given.get_one::<i64>(&arg.name).map(|v| Value::from(*v)),
            ArgKind::Boolean => given.get_flag(&arg.name).then_some(Value::Bool(true)),
        };
        if let Some(value) = value {
            args.insert(arg.name.clone(), value);
        }
    }
    Value::Object(args).to_string()
}

/// What clap says of a word that is no command of the CLI, suggestions included.
fn unrecognised(cli: &Command, word: &str) -> clap::Error {
    let strict = Command::new("hestia").subcommands(cli.get_subcommands().cloned());
    match strict.try_get_matches_from(["hestia", word]) {
        Err(error) => error,
        Ok(_) => clap::Error::raw(
            ErrorKind::InvalidSubcommand,
            format!("unrecognized subcommand '{word}'"),
        ),
    }
}

/// `cli` is the CLI's own parser, to word a refusal as clap does for any other
/// mistyped command.
pub async fn run(words: Vec<String>, cli: Command) -> Result<()> {
    let Some(name) = words.first().cloned() else {
        bail!("no plugin named");
    };
    let client = match super::connect().await {
        Ok(client) => client,
        Err(error) => {
            let typo = unrecognised(&cli, &name);
            return match typo.get(ContextKind::SuggestedSubcommand) {
                Some(_) => typo.exit(),
                None => Err(error),
            };
        }
    };
    let Some(plugin) = client
        .plugin()
        .list()
        .await?
        .into_iter()
        .find(|plugin| plugin.id == name)
    else {
        unrecognised(&cli, &name).exit()
    };
    if plugin.state != PluginState::Enabled {
        bail!("plugin '{name}' is not enabled");
    }
    if plugin.commands.is_empty() {
        bail!("plugin '{name}' declares no commands");
    }

    let matches = command_of(&plugin)
        .try_get_matches_from(words)
        .unwrap_or_else(|error| error.exit());
    let Some((command, given)) = matches.subcommand() else {
        bail!("no command named");
    };
    let spec = plugin
        .commands
        .iter()
        .find(|spec| spec.name == command)
        .ok_or_else(|| anyhow!("plugin '{name}' has no command '{command}'"))?;
    let args = arguments(spec, given);

    let id = client::job_id(&format!("plugin-{name}"));
    let (seen, mut drawn) = unbounded_channel::<Event>();
    let plugins = client.plugin();
    let job = plugins.run(
        &id,
        &name,
        command,
        &args,
        |_| {},
        move |event| {
            let _ = seen.send(event.clone());
        },
    );
    let outcome = super::cancellable(&client, &id, async {
        let mut job = std::pin::pin!(job);
        let mut asking: Option<Asking> = None;
        loop {
            tokio::select! {
                outcome = &mut job => break outcome,
                Some(event) = drawn.recv() => draw(event, &mut asking),
                (question, accepted) = answered(&mut asking), if asking.is_some() => {
                    let sent = client
                        .plugin()
                        .answer(&question.request, &question.plugin, accepted)
                        .await;
                    if let Err(error) = sent {
                        let _ = ui::show(View::warning(format!("cannot send the answer: {error}")));
                    }
                }
            }
        }
    })
    .await;
    while let Ok(event) = drawn.try_recv() {
        if event.topic != PluginQuestion::TOPIC {
            draw(event, &mut None);
        }
    }
    match outcome {
        Err(client::IpcError::Cancelled) => {
            ui::show(View::note(format!("{name} {command} cancelled")))
        }
        other => other.map_err(Into::into),
    }
}

/// A question on the terminal. The read cannot be interrupted, so a question
/// the daemon withdraws is abandoned: its reader ends with the next line typed.
struct Asking {
    question: PluginQuestion,
    reply: JoinHandle<bool>,
}

async fn answered(asking: &mut Option<Asking>) -> (PluginQuestion, bool) {
    let Some(pending) = asking.as_mut() else {
        return std::future::pending().await;
    };
    let accepted = (&mut pending.reply).await.unwrap_or(false);
    let Asking { question, .. } = asking.take().expect("a question is pending");
    (question, accepted)
}

fn draw(event: Event, asking: &mut Option<Asking>) {
    if let Err(error) = show(event, asking) {
        let _ = ui::show(View::warning(format!(
            "cannot show a plugin's message: {error}"
        )));
    }
}

fn show(event: Event, asking: &mut Option<Asking>) -> Result<()> {
    match event.topic.as_str() {
        PluginNotice::TOPIC => {
            let notice: PluginNotice = serde_json::from_value(event.payload)?;
            let text = format!("[{}] {}", notice.plugin, notice.message);
            ui::show(match notice.level {
                client::proto::plugin::NoticeLevel::Info => View::line(text),
                _ => View::warning(text),
            })
        }
        PluginQuestion::TOPIC => {
            let question: PluginQuestion = serde_json::from_value(event.payload)?;
            let text = format!("[{}] {}", question.plugin, question.message);
            let reply = tokio::task::spawn_blocking(move || {
                ui::confirm(&text, "yes", "no").unwrap_or(false)
            });
            *asking = Some(Asking { question, reply });
            Ok(())
        }
        PluginQuestionClosed::TOPIC => {
            let closed: PluginQuestionClosed = serde_json::from_value(event.payload)?;
            if asking
                .as_ref()
                .is_some_and(|pending| pending.question.request == closed.request)
            {
                *asking = None;
                ui::show(View::warning(
                    "the question was withdrawn: nobody answered in time; press Enter to go on",
                ))?;
            }
            Ok(())
        }
        PluginLink::TOPIC => {
            let link: PluginLink = serde_json::from_value(event.payload)?;
            if ui::confirm(
                &format!("[{}] open {}?", link.plugin, link.url),
                "open",
                "skip",
            )
            .unwrap_or(false)
            {
                open_browser(&link.url);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn a_mistyped_command_is_told_what_it_resembles_and_an_unknown_word_may_be_a_plugin() {
        let cli = crate::Cli::command();
        assert!(unrecognised(&cli, "servr")
            .get(ContextKind::SuggestedSubcommand)
            .is_some());
        assert!(unrecognised(&cli, "tidy")
            .get(ContextKind::SuggestedSubcommand)
            .is_none());
    }
}
