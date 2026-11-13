# Events

A plugin hears daemon events by listing topic globs in the manifest. The daemon decides, once per event and before
anything is queued, whether this plugin may receive it. The rules live in `proto::events` and
`daemon/src/runtime/plugins/events.rs`.

## Subscribing

```toml
[permissions]
"process:read" = {}
"process:spawn" = { programs = ["agent"] }

[contributes]
events = ["process.started", "process.exit"]
```


A glob is an exact topic (`job`, `process.exit`) or a prefix with a trailing `.*` (`process.*`). `process.*` matches every
topic beneath `process.` but not `process` itself. Topics are lowercase letters, digits, `.`, `-` and `_`; `*` alone,
`server*` and `a.*.b` are refused. The globs are part of the grant the user approves.

The plugin receives events in `Plugin::event(call: Events)`. Route each topic to a handler with `call.on::<T>(…)`, typed
by the topic's `proto` type (`T: Topic`, the payload is `T` itself):

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

Source: `Events` in `crates/sdk/src/events.rs` (a no-run doctest).

- The first handler that matches takes the event; one no handler takes is ignored.
- A payload that does not decode as `T` is logged with `hestia::log::warn` and dropped: `event` returns nothing, so an
  event cannot be answered.
- `call.glob("server.*", |topic, payload| …)` takes every topic the pattern matches (an exact topic, a prefix such as
  `server.*`, or `*`), with the topic and the raw JSON payload, camelCase as the proto type serializes it. Put typed
  handlers before it.

## Topics and the permission each needs

A topic is delivered only if the grant also holds the permission below. A topic with no permission is never delivered,
even when a glob matches it.

| Topic | Payload (`proto`) | Needs |
|---|---|---|
| `process.started` | `ProcessStartedEvent` `{ id, pid }` | `process:read` |
| `process.exit` | `ProcessExitEvent` `{ id, state, exitCode?, success }` | `process:read` |
| `process.metrics` | `ProcessMetricsEvent` `{ samples: [{ id, cpuPct, memBytes }] }` | `process:read` |
| `process.output` | `ProcessOutputEvent` | nothing: never delivered |
| `changed` | `ChangedEvent` `{ scope }` | the scope's permission, below |
| `job` | `JobEvent` | the permission of the channel that started the job |
| `net.state` | `NetworkStatus` | `network:connect` |
| `plugin.state` | `{ id, state }` | nothing; cut to those two fields |
| `plugin.schedule` | `ScheduleEvent` `{ id }` | nothing; only to the plugin that declared it ([below](#schedules)) |
| `plugin.log` | `PluginLogEvent` | nothing: never delivered |

`changed` names only the scope that changed, never what changed in it:

| Scope | Needs |
|---|---|
| `entries`, `sync`, `screenshots` | `entry:read` |
| `accounts` | `account:read` |
| `config` | `config:read` |
| `java` | `java:read` |
| `skins`, `cache`, `plugins` | nothing: never delivered |

Every topic the daemon publishes is listed in `proto::events::TOPICS`; a test fails one that is missing, and a topic
added later is closed to plugins until someone classifies it
([0092](../decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)).

## Delivery

- **One at a time, in order.** Each event runs on a fresh instance of the plugin with a 5 second limit. Nothing kept in
  memory survives from one event to the next; keep state in `hestia::data`.
- **Bounded.** Each plugin has an inbox of 256 events. When it is full the oldest is dropped (a warning is logged at most
  once a minute), so a slow plugin loses history rather than growing the daemon.
- **A plugin that keeps timing out stops hearing events.** Three timeouts in a row end delivery until the plugin is loaded
  again (disabled and enabled, or reloaded). Three traps within ten minutes disable the plugin as for any call.
- **Lifecycle.** Delivery starts when the plugin is loaded enabled and stops when it is disabled, removed or reloaded.
- **Calls made from a handler** are gated like any other, and the call chain still stops loops
  ([architecture](../architecture/plugins.md#calling-the-daemon)).

## Schedules

A plugin that must act on a clock declares timers instead of subscribing to a topic
([0108](../decisions/0108-a-schedule-is-an-event-to-its-own-plugin.md)):

```toml
[[contributes.schedules]]
id = "poll"
every = 300
```

The manifest is written from `proto::plugin::ScheduleContribution`.

- `every` is whole seconds, 30 to 2592000 (30 days). At most 8 schedules per plugin; ids are unique slugs.
- Each tick is a `plugin.schedule` event `{ "id": "poll" }` pushed into that plugin's own inbox. It is never published to
  the hub, so no other plugin or client hears it, a `plugin.*` glob does not reach it and it needs no permission. The
  schedules are part of the grant and listed when the user approves.
- The first tick comes one full period after the plugin loads. A tick missed while the machine slept is skipped, not
  replayed, and a tick is dropped while one of the same schedule still waits in the inbox.
- Timers start and stop with the plugin's event delivery.

A tick is the `proto::plugin::ScheduleEvent` topic, so it routes like any other event (see the handler above).

A plugin with schedules but no `events` still gets its ticks.
