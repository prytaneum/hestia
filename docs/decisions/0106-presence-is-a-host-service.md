# Presence is a host service: plugins state a card, the host owns the connection

*Applies to: [The engine](../architecture/engine.md), [The daemon](../architecture/daemon.md), [plugin permissions](../plugins/permissions.md)*

Rich Presence talks to Discord's local IPC socket (a named pipe on Windows), and a plugin has no sockets
([0101](0101-plugins-spawn-only-declared-programs.md) keeps them closed). Presence moves to a plugin eventually
(12.16), so the sandbox needs a way to say what Discord should show without being given the socket.

**The host owns the connection; a plugin hands it a card.** The `presence` WIT import has two calls,
`set(activity)` and `clear()`, gated by the sandboxed permission `presence:set` ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md))
because all it can do is write a status card. No generic local-socket capability exists, and the plugin never sees
Discord's protocol, its application id or its handshake.

**The service lives in the engine** (`engine::presence`), not under `plugins/`: the launcher's own card uses it too,
so it is not plugin-specific. `plugins/presence.rs` is only the permission gate and the plugin's handle on it. The
`discord-rich-presence` dependency moved from `daemon` to `engine` with the connection. The decision loop that picks
the core card (what is being played) stays in the daemon until the plugin exists; it now calls `Presence::set_core`
and `Presence::tick`.

**Everything [0059](0059-discord-presence-is-a-daemon-loop.md) decided still holds**: the dedicated blocking thread,
the 5 s tick that diffs against what was last published, the latched failed connect, the poll at a sixth of the rate
when Discord is absent, `discord.enabled` and `HESTIA_NO_PRESENCE`. A plugin's `set` only records the card, under a
lock that is never held across Discord I/O, so a host call on an async worker cannot wait on the socket; the card
reaches Discord on the next tick.

**Rate limit.** Discord allows 5 activity updates per 20 s. The link keeps the send times and defers a publish past
the limit to the next tick rather than dropping it. At a 5 s tick the loop cannot reach 5 in 20 s on its own, so this
is a guard for a shorter tick, not a behaviour users see. A clear is not counted.

**Arbitration.** The service holds the core card and a list of plugin cards, one per plugin. A plugin card, while
any is set, wins over the core card, and the newest writer wins among plugins. `clear` removes only the caller's own
card, so another plugin's card is shown again and, with none left, the core card is. When the last handle on a
plugin's loaded instance drops (disabled, removed, unloaded) or repeated traps disable it, its card is cleared, so a
stopped plugin never leaves a card behind. A reload that overlaps the old instance's drop clears the card once; the
plugin sets it again on its next event. `discord.enabled` off shows nothing, whoever asks.

**The card is validated at the edge** against Discord's documented limits, by `proto::presence::Activity::checked`:
details, state and image texts are trimmed and 2 to 128 characters (empty is omitted), at most 2 buttons, a label of
1 to 32 characters, an `https` URL of at most 512, an image key of at most 256. Over a limit is an `invalid` error to
the plugin, never a silent truncation. The launcher's own card is built by the daemon and not checked: an instance
name is the user's, and rejecting it would blank the card.

Rejected:

- **Opening `wasi:sockets` for a plugin.** A grant could not say what it allows, and the Discord peer is a process
  Hestia does not control.
- **Letting the plugin publish directly** (the host call blocks until Discord answers). It would hold an async worker
  on a socket write and let a plugin outrun the rate limit.
- **Putting the service in `plugins/`.** The core card would then reach the engine through the plugin layer.
- **Per-plugin connections.** Discord shows one activity per application id, so two connections would overwrite each
  other.
