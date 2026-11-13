# Plugin permissions

A plugin reaches nothing it was not granted. Its manifest's `[permissions]` table declares the permissions it needs, each
with the scope it applies to; the user approves that grant, which is `permission -> scope`, at install, and an update that
asks for more (a permission, or one more entry in a scope) asks again
([0117](../decisions/0117-a-grant-is-permission-to-scope.md)). A daemon channel a
plugin may call names the permission it needs on its contract; a channel that names none is closed to plugins
([0092](../decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)).

## Permissions

An **unrestricted** permission lets a plugin act on the computer outside the sandbox, so the install states it as full
access to the computer.

| Permission | Tier | Scope | Allows |
|---|---|---|---|
| `entry:read` | sandboxed | `files` | See your servers and instances and their settings |
| `entry:write` | unrestricted | none | Create, change and delete servers and instances, including their files and JVM arguments |
| `process:read` | sandboxed | none | See which servers and games are running |
| `process:control` | unrestricted | none | Start and stop servers and games, and run console commands |
| `process:spawn` | unrestricted | `programs` (required) | Run the programs it names (below) |
| `content:read` | sandboxed | none | Search and inspect mods, packs and other content |
| `content:write` | unrestricted | none | Install, update and remove mods, packs and other content |
| `account:read` | sandboxed | none | See the names of signed-in accounts, never their tokens |
| `config:read` | sandboxed | none | See the few launcher settings that are safe to share, never keys, locations or update settings |
| `config:write` | unrestricted | none | Change the few launcher settings that are safe to hand over; everything that decides where data lives, what runs, what updates and what is downloaded stays yours |
| `java:read` | sandboxed | none | See installed and available Java runtimes |
| `java:manage` | sandboxed | none | Install and remove Java runtimes |
| `network:connect` | sandboxed | `hosts` (required), `unhashed` | Connect to the hosts it lists |
| `ui:css` | sandboxed | none | Restyle the app with its own stylesheet ([0100](../decisions/0100-themes-are-plugin-css.md)) |
| `launch:patch` | sandboxed | `properties` | Adjust launch settings from an allowlist when a game or server starts; a wrapper program also needs `process:spawn` |
| `ipc:connect` | unrestricted | `endpoints` (required) | Talk to the programs running on this computer that it lists (below) |
| `flavor:provide` | unrestricted | none | Add game distributions that choose what code the game runs, including its main class and arguments |
| `format:provide` | unrestricted | none | Add archive formats that choose the game version, loader and JVM settings of instances you import |
| `java:provide` | unrestricted | none | Add Java distributions that choose the runtime binary Hestia executes |

