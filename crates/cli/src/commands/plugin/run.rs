//! Commands an installed plugin contributes: `hestia <plugin> <command>` for a
//! top-level one, `hestia server|instance <name> <command>` for an entry-scoped
//! one. Arguments are parsed here against the command's declared list; the
//! daemon resolves the entry and runs the command as a job.

use std::sync::Arc;

use anyhow::{anyhow, bail, Result};
use client::proto::plugin::{
    CommandArg, CommandArgKind, CommandContribution, CommandScope, PluginInfo, PluginRunParams,
    PluginRunProgress, PluginState,
};
use client::proto::ui::UiNode;
use client::Client;
use serde_json::{Map, Number, Value};

use crate::commands::connect;
use crate::ui::plugin::views;
use crate::ui::{self, Spinner, View};

pub async fn top(argv: Vec<String>) -> Result<()> {
    let (id, rest) = argv
        .split_first()
        .ok_or_else(|| anyhow!("missing plugin name"))?;
    let client = connect().await?;
    let plugins = client.plugin().list().await?;
    let Some(plugin) = plugins.iter().find(|p| p.manifest.id == *id) else {
        let installed: Vec<&str> = plugins.iter().map(|p| p.manifest.id.as_str()).collect();
        bail!(
            "unrecognized subcommand '{id}'; installed plugins: {}",
            joined(&installed)
        );
    };
    if plugin.state != PluginState::Enabled {
        bail!(
            "'{id}' is {}; `hestia plugin enable {id}` loads it",
            super::state_label(plugin.state)
        );
    }
    let commands: Vec<&CommandContribution> = commands_of(plugin, CommandScope::Top).collect();
    let Some((name, args)) = rest.split_first() else {
        return list_commands(id, &commands);
    };
    let Some(command) = commands.iter().find(|c| c.id == *name) else {
        if super::page::is_page(plugin, name) {
            return super::page::open(&client, plugin, name, args).await;
        }
        let ids: Vec<&str> = commands.iter().map(|c| c.id.as_str()).collect();
        bail!("'{id}' has no command or page '{name}'; commands: {}", joined(&ids));
    };
    execute(&client, id, command, CommandScope::Top, None, args).await
}

pub async fn scoped(
    client: &Client,
    scope: CommandScope,
    entry: &str,
    argv: Vec<String>,
) -> Result<()> {
    let (name, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("missing command"))?;
    let plugins = client.plugin().list().await?;
    let matches: Vec<(&PluginInfo, &CommandContribution)> = plugins
        .iter()
        .filter(|p| p.state == PluginState::Enabled)
        .flat_map(|p| commands_of(p, scope).map(move |c| (p, c)))
        .filter(|(_, c)| c.id == *name)
        .collect();
    let (plugin, command) = match matches.as_slice() {
        [] => bail!("unrecognized command '{name}'; run `--help` for the built-in ones"),
        [only] => *only,
        several => {
            let ids: Vec<String> = several.iter().map(|(p, _)| p.manifest.id.clone()).collect();
            if !ui::is_interactive() {
                bail!(
                    "'{name}' is contributed by {}; no terminal to choose one",
                    ids.join(", ")
                );
            }
            several[ui::select(&format!("run '{name}' from"), &ids)?]
        }
    };
    execute(
        client,
        &plugin.manifest.id,
        command,
        scope,
        Some(entry),
        args,
    )
    .await
}

fn commands_of(
    plugin: &PluginInfo,
    scope: CommandScope,
) -> impl Iterator<Item = &CommandContribution> {
    plugin
        .manifest
        .contributes
        .commands
        .iter()
        .filter(move |c| c.scope == scope)
}

fn joined(ids: &[&str]) -> String {
    if ids.is_empty() {
        "(none)".to_string()
    } else {
        ids.join(", ")
    }
}

fn list_commands(plugin: &str, commands: &[&CommandContribution]) -> Result<()> {
    if commands.is_empty() {
        return ui::show(View::note(format!("'{plugin}' contributes no commands")));
    }
    ui::show(View::table(
        format!("{plugin} commands"),
        ["COMMAND", "TITLE", "DESCRIPTION"],
        commands
            .iter()
            .map(|c| vec![c.id.clone(), c.title.clone(), c.description.clone()])
            .collect(),
    ))
}

