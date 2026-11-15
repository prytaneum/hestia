# Events, schedules and hooks

Three ways a plugin reacts instead of being asked: it hears daemon events, it wakes on a timer, and it adjusts or
vetoes an operation at a hook point.

## Events

Subscribe by listing topic globs in the manifest. The daemon decides once per event, before anything is queued,
whether this plugin may receive it.

```toml
[permissions]
"process:read" = {}
"process:spawn" = { programs = ["agent"] }

[contributes]
events = ["process.started", "process.exit"]
```

A glob is an exact topic (`process.exit`) or a prefix with a trailing `.*` (`process.*`, which matches every topic
beneath `process.` but not `process` itself). Topics are lowercase letters, digits, `.`, `-` and `_`. The globs are
part of the grant.

Events arrive in `Plugin::event(call: Events)`. Route each topic to a handler typed by its `proto` payload:

```rust
impl Plugin for Watcher {
    fn event(call: Events) {
        call.on::<ProcessExitEvent>(|exit| {
            hestia::log::info(&format!("{} exited", exit.id));
        })
        .on::<ScheduleEvent>(|tick| {
            if tick.id == "poll" {
                hestia::log::info("polling");
            }
        })
        .glob("server.*", |topic, _payload| {
            hestia::log::debug(topic);
        })
        .finish()
    }
}
```

Source: `Events` in `crates/sdk/src/events.rs`.

The first matching handler takes the event; one no handler takes is ignored. A payload that does not decode is logged
and dropped. `glob` takes every topic its pattern matches with the raw camelCase JSON payload; put typed handlers
first.

### Topics and the permission each needs

A topic is delivered only if the grant also holds its permission. A topic with no permission is never delivered, even
when a glob matches.

| Topic | Payload | Needs |
|---|---|---|
| `process.started` | `ProcessStartedEvent { id, pid }` | `process:read` |
| `process.exit` | `ProcessExitEvent { id, state, exitCode?, success }` | `process:read` |
| `process.metrics` | `ProcessMetricsEvent { samples: [{ id, cpuPct, memBytes }] }` | `process:read` |
| `changed` | `ChangedEvent { scope, ids }` | the scope's permission, below |
| `account.signed_in`, `account.signed_out` | `{ uuid, name }`; never a token | `account:read` |
| `server.ready` | `ServerReadyEvent { id }`; fires once per start, when the server answers a Server List Ping | `entry:read` |
| `ipc.readable`, `ipc.closed` | `{ connection }`; only to the owning plugin | `ipc:connect` |
| `job` | `JobEvent` | the permission of the channel that started the job |
| `net.state` | `NetworkStatus` | `network:connect` |
| `plugin.state` | `{ id, state }` | nothing |
| `plugin.schedule` | `ScheduleEvent { id }`; only to the plugin that declared it | nothing |

`process.output`, `plugin.log`, `plugin.updates`, `plugin.auto_updated` and `plugin.rolled_back` are never delivered, and
a topic the daemon adds later is closed to plugins until it is classified.

`changed` names a scope and the stable `ids` the call touched (entry ids, or plugin ids for `plugins`). Read `ids` as
"at least these", and refetch the whole scope when it is empty. Scope permissions: `entries`, `sync` and `screenshots`
need `entry:read`; `accounts` needs `account:read`; `config` needs `config:read`; `java` needs `java:read`; `skins`,
`cache` and `plugins` are never delivered.

### Delivery

- **One at a time, in order**, each on a fresh instance with a 5 second limit. Keep state in `hestia::data`.
- **Bounded.** The inbox holds 256 events; when full, the oldest is dropped.
- **Budgeted.** Every schedule tick and delivered event spends one token from a bucket of 30 that refills one per second.
  When it is empty a tick is skipped and an event waits. Calls somebody asked for (a command, a page, a provider
  operation, a hook) never spend from it. A plugin that wants a call a millisecond should batch the work.
- **Three timeouts in a row** end delivery until the plugin is reloaded. Three traps within ten minutes disable it.
- Delivery starts when the plugin is loaded enabled and stops when it is disabled, removed or reloaded.

## Schedules

A plugin that must act on a clock declares timers instead of subscribing to a topic:

```toml
[[contributes.schedules]]
id = "poll"
every = 300
```

`every` is whole seconds, 1 to 2592000, and a plugin has at most 8. Each tick is a `plugin.schedule` event
`{ "id": "poll" }` for that plugin alone; it needs no permission and no `events` entry. The first tick comes one period
after the plugin loads; ticks missed while the machine slept are skipped, and a tick is dropped while one of the same
schedule still waits. Ticks share the call budget above. No example plugin uses a schedule.

## Local connections