A permission takes only the scope kinds in its row, and a scope marked required may not be empty; the manifest is
refused otherwise (`ScopeNotAccepted`, `ScopeRequired`). The table is `Permission::spec`; the entry rules are in
[manifest.md](manifest.md#access). A permission written as `"content:read" = {}` is held with no narrowing.

## Settings

A plugin reaches a launcher setting only as far as `proto::config::PLUGIN_ACCESS` allows: each key is `none`, `read` or
`readwrite` for plugins, and a key missing from the table is `none`. `config.get` of a closed key answers
permission-denied; `config.list`, or a `config.get` of a parent such as `defaults`, returns only the readable keys.
`config.set` needs `config:write` and a `readwrite` key; setting a parent needs every key under it to be
`readwrite`. Keys match in canonical form, so `defaults.jvmArgs` is `defaults.jvm-args`. Nothing is `readwrite` yet.
The table is checked against the settings schema by a test in `crates/engine/src/config.rs`.

Hook points and provider kinds name the permissions they need in `proto::hook` and `proto::provider`; a manifest that
declares one without them is refused at `pack` and at install.

## Network

`network:connect` reaches only its `hosts`: a domain name, or `*.domain` for the names one label below it. Its `unhashed`
lists the hosts (each also in `hosts`) a flavor's profile may serve a file from without a checksum
([0114](../decisions/0114-an-unhashed-download-is-declared.md)).

```toml
[permissions]
"network:connect" = { hosts = ["api.adoptium.net", "meta.fabricmc.net"], unhashed = ["meta.fabricmc.net"] }
```

## Launch properties

`launch:patch` may set only the `-D` keys in its `properties`; the allowlist for the other launch arguments is fixed
([manifest.md](manifest.md#launch-properties)).

```toml
[permissions]
"launch:patch" = { properties = ["my.property"] }
```

## Programs

`process:spawn` runs only the programs the manifest defines and names in its `programs`
([0101](../decisions/0101-plugins-spawn-only-declared-programs.md)):

```toml
[permissions]
"process:spawn" = { programs = ["tailscale", "agent", "tool"] }

[[programs]]
id = "tailscale"
command = "tailscale"            # a name on PATH, pinned to its absolute path and hash at approval
args = ["status", "--json"]      # literal tokens, `*` for one token, a trailing `**` for the rest
requires = { name = "Tailscale", url = "https://tailscale.com/download" }

[[programs]]
id = "agent"
command = "bin/playit"           # a file inside data/, run from a verified host-owned copy
args = ["**"]
env = ["PLAYIT_SECRET"]          # the only names a call may pass
[programs.sha256]
x86_64-unknown-linux-gnu = "..."   # one per target triple the plugin supports
x86_64-pc-windows-msvc = "..."

[[programs]]
id = "tool"
java = { min = 17, max = 21 }    # managed Java; the call picks the major inside the range

[[programs.invocations]]
name = "run"
args = ["-jar", "*"]
```

- A `java` program is shown in the grant as "Hestia-managed Java 17-21". The runtime is host-owned, so nothing is pinned
  at approval. The grant entry is the program's whole definition on one line, identity first and then each invocation,
  so any change to either is a new entry and asks for approval again.
- A `PATH` program is pinned to a path and the hash of the file there, shown at install. One not found at install is
  shown as not found and cannot run until the plugin is approved again with it installed
  ([processes.md](processes.md#path-programs-are-pinned)).
- The SDK's `hestia::process` runs them. `exec` runs a program to completion, with output cut at 1 MiB per stream. The
  other calls handle long-lived agents: `spawn`, `stop`, `status`, `logs` and `list`.
- A call naming another program, one defined but not named in `programs`, arguments no invocation matches, or an
  environment name the matching invocation does not declare is refused with `denied`. The grant holds each named
  program's whole definition, so a changed identity, pattern, environment name or hash asks for approval again.
- There is no shell, the working directory is `data/` unless `exec_in` names another (`/data`, or `/build` during a
  flavor's `build`; anything else is `denied`), and the environment starts empty.
- A spawned agent is supervised with the owner `plugin:<id>`. Only its plugin can see or stop it, `list()` finds it again
  after a restart, and disabling or removing the plugin stops it.
- `env` may not name `LD_*`, `DYLD_*`, `PATH` or a `*JAVA*OPTIONS` variable, and a shell cannot be declared as a program.

The [`tunnel-demo`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/tunnel-demo) example plugin starts a stand-in agent when a server starts and stops it when the server
exits.

## Entry files

`entry:read` does not let a plugin read the files of a server. `server.files.read` serves a plugin only the paths in the
`files` of its `entry:read`, matched exactly, and the list is part of the grant: the install prompt shows it and an update
that adds a path asks again. The channel's contract names that narrowing, so the plugin gate applies it and no handler
checks it.

```toml
[permissions]
"entry:read" = { files = ["whitelist.json", "ops.json", "banned-players.json"] }
```

- A path is relative to the server's directory and already normalised: no leading `/`, no `\`, no `.` or `..`
  segment, no glob. `./ops.json` is refused at install and, in a call, matches nothing.
- `server.properties` can never be listed, because it holds the RCON password.
- A client other than a plugin reads any file, as before ([0092](../decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)).

## Local endpoints

`ipc:connect` reaches only the local endpoints in its `endpoints`
([0106](../decisions/0106-the-plugin-api-is-mechanisms-not-features.md)):

```toml
[permissions]
"ipc:connect" = { endpoints = ["discord-ipc-*", "app/com.discordapp.Discord/discord-ipc-*"] }
```

- An entry is a relative name of letters, digits, `.`, `_` and `-`, with `/` between segments. A trailing `*` matches
  any of those characters (`discord-ipc-*` matches `discord-ipc-0`); a bare `*` is refused.
- On Linux and macOS the host looks for a Unix socket by that name under `$XDG_RUNTIME_DIR`, `$TMPDIR`, `$TMP`, `$TEMP`
  and `/tmp`, in that order. On Windows the name is a named pipe, `\\.\pipe\<name>`, and a name with `/` never matches.
- The SDK's `hestia::ipc` opens a connection and sends and receives bytes; the plugin speaks the peer's protocol
  itself. The host keeps a connection open across calls (each call runs on a fresh instance), so the plugin names it
  again by its id. At most 8 are open at once, a message is at most 1 MiB, and every connection closes when the plugin
  is disabled, removed or reloaded.
- A local service can do whatever it was built to do (a rootless Docker socket lives in the runtime directory), which
  is why the permission is unrestricted and the install shows every endpoint.