async fn execute(
    client: &Client,
    plugin: &str,
    command: &CommandContribution,
    scope: CommandScope,
    entry: Option<&str>,
    argv: &[String],
) -> Result<()> {
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        return show_help(plugin, command, scope);
    }
    let (args, json) = parse_args(&command.args, argv)?;
    let spinner = Arc::new(Spinner::start(format!("running {plugin} {}", command.id)));
    let reporter = spinner.clone();
    let done = client
        .plugin()
        .run(
            PluginRunParams {
                plugin: plugin.to_string(),
                command: command.id.clone(),
                entry: entry.map(str::to_string),
                args: Value::Object(args),
                ..PluginRunParams::default()
            },
            move |progress| {
                if !progress.message.is_empty() {
                    reporter.set(progress_label(progress));
                }
            },
        )
        .await;
    drop(spinner);
    let output = done?.output;
    if json {
        return ui::show(View::line(format!("{output:#}")));
    }
    if let Ok(tree) = serde_json::from_value::<UiNode>(output.clone()) {
        return views(&tree).into_iter().try_for_each(ui::show);
    }
    ui::show(render(&output))
}

fn progress_label(progress: &PluginRunProgress) -> String {
    match progress.fraction {
        Some(_) => format!("{} {:.0}%", progress.message, progress.ratio() * 100.0),
        None => progress.message.clone(),
    }
}

fn show_help(plugin: &str, command: &CommandContribution, scope: CommandScope) -> Result<()> {
    let prefix = match scope {
        CommandScope::Top => format!("hestia {plugin}"),
        CommandScope::Server => "hestia server <name>".to_string(),
        CommandScope::Instance => "hestia instance <name>".to_string(),
    };
    ui::show(View::line(format!("{prefix} {} [options]", command.id)))?;
    if !command.description.is_empty() {
        ui::show(View::note(command.description.clone()))?;
    }
    let mut rows: Vec<(String, String)> = command
        .args
        .iter()
        .map(|arg| {
            let flag = match arg.kind {
                CommandArgKind::Bool => format!("--{}", arg.name),
                kind => format!("--{} <{}>", arg.name, kind_label(kind)),
            };
            let required = if arg.required { " (required)" } else { "" };
            (flag, format!("{}{required}", arg.help))
        })
        .collect();
    rows.push((
        "--json".to_string(),
        "Print the raw JSON output".to_string(),
    ));
    ui::show(View::detail(rows))
}

fn kind_label(kind: CommandArgKind) -> &'static str {
    match kind {
        CommandArgKind::String => "string",
        CommandArgKind::Number => "number",
        CommandArgKind::Bool => "bool",
        CommandArgKind::Entry => "entry",
    }
}

/// `--json` is the CLI's own flag and is never passed on.
fn parse_args(declared: &[CommandArg], argv: &[String]) -> Result<(Map<String, Value>, bool)> {
    let mut args = Map::new();
    let mut json = false;
    let mut tokens = argv.iter();
    while let Some(token) = tokens.next() {
        let Some(flag) = token.strip_prefix("--") else {
            bail!("unexpected '{token}'; arguments are given as --name value");
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (flag, None),
        };
        if name == "json" && inline.is_none() {
            json = true;
            continue;
        }
        let Some(arg) = declared.iter().find(|a| a.name == name) else {
            let names: Vec<String> = declared.iter().map(|a| format!("--{}", a.name)).collect();
            if names.is_empty() {
                bail!("unknown argument --{name}; this command takes none");
            }
            bail!("unknown argument --{name}; declared: {}", names.join(", "));
        };
        let value = match arg.kind {
            CommandArgKind::Bool => match inline.as_deref() {
                None => Value::Bool(true),
                Some(text) => Value::Bool(
                    text.parse()
                        .map_err(|_| anyhow!("--{name} takes true or false, not '{text}'"))?,
                ),
            },
            kind => {
                let raw = inline
                    .or_else(|| tokens.next().cloned())
                    .ok_or_else(|| anyhow!("--{name} needs a value"))?;
                match kind {
                    CommandArgKind::Number => number(name, &raw)?,
                    _ => Value::String(raw),
                }
            }
        };
        args.insert(name.to_string(), value);
    }
    if let Some(missing) = declared
        .iter()
        .find(|a| a.required && !args.contains_key(&a.name))
    {
        bail!("missing required argument --{}", missing.name);
    }
    Ok((args, json))
}

fn number(name: &str, raw: &str) -> Result<Value> {
    if let Ok(integer) = raw.parse::<i64>() {
        return Ok(Value::from(integer));
    }
    raw.parse::<f64>()
        .ok()
        .and_then(Number::from_f64)
        .map(Value::Number)
        .ok_or_else(|| anyhow!("--{name} takes a number, not '{raw}'"))
}

fn render(output: &Value) -> View {
    match output {
        Value::Object(map) => View::detail(map.iter().map(|(k, v)| (k.clone(), cell(v)))),
        Value::Array(items) => table(items).unwrap_or_else(|| View::line(format!("{output:#}"))),
        Value::String(text) => View::line(text.clone()),
        Value::Null => View::note("done"),
        other => View::line(format!("{other:#}")),
    }
}