A plugin holding a connection open with [`hestia::ipc`](api.md#local-sockets) is told when something happens on it:

```toml
[permissions]
"ipc:connect" = { endpoints = ["discord-ipc-*"] }

[contributes]
events = ["ipc.*"]
```

```rust
fn event(call: Events) {
    call.on::<IpcReadableEvent>(|readable| {
        let connection = hestia::ipc::Connection::resume(readable.connection);
        let bytes = connection.receive(64 * 1024, Duration::ZERO);
    })
    .on::<IpcClosedEvent>(|closed| {
        hestia::log::info(&format!("connection {} closed", closed.connection));
    })
    .finish()
}
```

`ipc.readable` is edge-triggered: it fires once when bytes arrive while no `receive` is waiting, and again only after the
plugin has called `receive` on that connection (or when a `receive` left bytes behind). Drain with one large `receive`.
`ipc.closed` fires once when the peer closes or the connection fails, not for one the plugin closed itself; bytes that
arrived first can still be read.

## Hooks

A hook point is a moment an operation lets plugins adjust or refuse it. A hook is customisation, not a control: it is
bounded and fails open, so do not present one as a policy or a scanner.

| Point | Runs | Context | Patch | Permissions to declare it |
|---|---|---|---|---|
| `before-launch` | once per instance launch, after the plan is assembled | `LaunchContext` | `LaunchPatch` | `launch:patch`, `account:read` |
| `before-server-start` | once per server start, before spawn | `ServerContext` | `LaunchPatch` (game arguments ignored) | `launch:patch`, `account:read` |
| `before-content-install` | once per file about to land in an entry | `InstallContext` | none (`Empty`) | `content:read` |

The contexts (`crates/proto/src/hook.rs`) carry what the plugin needs to decide: the entry, game version, flavor,
Java path and memory for a launch; the source, project, version and file name and sha1 for an install. They never carry
the access token, the raw command line or the file's bytes. Declaring a point without its permissions is refused at
`pack` and at install.

```toml
[permissions]
"launch:patch" = {}
"account:read" = {}

[contributes]
hooks = ["before-launch"]
```

Source: `plugins/launch-flags/plugin.toml`.

Route each point with `Intercept::on`. The handler returns the patch to add, or an `Err(reason)` that vetoes the
operation. From the doc example of `crates/sdk/src/hooks.rs`:

```rust
use hestia::hooks::{Intercept, Intercepted};
use hestia::proto::hook::{BeforeLaunch, LaunchPatch};

struct Tuner;

impl hestia::Plugin for Tuner {
    fn intercept(call: Intercept) -> Intercepted {
        call.on::<BeforeLaunch>(|context| {
            let mut patch = LaunchPatch::default();
            if context.memory.is_none() {
                patch.extra_jvm_args.push("-XX:+UseZGC".into());
            }
            Ok(patch)
        })
        .finish()
    }
}

hestia::plugin!(Tuner);
```

A handler vetoes with `Err(Error::Denied("not on this list".into()))`; `before-content-install` answers `Ok(Empty {})` to allow.

The [`launch-flags`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/launch-flags) example reads a
`select` setting and returns garbage collector flags.

- Each plugin runs under a 2 second limit. A timeout, a trap or an answer that is not the point's patch skips that
  plugin with a `PluginHookFailed` warning, and the operation goes ahead.
- Only an `Err` stops the operation: later plugins are not called and it fails with `PluginVetoed { plugin, hook,
  reason }`. For `before-content-install` the veto refuses that file only, with the file name in the reason.
- Plugins run in plugin-id order and their patches fold in that order.
- A hook cannot change the account, the Java runtime or the game version.

### What a launch patch may add

`LaunchPatch` has `extra_jvm_args`, `extra_game_args` and `wrapper`. The host keeps what an allowlist
(`crates/engine/src/plugins/patch.rs`) names and drops the rest with a `LaunchArgDropped` warning each.

- `-XX:+Flag` / `-XX:-Flag` for `UseG1GC`, `UseZGC`, `UseParallelGC`, `ZGenerational`, `UnlockExperimentalVMOptions`,
  `AlwaysPreTouch`, `ParallelRefProcEnabled`, `DisableExplicitGC`.
- `-XX:Name=value` for `MaxGCPauseMillis`, `G1NewSizePercent`, `G1ReservePercent`, `G1MixedGCCountTarget`,
  `InitiatingHeapOccupancyPercent`, `ParallelGCThreads`, `ConcGCThreads` (digits), and `G1HeapRegionSize`,
  `MetaspaceSize`, `MaxMetaspaceSize`, `ReservedCodeCacheSize` (digits with an optional `k`, `m` or `g`).
- `-Dkey=value` only for a key in the `properties` of the grant's `launch:patch`; the value is at most 1024 characters.
- No code-loading option (`-javaagent`, `-cp`, `--module-path`, ...) and no heap option (`-Xmx`).
- Game arguments: `--fullscreen`, `--width N` and `--height N`. All are dropped for a server start.

`wrapper` names one of the plugin's own `[[programs]]` (a `PATH` program), and the host runs the game or server through
it, such as `gamemoderun` or `mangohud`. It needs `process:spawn` for that program; otherwise it is dropped with the same
warning. Wrappers nest, the first plugin in id order outermost. No example plugin wraps a launch; this is written from
the types and not compiled:

```toml
[permissions]
"launch:patch" = {}
"account:read" = {}
"process:spawn" = { programs = ["mangohud"] }

[[programs]]
id = "mangohud"
command = "mangohud"

[contributes]
hooks = ["before-launch"]
```

```rust
LaunchPatch {
    wrapper: Some("mangohud".into()),
    ..LaunchPatch::default()
}
```
