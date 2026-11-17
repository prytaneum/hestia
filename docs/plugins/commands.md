# Running work

*[← Writing a plugin](README.md)*

A plugin does nothing between calls: every call gets a fresh instance, and state lives in `/data`, settings and secrets.
The host calls an export when something happens, and each call has a wall-clock budget.

## Exports

| Export (SDK feature, trait)                     | Called when                              | Budget |
|-------------------------------------------------|------------------------------------------|--------|
| `lifecycle` (`Lifecycle::init`)                 | the plugin is enabled or loaded          | 10 s   |
| `commands` (`Commands::run`)                    | a person or `jobs::start` runs a command | 1 h    |
| `on-schedule` (`OnSchedule::tick`)              | a declared schedule ticks                | 5 s    |
| `on-job-done` (`OnJobDone::finished`)           | a job the plugin started ends            | 5 s    |
| `settings-changed` (`SettingsChanged::changed`) | the user changes one of its settings     | 5 s    |

Enable the feature in `Cargo.toml` (`features = ["lifecycle", "commands"]`), implement the trait and call
`hestia::export!(Plugin)` once. A budget also bounds a call that waits inside a host function, and a call that is
dropped stops the guest. Memory is capped at 256 MiB; at most 4 calls per plugin and 16 overall run at once. A plugin
that traps three times in ten minutes is disabled and stays so across restarts. Exports a plugin does not implement read
as not provided, and adding one in an update asks the user again.

Hestia has no screen or command that changes a plugin's setting yet, so `settings-changed` is not called today;
`settings::get` and `settings::set` (values are JSON text) work.

## Commands

Declare each command in the manifest and implement `Commands::run`, which receives the command name and its arguments as
a JSON object:

```toml
[[commands]]
name = "clean"
summary = "Delete old backups"
args = [{ name = "keep", kind = "integer", required = true }, { name = "dry-run", kind = "boolean" }]
```

```rust
impl Commands for Plugin {
    fn run(command: String, args: String) -> Result<(), Error> {
        match command.as_str() {
            "clean" => {
                for step in 0..10 {
                    if hestia::progress::cancelled() {
                        return Ok(());
                    }
                    hestia::progress::report(step, 10, "cleaning");
                }
                Ok(())
            }
            other => Err(Error::Invalid(format!("no command {other}"))),
        }
    }
}
```

An argument kind is `string`, `integer` or `boolean`. `hestia <plugin> <command> --keep 3 --dry-run` checks the
arguments in the daemon and runs the command as a job (`hestia job list`), with an hour to finish. It reports
`progress::report(current, total, message)` (only the latest report is kept) and is cancelled cooperatively: poll
`progress::cancelled()`; after a cancel the plugin has 5 s, then it is stopped and the job reports `cancelled`. What it
wrote to `/data` stays. A plugin with a command running cannot be disabled, removed or updated. A manifest that declares
commands must export `commands`.

## Schedules

```toml
[[schedules]]
id = "nightly"
every = "1d"
```

`every` is a whole number of minutes, hours or days (`30m`, `6h`, `1d`), counted from the last run, and
`OnSchedule::tick(schedule)` has 5 s, so a tick starts a command rather than doing the work:
`hestia::jobs::start("clean", r#"{"keep":3}"#)`. A run missed while the daemon was down or the machine asleep is made up
once at the next check (the daemon checks every minute); a clock set back never repeats a run. A schedule is part of the
grant: the prompt lists it, and an update that adds one or ticks more often asks again. A manifest that declares
schedules must export `on-schedule`.

## Jobs

`jobs::start(command, args)` queues one of the plugin's own commands as a visible, cancellable job with the command's
hour, and returns its id. It cannot start another plugin's command. When the job ends, `OnJobDone::finished(job,
outcome)` runs with `Succeeded`, `Failed(message)`, `Cancelled` or `Other`. Each `jobs::start` and each schedule tick
spends one wake-up: 30 in a burst, one more each second, per plugin; a call finding none fails with `busy`, a tick
finding none is skipped.

## Talking to the user

`notify::notify(level, message)`, `notify::confirm(message)` and `notify::open_url(url)` never draw anything themselves:
Hestia shows each with the plugin's name, so a plugin cannot imitate Hestia's own dialogs. A call allows five notices,
one question and one link. Only a call a person started may `confirm` (a command run from the CLI or desktop; elsewhere
it answers `unsupported`), and the call's budget pauses while the person decides, for up to two minutes. `open_url`
takes an https address, which the user sees before opening.

## Events and hooks

An event or hook is an export interface that the plugin providing an API package defines and a plugin fills by exporting
it. The kernel's own are the table above. Built-in packages that define more (a server stopped, a hook before a backup)
are not shipped yet, so there are none to hear today. When they arrive, a plugin hears a topic by exporting its
interface (which needs the topic's read permission), events about entries outside its entries scope are not delivered,
and a hook runs briefly (2 s), in plugin-id order, and may veto.
