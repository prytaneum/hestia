# Host-pushed events and a call budget instead of a resident instance

*Applies to: [Plugins](../architecture/plugins.md#events)*

A plugin that lives beside a local program (rich presence over Discord's IPC socket) must react when the socket has
data or closes, when the user signs in, when a server becomes ready. Until now it could only poll: a schedule, floored
at 30 s ([0108](0108-a-schedule-is-an-event-to-its-own-plugin.md)), asking `receive` whether anything had come in.

**Per-call instances stay.** A plugin is a fresh instance per call ([0091](0091-plugins-are-wasm-components.md)); none
is long-lived. What changes is what wakes it.

- **The host listens and pushes.** Each connection a plugin opens has a reader task in the host. When bytes arrive while
  no call is waiting in `receive`, the host pushes `ipc.readable { connection }` into the owning plugin's inbox; when the
  peer closes it, `ipc.closed { connection }`. They are `proto` topics (`pushed.rs`), delivered through the existing
  `events.on-event` export, subscribed in `contributes.events` and gated by `ipc:connect`. Like `plugin.schedule` they are
  never published to the hub, so no client and no other plugin hears them, and the hub-side filter refuses the names.
  `ipc.readable` is edge-triggered and re-arms on `receive`, so a chatty peer costs one event, not one per frame.
- **More daemon topics.** `account.signed_in` and `account.signed_out` (profile only, `account:read`) and `server.ready`
  (entry id, `entry:read`) join the topic table. Readiness is the first answer to a Server List Ping, which a per-start
  watcher polls; the supervisor knows only that a process runs.
- **`changed` names what changed.** `ChangedEvent` carries `ids`, empty meaning the whole scope. The router collects
  the ids a request resolved (entry lookups in the guards, a plugin id in the plugin handlers) and the registrar
  publishes them, so `Contract::CHANGES` stays a const and no contract grows a function. A call that names nothing
  keeps the whole-scope meaning.
- **A budget replaces the 30 s floor.** The floor existed to bound how often a plugin can make the host run it, but it
  bounded only schedules and made fast, legitimate ticks impossible. Each plugin gets a token bucket of 30 calls
  refilling one per second, spent by every host-initiated wake-up (tick or event) and enforced in one place, where the
  inbox is drained. A tick over budget is skipped, never queued; an event waits for a token while the bounded inbox
  drops its oldest. A refusal is logged at most once a minute and counted in `plugin.info` as `throttled`. Calls a person
  asked for (commands, pages, providers, hooks) are not budgeted. The schedule floor drops to 1 s. A burst of 30 absorbs a
  handful of sign-ins and a connection's first frames; one per second is slow enough that an unbounded plugin costs the
  daemon a call a second, and fast enough for presence.
- **`data::update` is locked by the host.** WASI preview 2 has no file locks and calls of one plugin overlap, so a
  read-modify-write loses updates. The host keeps named locks per plugin (`data.lock` / `data.unlock`), released with the
  call's store however the call ends, with a bounded wait that ends `busy` and never outlasts the call. The SDK's
  `update` takes the lock around load, change and the existing write-aside-and-rename save.
- **`settings.set`.** A plugin writes its own declared settings through the same `PluginSettings::set` the user's write
  uses, so validation and the owner-only file are one code path, and the write shares the lock with the user's. The
  clients are told through a `changed` event for the plugin; the plugin's own `on-settings-changed` is not run for its own
  write, so it cannot loop.

Rejected:

- **A long-lived instance.** It would let a plugin hold a socket and react in-process, but it brings back everything
  per-call instances avoid: guest memory that lives for the daemon's lifetime, a trap that loses state instead of one
  call, a plugin that can spin outside any deadline, and a second execution model for every export. The host holds the
  connection and the plugin is woken only when there is something to do.
- **Keeping the 30 s floor and adding events.** Events would then be the only fast path, and a plugin with a real timer
  need (a countdown, a heartbeat) would still be refused for no reason the budget does not already cover.
- **A per-contract id function on `Contract`.** The ids a call names are the ids its handler resolved; extracting them
  again from `Params` and `Result` per contract repeats the lookup and cannot see a display name resolved to an id.
- **Level-triggered readable events.** Firing for as long as bytes are buffered would hit the budget at once and starve
  every other event; one event per arrival edge, re-armed by `receive`, is enough.
- **A file lock in the guest.** WASI preview 2 offers none, and a lock file races without one.
