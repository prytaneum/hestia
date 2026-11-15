# Commands

A plugin adds commands to the CLI and the desktop. The manifest's `[[contributes.commands]]` is the schema: the daemon
checks every call's arguments against it before the plugin runs, so the plugin receives only declared, correctly typed
arguments.

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
| `id` | one to three slugs separated by a space (`players allow`), unique in the plugin. The CLI matches the longest id, so `players` and `players allow` may both exist |
| `title`, `description` | shown in listings and `--help` |
| `scope` | `top` (default), `server` or `instance` |
| `args` | the declared arguments |

An argument has a `name` (slug, unique per command), a `kind`, `required` (default `false`) and `help`:

| Kind | JSON value | CLI |
|---|---|---|
| `string` (default) | string | `--name value` |
| `number` | number | `--count 3` |
| `bool` | boolean | `--flag` |
| `entry` | string naming an entry; not resolved or checked beyond being a string | `--<name> value` |
| `words` | array of strings | the bare words after the command: `players allow steve alex` gives `names = ["steve", "alex"]` |

A command has at most one `words` argument and it is the last. An omitted optional argument is absent from what the
plugin receives, never `null`. An undeclared argument, a missing required one or a wrong type is refused with
`PluginCommandInvalid` naming the argument.

## Scopes

- **`top`** commands live under the plugin: `hestia <plugin> <command>`. They take no entry.
- **`server`** and **`instance`** commands act on one entry and read like built-in verbs: `hestia server <name>
  <command>`. The daemon resolves `<name>` to the entry's id and passes it as `entry`, so a plugin never sees a name that
  could mean another entry. A scoped command may not declare an argument named `entry`.

A command id may not be a built-in verb of its scope, and if two plugins contribute the same scoped command the CLI
asks which to run. A scoped command is refused while a job that owns the entry's files runs; a running server is fine.

## The handler

`Plugin::command(call: Commands)` routes by command id: `call.on("name", handler)`, then `finish()`. The handler takes
the plugin's own struct of the declared arguments and returns any serializable value, which is the job's result. A
payload that does not fit the struct is `Error::Invalid`, meaning the struct and the declaration disagree. Use
`hestia::proto::Empty` for no arguments. A command no handler takes answers `unsupported`.

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

Source: `Commands` in `crates/sdk/src/commands.rs`.

Wrap the struct in `hestia::commands::Scoped` to receive the entry id of a scoped command beside the arguments:

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

Source: `Plugin::command` in the engine test fixture `crates/engine/tests/fixture/plugins/probe/src/lib.rs`, with the
other commands removed.

A command is a job (`plugin.run`) with a limit of one hour. What it already did stays done when it is cancelled.

## Output

The CLI renders an object as a key/value block, a list of objects as a table, a string as a line and anything else as
indented JSON; `--json` prints it raw. A [`UiNode`](ui.md) tree renders like a page without actions.

## How front-ends run them

The CLI lists a plugin's commands with `hestia <plugin>` and takes arguments as `--name value` or `--name=value`. The
desktop shows a scoped command in the server's or instance's actions menu and a `top` command on the plugin's page, with
a form for the arguments. Both start `plugin.run` and follow the job. `plugin.run` needs no plugin permission and is
closed to plugins, so one plugin cannot start another's command. Only an enabled plugin's commands run.
