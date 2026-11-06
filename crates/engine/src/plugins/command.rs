//! A plugin's contributed commands: finding one in the manifest and checking a
//! call's arguments against what the manifest declares. The declared list is the
//! schema; the guest is never called with arguments that do not match it.

use proto::error::ErrorInfo;
use proto::plugin::{
    CommandArg, CommandArgKind, CommandContribution, CommandScope, Manifest, ENTRY_ARG,
};
use serde_json::{Map, Value};

fn refuse(plugin: &str, command: &str, detail: String) -> ErrorInfo {
    ErrorInfo::PluginCommandInvalid {
        plugin: plugin.to_string(),
        command: command.to_string(),
        detail,
    }
}

/// The command `name` of `manifest`.
pub fn find_command<'a>(
    manifest: &'a Manifest,
    name: &str,
) -> Result<&'a CommandContribution, ErrorInfo> {
    manifest
        .contributes
        .commands
        .iter()
        .find(|command| command.id == name)
        .ok_or_else(|| refuse(&manifest.id, name, "no such command".into()))
}

/// The arguments object to hand the guest: `args` checked against the
/// declaration, plus `entry` for a scoped command. `entry` is the id the daemon
/// resolved; a top command takes none and a scoped one requires it.
pub fn command_args(
    plugin: &str,
    command: &CommandContribution,
    entry: Option<&str>,
    args: &Value,
) -> Result<Value, ErrorInfo> {
    let invalid = |detail: String| refuse(plugin, &command.id, detail);
    let mut given = match args {
        Value::Null => Map::new(),
        Value::Object(map) => map.clone(),
        _ => return Err(invalid("arguments must be an object".into())),
    };
    for name in given.keys() {
        if !command.args.iter().any(|arg| &arg.name == name) {
            return Err(invalid(format!("unknown argument `{name}`")));
        }
    }
    for arg in &command.args {
        match given.get(&arg.name) {
            None | Some(Value::Null) if arg.required => {
                return Err(invalid(format!("argument `{}` is required", arg.name)))
            }
            None | Some(Value::Null) => {
                given.remove(&arg.name);
            }
            Some(value) if !matches_kind(arg, value) => {
                return Err(invalid(format!(
                    "argument `{}` must be a {}",
                    arg.name,
                    kind_name(arg.kind)
                )))
            }
            Some(_) => {}
        }
    }
    match (command.scope, entry) {
        (CommandScope::Top, None) => {}
        (CommandScope::Top, Some(_)) => {
            return Err(invalid("takes no entry".into()));
        }
        (_, None) => return Err(invalid("needs an entry".into())),
        (_, Some(entry)) => {
            given.insert(ENTRY_ARG.into(), Value::String(entry.into()));
        }
    }
    Ok(Value::Object(given))
}

fn matches_kind(arg: &CommandArg, value: &Value) -> bool {
    match arg.kind {
        CommandArgKind::String | CommandArgKind::Entry => value.is_string(),
        CommandArgKind::Number => value.is_number(),
        CommandArgKind::Bool => value.is_boolean(),
    }
}

fn kind_name(kind: CommandArgKind) -> &'static str {
    match kind {
        CommandArgKind::String => "string",
        CommandArgKind::Entry => "string naming an entry",
        CommandArgKind::Number => "number",
        CommandArgKind::Bool => "boolean",
    }
}
