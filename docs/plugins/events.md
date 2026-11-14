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
| `changed` | `ChangedEvent` `{ scope, ids }` | the scope's permission, below |
| `account.signed_in` | `AccountSignedInEvent` `{ uuid, name }` | `account:read` |
| `account.signed_out` | `AccountSignedOutEvent` `{ uuid, name }` | `account:read` |
| `server.ready` | `ServerReadyEvent` `{ id }` | `entry:read` |
| `ipc.readable` | `IpcReadableEvent` `{ connection }` | `ipc:connect`; only the plugin that owns the connection ([below](#local-connections)) |
| `ipc.closed` | `IpcClosedEvent` `{ connection }` | `ipc:connect`; only the plugin that owns the connection |
| `job` | `JobEvent` | the permission of the channel that started the job |
| `net.state` | `NetworkStatus` | `network:connect` |
| `plugin.state` | `{ id, state }` | nothing; cut to those two fields |
| `plugin.schedule` | `ScheduleEvent` `{ id }` | nothing; only to the plugin that declared it ([below](#schedules)) |
| `plugin.log` | `PluginLogEvent` | nothing: never delivered |
| `plugin.updates`, `plugin.auto_updated`, `plugin.rolled_back` | `Outdated`, `PluginAutoUpdatedEvent`, `PluginRolledBackEvent` | nothing: never delivered |

`changed` names the scope that changed and, in `ids`, the stable ids the call touched, never what changed in them. `ids`
holds entry ids for `entries` (a server or instance the caller named by display name arrives as its id, and a rename
lists both the old id and the new one) and plugin ids for `plugins`; it is empty when the call touched nothing
the daemon could name, which means the whole scope. Always read `ids` as "at least these": refetch the scope when it is
empty.

The account topics carry the profile only, never a token. `account.signed_in` also fires when an account that needed
re-authentication signs in again. `server.ready` fires once per start, when the started server answers a Server List Ping
on its game port, so it accepts players; a server the daemon re-adopts after its own restart announces itself again.
The daemon gives up after ten minutes without an answer.

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
- **Budgeted.** The host will not wake a plugin faster than its call budget: a token bucket of 30 calls that refills at
  one call per second. Every schedule tick and every delivered event spends one token. When the bucket is empty a tick
  is skipped (it is not queued and not made up later) and an event waits for the next token while the inbox behind it
  drops its oldest. The log says so at most once a minute, and `plugin.info` counts the wake-ups held back as
  `throttled`. Calls somebody asked for (a command, a page, a provider operation, a hook) never spend from it. A plugin
  that must react to something rare is well served; one that wants a call a millisecond should keep the work in the
  peer, or batch it.
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

- `every` is whole seconds, 1 to 2592000 (30 days). At most 8 schedules per plugin; ids are unique slugs. The
  [call budget](#delivery) is what limits how often a plugin really runs, so a short `every` is allowed but a plugin
  that ticks every second has nothing left for its events.
- Each tick is a `plugin.schedule` event `{ "id": "poll" }` pushed into that plugin's own inbox. It is never published to
  the hub, so no other plugin or client hears it, a `plugin.*` glob does not reach it and it needs no permission. The
  schedules are part of the grant and listed when the user approves.
- The first tick comes one full period after the plugin loads. A tick missed while the machine slept is skipped, not
  replayed, and a tick is dropped while one of the same schedule still waits in the inbox.
- Timers start and stop with the plugin's event delivery.

A tick is the `proto::plugin::ScheduleEvent` topic, so it routes like any other event (see the handler above).

A plugin with schedules but no `events` still gets its ticks.

## Local connections

A plugin that holds a connection open with [`hestia::ipc`](ipc.md) is told when something happens on it, so it need
not poll from a schedule. The host reads each connection itself and pushes two topics into the owning plugin's inbox,
nowhere else: they are never published to the hub, so no client and no other plugin hears them, and a topic the hub
claims to carry under these names is refused. The plugin subscribes like to any topic and needs `ipc:connect`:

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

- `ipc.readable` is edge-triggered. It fires once when bytes arrive while no call is waiting in `receive`, and not
  again until the plugin has called `receive` on that connection, so a chatty peer produces one event, not one per
  frame. A `receive` that leaves bytes behind (a `max` smaller than what arrived) fires it again. Drain with one large
  `receive`, or until it returns empty.
- `ipc.closed` fires once, when the peer closes the connection or it fails. Bytes that arrived first can still be
  read; the `receive` after them answers `not-found`. It does not fire for a connection the plugin closed itself.
- The events share the inbox, the one-at-a-time order and the [call budget](#delivery) of every other event.
