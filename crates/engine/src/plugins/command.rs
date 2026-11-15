//! A plugin's contributed commands: finding one in the manifest and checking a
//! call's arguments against what the manifest declares. The declared list is the
//! schema; the guest is never called with arguments that do not match it.

use proto::error::{EntryKind, ErrorInfo};
use proto::plugin::{
    CommandArg, CommandArgKind, CommandContribution, CommandScope, Manifest, PageContribution,
    PageSlot, ENTRY_ARG,
};
use proto::ui::Effect;
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

/// Refuses a page action's `run` effect that names a command `manifest` does not
/// declare, or a scoped command on a page that is not an entry page of that kind.
pub fn check_run_effects(
    manifest: &Manifest,
    page: &PageContribution,
    effects: &[Effect],
) -> Result<(), ErrorInfo> {
    let page_kind = (page.slot == PageSlot::Entry).then(|| page.kind.unwrap_or_default());
    for effect in effects {
        let Effect::Run { command, .. } = effect else {
            continue;
        };
        let declared = find_command(manifest, command)?;
        let fits = match declared.scope {
            CommandScope::Top => true,
            CommandScope::Server => page_kind == Some(EntryKind::Server),
            CommandScope::Instance => page_kind == Some(EntryKind::Instance),
        };
        if !fits {
            tracing::warn!(plugin = %manifest.id, %command, page = %page.id, "run effect refused: scope does not fit the page");
            return Err(refuse(
                &manifest.id,
                command,
                format!(
                    "a {:?} command cannot run from page '{}'",
                    declared.scope, page.id
                ),
            ));
        }
    }
    Ok(())
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
        CommandArgKind::Words => value
            .as_array()
            .is_some_and(|words| words.iter().all(Value::is_string)),
    }
}

fn kind_name(kind: CommandArgKind) -> &'static str {
    match kind {
        CommandArgKind::String => "string",
        CommandArgKind::Entry => "string naming an entry",
        CommandArgKind::Number => "number",
        CommandArgKind::Bool => "boolean",
        CommandArgKind::Words => "list of strings",
    }
}