fn table(items: &[Value]) -> Option<View> {
    let objects: Vec<&Map<String, Value>> = items.iter().filter_map(Value::as_object).collect();
    if objects.is_empty() || objects.len() != items.len() {
        return None;
    }
    let mut headers: Vec<String> = Vec::new();
    for object in &objects {
        for key in object.keys() {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
    }
    let rows = objects
        .iter()
        .map(|object| {
            headers
                .iter()
                .map(|key| object.get(key).map(cell).unwrap_or_default())
                .collect()
        })
        .collect();
    Some(View::table("Output", headers, rows))
}

fn cell(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use client::proto::plugin::{RESERVED_IDS, RESERVED_INSTANCE_VERBS, RESERVED_SERVER_VERBS};
    use serde_json::json;

    use super::*;

    fn arg(name: &str, kind: CommandArgKind, required: bool) -> CommandArg {
        CommandArg {
            name: name.into(),
            kind,
            required,
            help: String::new(),
        }
    }

    fn argv(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(ToString::to_string).collect()
    }

    fn declared() -> Vec<CommandArg> {
        vec![
            arg("name", CommandArgKind::String, true),
            arg("count", CommandArgKind::Number, false),
            arg("loud", CommandArgKind::Bool, false),
        ]
    }

    #[test]
    fn flags_values_and_bools_become_a_json_object() {
        let (args, json) = parse_args(
            &declared(),
            &argv(&["--name", "world", "--count=3", "--loud"]),
        )
        .unwrap();
        assert!(!json);
        assert_eq!(
            Value::Object(args),
            json!({"name": "world", "count": 3, "loud": true})
        );
    }

    #[test]
    fn json_is_the_cli_flag_and_a_bool_takes_an_explicit_value() {
        let (args, json) =
            parse_args(&declared(), &argv(&["--name=a", "--loud=false", "--json"])).unwrap();
        assert!(json);
        assert_eq!(Value::Object(args), json!({"name": "a", "loud": false}));
    }

    #[test]
    fn a_number_may_be_fractional_and_must_be_a_number() {
        let (args, _) = parse_args(&declared(), &argv(&["--name=a", "--count", "1.5"])).unwrap();
        assert_eq!(args["count"], json!(1.5));
        let error = parse_args(&declared(), &argv(&["--name=a", "--count", "many"]));
        assert!(error.unwrap_err().to_string().contains("--count"));
    }

    #[test]
    fn a_missing_required_argument_is_named() {
        let error = parse_args(&declared(), &argv(&["--loud"])).unwrap_err();
        assert!(error.to_string().contains("--name"));
    }

    #[test]
    fn an_unknown_argument_lists_the_declared_ones() {
        let error = parse_args(&declared(), &argv(&["--nope", "x"])).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("--nope"));
        assert!(message.contains("--name, --count, --loud"));
        let error = parse_args(&[], &argv(&["--nope"])).unwrap_err();
        assert!(error.to_string().contains("takes none"));
    }

    #[test]
    fn a_value_flag_without_a_value_and_a_stray_word_are_refused() {
        assert!(parse_args(&declared(), &argv(&["--name"])).is_err());
        assert!(parse_args(&declared(), &argv(&["stray"])).is_err());
    }

    #[test]
    fn output_maps_to_the_view_its_shape_suggests() {
        assert!(matches!(
            render(&json!({"a": 1, "b": "x"})),
            View::Detail(rows) if rows == [("a".into(), "1".into()), ("b".into(), "x".into())]
        ));
        assert!(matches!(
            render(&json!([{"id": "a", "n": 1}, {"id": "b"}])),
            View::Table { headers, rows, .. }
                if headers == ["id", "n"] && rows[1] == ["b", ""]
        ));
        assert!(matches!(render(&json!("hi")), View::Line(text) if text == "hi"));
        assert!(matches!(render(&json!([1, 2])), View::Line(_)));
        assert!(matches!(render(&json!([])), View::Line(_)));
        assert!(matches!(render(&Value::Null), View::Note(_)));
    }

    fn assert_reserved(command: &clap::Command, reserved: &[&str]) {
        for sub in command.get_subcommands() {
            for name in std::iter::once(sub.get_name()).chain(sub.get_all_aliases()) {
                assert!(
                    reserved.contains(&name),
                    "`{name}` is a built-in verb missing from the reserved list"
                );
            }
        }
    }

    #[test]
    fn the_reserved_lists_cover_every_built_in_verb() {
        assert_reserved(&crate::Cli::command(), RESERVED_IDS);
        assert_reserved(
            &crate::commands::server::ServerEntry::command(),
            RESERVED_SERVER_VERBS,
        );
        assert_reserved(
            &crate::commands::instance::InstanceEntry::command(),
            RESERVED_INSTANCE_VERBS,
        );
    }
}
