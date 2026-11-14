# An ambient channel hands out harmless facts and needs no permission

*Applies to: [Plugins](../architecture/plugins.md#permissions), [plugin permissions](../plugins/permissions.md)*

[0092](0092-a-contract-names-the-permission-a-plugin-needs.md) closed every channel that names no permission, and parked
`app.info`, `host.info`, `health.ping` and `net.status` there with "no permission fits". A plugin that needs to know the
operating system, the architecture or whether the network is up then had two bad choices: ask the user for an unrelated
permission, or probe the machine itself (a port ran `-XshowSettings` to learn `${pathsep}`).

**A contract may be ambient.** `Contract::AMBIENT: bool` (default `false`) marks a channel any plugin may call with no
permission. It is a flag beside `PLUGIN`, not a variant that replaces `Option<Permission>`: the type is read at about
130 sites that would all change for one tier, and the two are exclusive by construction, which the router asserts when a
handler registers.

**Ambient is not a permission.** It is not in `Permission`, `Permission::ALL` or `Permission::spec`, so it is in no
`Grant`, no approval digest, no `widening_over` and no install prompt, and no grant can widen it. The plugin gate asks
`HostCalls::ambient` only after finding the channel names no permission, then forwards it with the same decoding as any
call; a job channel is never ambient, so `host.run-job` still refuses it.

**What qualifies.** The channel's *whole* result is harmless facts about the app or the machine: nothing under the
user's home, no account identity, no entry, no config value, no token, and no path. Three channels:

| Channel | Why it is harmless |
|---|---|
| `app.info` | name, version, id, vendor and release channel of this build |
| `host.info` | operating system and architecture (the vocabulary a flavor's `Target` already carries), classpath separator and installed memory |
| `net.status` | whether upstream is reachable and since when |

Left out: `health.ping`, which answers the daemon's pid and is denied; no plugin is handed a pid. Also `daemon.status` (the data home and log paths), `plugin.*`, `account.*`, `cache.info` and every list of entries.
The `net.state` topic stays behind `network:connect`: a topic is admitted through `Permission`, and the polled
`net.status` carries the same readout.

**`host.info` grows.** It answers `os`, `arch` and `pathSeparator` beside `totalMemoryBytes`, filled from the same
host facts a flavor query carries so the two cannot disagree. `pathSeparator` is what joins a classpath or `PATH`
(`:` or `;`), the meaning it has in `Target`; a file separator follows from `os`. The SDK wraps it as `hestia::host::info()`.

**The test keeps it deliberate.** `crates/proto/tests/permission.rs` still fails a contract that is neither
permissioned, ambient nor on `DENIED`, and pins the ambient channels as a list, so widening the tier is a visible edit.
The `plugin_api` snapshot records an ambient channel with `permission: "ambient"`.

Rejected:

- **A `Permission::Ambient` every grant holds implicitly.** It would sit in the digest and the prompt, and every
  consumer of the permission table would need a case for the one permission nobody declares.
- **Replacing `Option<Permission>` with an enum.** Correct but a change at every contract for no behaviour.
- **Leaving the facts to `daemon.status`.** It carries the data home and the log path.
