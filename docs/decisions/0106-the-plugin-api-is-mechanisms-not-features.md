# The plugin API is mechanisms, not features

*Applies to: [Plugins](../architecture/plugins.md), [plugin permissions](../plugins/permissions.md)*

The plugin system exists so users can make Hestia do what they want. It had started to grow the other way: each
feature planned to move into an official plugin got a surface of its own: a `presence` import for Discord, an
`archive-format` export for import/export, a `java-provider` export for Adoptium, three WIT exports for three hooks.
Every one was a new WIT interface, a new permission, a new `Plugin` trait method and a new decision, designed backwards
from the one feature that needed it. A user's idea that matched none of them had nowhere to go, and the official
plugins would have used an API nobody else could have designed.

**The rule: a plugin surface describes the host, never a feature.** The ABI is a fixed set of general mechanisms, and
everything that varies is data in `proto`:

| Mechanism | ABI | What varies, as data |
|---|---|---|
| Call the daemon | `host.call`, `host.run-job` | contracts, each naming its permission ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)) |
| Hear what happens | `events.on-event` | topics, all classified in `proto::events::TOPICS` |
| Adjust or refuse an operation | `hooks.intercept(point, context)` | hook points in `proto::hook` |
| Fill an open registry | `provider.invoke(kind, id, op, args)` | provider kinds and operations in `proto::provider` |
| Own commands, pages, settings, themes | `commands`, `ui`, `settings`, `settings-changed` | the manifest's contributions |
| Reach outside the sandbox | `wasi:http`, `process`, `ipc` | the grant: hosts, programs, local endpoints |

Adding a hook point, a provider kind or an operation is a type in `proto` plus the host code that acts on it; the WIT,
the SDK's `Plugin` trait and the daemon do not change. `tests/plugin_api.rs` snapshots every point and operation with
the contracts, so a change is still a reviewed diff.

**The test for any new surface, and for every port of a core feature:** could a third party have written this plugin
without changing the host? If not, the missing piece is a general mechanism, added as one, never a capability shaped
like the feature. Official plugins hold no API privilege; a bare provider id is a namespace, not a capability
([0099](0099-official-plugins-are-signed.md)).

**Discord presence is the worked example.** It needs a connection to Discord's local socket that outlives one call. The
general form is `ipc:connect`: a manifest lists the local endpoints it may reach (`ipc = ["discord-ipc-*"]`, a relative
name under the user's runtime and temp directories, or a named pipe on Windows), the user approves that list, and the
host dials and keeps each connection open across calls until the plugin closes it or stops. The plugin speaks Discord's
protocol and decides what to show from process events. Nothing in the host knows Discord exists. The same mechanism
serves any local program with a socket or a pipe. Reaching a local service can be reaching anything that service can
do (a rootless Docker socket sits in the runtime directory), so the permission is unrestricted, and a bare `*` is
refused so a grant always names what it allows. Until the presence plugin ships, the core keeps its own loop
([0059](0059-discord-presence-is-a-daemon-loop.md)).

**Optional operations fall back in the host.** A guest answers `unsupported` for an operation it does not implement,
and the adapter applies the operation's documented fallback (`projects` asks `project` for each, `identify` finds
nothing), so the SDK carries no defaults of its own and a guest in another language behaves the same.

**The SDK routes instead of growing.** `Plugin::intercept` and `Plugin::provide` take a request and route it with
`.on::<Point>(…)` / `.on::<Op>(…)`, typed by the same `proto` definitions the host uses.

Rejected:

- **Keeping one WIT interface per surface.** Each was clear on its own, but the set only grew with what the core team
  wanted to move out, and every addition broke the ABI.
- **Host services for features (`presence.set`).** A channel that only Discord presence uses is the same bespoke
  surface behind a contract.
- **Opening `wasi:sockets` to plugins.** A grant could not say what it allows; named endpoints can.
- **A generic local-socket call per request, with no kept connection.** Each call runs on a fresh instance, and a peer
  such as Discord drops what a client set when its connection closes.
