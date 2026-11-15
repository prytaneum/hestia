# Commands

A plugin adds commands to the CLI and the desktop. The manifest's `[[contributes.commands]]` is the schema: the daemon
checks every call's arguments against it before the plugin runs, so the plugin receives only declared, correctly typed
arguments. The longer handlers below are from the engine test fixture `probe`.

## Declaring one

```toml
[[contributes.commands]]
id = "greet"
title = "Greet"
description = "Say hello"

[[contributes.commands.args]]
name = "name"
kind = "string"
help = "Who to greet"
```

`hestia hello greet --name Bob` answers `hello, Bob`.

| Key | Meaning |
|---|---|
| `id` | the command name: one to three slugs separated by a space, such as `players allow`; unique in the plugin. On the CLI the words are typed in order and the longest id that matches wins, so `players` and `players allow` may both exist |
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
| `words` | array of strings | the bare words after the command: `hestia server smp players allow steve alex` gives `names = ["steve", "alex"]` |

A command has at most one `words` argument and it is the last; flags and bare words may be mixed. A `words` argument that
is `required` is refused when no word is given. On the desktop it is a text field split on whitespace.

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

`Plugin::command(call: Commands)` is routed like a provider or a hook: `call.on("name", handler)` runs `handler` when
`name` is the command's id, and `finish()` ends the chain. A command id is a runtime string from `plugin.toml`, so it is
routed by name; hook points, provider operations and events are defined in `proto` and routed by type.

The handler takes the plugin's own struct of the declared arguments and returns any serializable value, which is the
job's result. The daemon has already checked the arguments against the manifest, so a payload that does not fit the
struct (`Error::Invalid`) means the struct and the declaration disagree. A command with no arguments takes
`hestia::proto::Empty`, and `Value` takes the raw object. A command no handler takes answers `unsupported` naming it.

```rust
#[derive(Deserialize)]
struct Greet {
    name: String,
}

impl Plugin for Hello {
    fn command(call: Commands) -> Ran {
        call.on("greet", |args: Greet| Ok(format!("hello, {}", args.name)))
            .finish()
    }
}
```

Source: `Commands` in `crates/sdk/src/commands.rs` (compiled as a no-run doctest).

A scoped command receives the entry's id as `entry`. Wrap the argument struct in `hestia::commands::Scoped` to get it
next to the arguments:

```rust
call.on("restart", |scoped: Scoped<Greet>| {
    hestia::log::info(&format!("{} on {}", scoped.args.name, scoped.entry));
    Ok(())
})
```

A command that reports progress and honours a cancel:

```rust
call.on("progress", |_: Empty| {
    hestia::progress::report(Some(0.5), "halfway");
    hestia::progress::report(None, "finishing");
    Ok(json!({ "cancelled": hestia::progress::cancelled() }))
})
.on("wait", |_: Empty| -> Result<(), Error> {
    while !hestia::progress::cancelled() {
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(Error::Failed("stopped".into()))
})
```

Source: `Plugin::command` in `crates/engine/tests/fixture/plugins/probe/src/lib.rs`, with the other commands removed.

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
| Desktop | `plugin.commands` returns every command of every enabled plugin that can run them. A scoped command appears in the server's or instance's actions menu; a `top` command on that plugin's page. A form asks for the arguments |
| Both | start `plugin.run` with `{ plugin, command, entry?, args }` and follow the job |

`plugin.run` carries no plugin permission, so a plugin cannot start another's command through `host.call`
([0092](../decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)). Only an enabled plugin's commands run.
