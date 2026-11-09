# Commands

A plugin adds commands to the CLI and the desktop. The manifest's `[[contributes.commands]]` is the schema: the daemon
checks every call's arguments against it before the plugin runs, so the plugin receives only declared, correctly typed
arguments. No example plugin contributes a command yet; the manifest below is written from
`proto::plugin::CommandContribution`, and the Rust is the engine test fixture `probe`.

## Declaring one

```toml
[[contributes.commands]]
id = "greet"
title = "Greet"
description = "Say hello"
scope = "top"

[[contributes.commands.args]]
name = "name"
kind = "string"
required = true
help = "Who to greet"
```

| Key | Meaning |
|---|---|
| `id` | the command name; a slug, unique in the plugin |
| `title`, `description` | shown in listings and `--help` |
| `scope` | what it acts on: `top` (default), `server` or `instance` |
| `args` | the declared arguments |

An argument has a `name` (a slug, unique per command), a `kind`, `required` (default `false`) and `help`. The kinds:

| Kind | JSON value | CLI flag |
|---|---|---|
| `string` (default) | string | `--name value` |
| `number` | number | `--count 3` |
| `bool` | boolean | `--flag` |
| `entry` | string naming an entry | `--<name> value` |

`entry` is only checked to be a string; the daemon does not resolve it. An omitted optional argument is absent from the
object the plugin receives, never `null`. An undeclared argument, a missing required one or a wrong type is refused with
`PluginCommandInvalid` naming the argument.

## Scopes

- **`top`** commands live under the plugin: `hestia <plugin> <command>`. They take no entry.
- **`server`** and **`instance`** commands act on one entry and read like the built-in verbs:
  `hestia server <name> <command>`, `hestia instance <name> <command>`. The daemon resolves `<name>` to the entry's id
  and adds it to the arguments as `entry`, so a plugin never sees a name that could mean another entry. A scoped
  command may not declare an argument called `entry`.

A command id may not be a built-in verb of its scope (`RESERVED_SERVER_VERBS`, `RESERVED_INSTANCE_VERBS` in
`proto::plugin`), and a plugin id may not be a top-level CLI verb (`RESERVED_IDS`); the manifest is refused otherwise.
If two plugins contribute the same scoped command, the CLI asks which to run (without a terminal it errors, listing
them). A scoped command is refused while a job that owns the entry's files runs; a running server or instance is fine. The
entry is not claimed while the command runs: a command that must exclude other work takes that itself.

## Writing the handler

`Plugin::run(command, args)` receives the command id and the arguments object, and returns a JSON value. `hestia::args`
reads the object into a struct and refuses a mismatch with the declaration as `Error::Invalid`. Progress goes through
`hestia::progress::report(fraction, message)`, and a long command polls `hestia::progress::cancelled()`.

```rust
fn run(command: &str, args: Value) -> Result<Value, Error> {
    match command {
        "echo" => Ok(args),
        "progress" => {
            hestia::progress::report(Some(0.5), "halfway");
            hestia::progress::report(None, "finishing");
            Ok(json!({ "cancelled": hestia::progress::cancelled() }))
        }
        "wait" => {
            while !hestia::progress::cancelled() {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(Error::Failed("stopped".into()))
        }
        _ => Err(Error::NotFound(format!("unknown command `{command}`"))),
    }
}
```

Source: `Plugin::run` in `crates/engine/tests/fixture/plugins/probe/src/lib.rs`, with the process arms removed.

- `report(Some(0.0..=1.0), message)` is a fraction; `None` leaves it indeterminate. The CLI shows the message inline.
- A command is a job (`plugin.run`). Its limit is one hour. A cancel only asks: `cancelled()` starts returning `true`,
  and the call is dropped 10 seconds later if it has not returned. What the command already did stays done.
- Every call runs on a fresh instance. Keep state in `hestia::data`.

## Output

The returned value is the job's result. The CLI renders an object as a key/value block, a list of objects as a table, a
string as a line and anything else as indented JSON; `--json` prints it raw. A value that is a `UiNode` tree renders
like a page without actions ([ui.md](ui.md)).

## How front-ends run them

| | |
|---|---|
| CLI | `hestia <plugin>` lists the plugin's commands; `hestia <plugin> <command> --help` prints the declared arguments; arguments are `--name value` or `--name=value`. Reads `plugin.list` |
| Desktop | `plugin.commands` returns every command of every enabled plugin that can run them. A scoped command appears in the server's or instance's actions menu; a `top` command in that plugin's row on the settings page. A form asks for the arguments |
| Both | start `plugin.run` with `{ plugin, command, entry?, args }` and follow the job |

`plugin.run` carries no plugin permission, so a plugin cannot start another's command through `host.call`
([0092](../decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)). Only an enabled plugin's commands run.
