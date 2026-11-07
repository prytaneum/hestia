# Discord presence belongs to the daemon, and is a loop rather than a hook

*Applies to: [The daemon](../architecture/daemon.md)*

Rich Presence is a thing a launcher shows about the user, so the obvious home is
the front-end the user is looking at — the Tauri shell, where the equivalent
feature lives in Modrinth's launcher. It is the wrong home here, and for a
reason specific to hestia's shape: the desktop is one of several front-ends, and
the one least likely to be open. `hestia play` from a terminal would publish
nothing, and closing the window mid-game would clear a presence for a session
still running. Modrinth's placement is in *their* core library, not their Tauri
crate; hestia's core is behind a socket, so the equivalent placement is the
daemon.

Everything the card needs is already there. The supervisor knows which sessions
are live, `instance_sessions` already maps a session key back to its entry, and
`config.*` already carries a setting to every front-end — so `discord.enabled`
is a CLI command and a desktop toggle without a channel of its own. Discord's
endpoint is a local socket owned by the user's session, which is where `hestiad`
already runs; a daemon promoted to a system service would lose reach to it, and
that is the one assumption this rests on.

**The publisher is a tick loop, not hooks on launch and exit.** Hooks are the
smaller diff and the wrong mechanism: they describe transitions, and three of
the four things that change what Discord should be showing are not transitions
hestia observes. Discord can start after the daemon. A restart re-adopts
sessions that were never launched by this process. A dead socket is discovered
on a write, not announced. A loop that recomputes the card and sends only when
it differs answers all four with one mechanism, and gets rate-limit safety for
free — Discord caps activity updates per connection, and an unchanged tick sends
nothing.

Consequences worth writing down:

- **It runs on its own thread, not a tokio task.** Every call into the Discord
  client is a blocking socket write whose peer is a process hestia does not
  control. A worker thread is not the place to wait on that.
- **A failed connect must latch.** The client panics if asked to set an activity
  having never connected, and `hestiad` is built `panic = "abort"` — so the
  connection state gates every later call rather than being logged and stepped
  past.
- **Idle is a state, not the absence of one.** The daemon is resident, so
  presence says the launcher is open even when nothing is running. That is a
  deliberate choice and the reason the setting exists: it is also the one thing
  presence reveals that the user did not do on purpose.
- **Automated tests opt out** with `HESTIA_NO_PRESENCE=1`, as they already do for
  the tray. Discord's handshake answers one connecting client at a time, so a
  test run standing several throwaway daemons up at once leaves the rest of them
  blocked in that read — with nothing worth publishing either way.
- **The connection has since moved into the engine** as a host service plugins may also write to; the daemon keeps
  only this decision loop ([0106](0106-presence-is-a-host-service.md)).
- **No Discord client is the ordinary case**, not a fault. Its absence is polled
  for at a sixth of the tick rate, and the crate's per-attempt warning is held
  below the foreign-log threshold, or a daemon on a machine that never runs
  Discord writes a warning every five seconds forever.
