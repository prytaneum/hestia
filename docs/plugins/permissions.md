# Plugin permissions

A plugin reaches nothing it was not granted. Its manifest declares the permissions it needs and the narrower lists they
apply to; the user approves that grant at install, and an update that asks for more asks again. A daemon channel a
plugin may call names the permission it needs on its contract; a channel that names none is closed to plugins
([0092](../decisions/0092-a-contract-names-the-permission-a-plugin-needs.md)).

## Permissions

An **unrestricted** permission lets a plugin act on the computer outside the sandbox, so the install states it as full
access to the computer.

| Permission | Tier | Allows |
|---|---|---|
| `entry:read` | sandboxed | See your servers and instances and their settings |
| `entry:write` | unrestricted | Create, change and delete servers and instances, including their files |
| `process:read` | sandboxed | See which servers and games are running |
| `process:control` | unrestricted | Start and stop servers and games, and run console commands |
| `process:spawn` | unrestricted | Run the programs it lists (below) |
| `content:read` | sandboxed | Search and inspect mods, packs and other content |
| `content:write` | unrestricted | Install, update and remove mods, packs and other content |
| `account:read` | sandboxed | See the names of signed-in accounts, never their tokens |
| `config:read` | sandboxed | Read launcher settings |
| `config:write` | unrestricted | Change launcher settings, including default JVM arguments |
| `java:read` | sandboxed | See installed and available Java runtimes |
| `java:manage` | sandboxed | Install and remove Java runtimes |
| `network:connect` | sandboxed | Connect to the hosts it lists in `network` |
| `ui:css` | sandboxed | Restyle the app with its own stylesheet ([0100](../decisions/0100-themes-are-plugin-css.md)) |
| `launch:patch` | sandboxed | Adjust launch settings from an allowlist when a game or server starts |
| `ipc:connect` | unrestricted | Talk to the programs running on this computer that it lists in `ipc` (below) |
| `flavor:provide` | unrestricted | Add game distributions that choose what code the game runs, including its main class and arguments |
| `format:provide` | unrestricted | Add archive formats that choose the game version, loader and JVM settings of instances you import |
| `java:provide` | unrestricted | Add Java distributions that choose the runtime binary Hestia executes |

Hook points and provider kinds name the permissions they need in `proto::hook` and `proto::provider`; a manifest that
declares one without them is refused at `pack` and at install.

## Programs

`process:spawn` runs only the programs the manifest declares
([0101](../decisions/0101-plugins-spawn-only-declared-programs.md)):

```toml
permissions = ["process:spawn"]

[[programs]]
id = "tailscale"
command = "tailscale"            # a name on PATH, pinned to its absolute path at approval
args = ["status", "--json"]      # literal tokens, `*` for one token, a trailing `**` for the rest
env = []

[[programs]]
id = "agent"
command = "bin/playit"           # a file inside data/, run from a verified host-owned copy
args = ["**"]
env = ["PLAYIT_SECRET"]          # the only names a call may pass
[programs.sha256]
x86_64-unknown-linux-gnu = "…"   # one per target triple the plugin supports
x86_64-pc-windows-msvc = "…"
```

- The SDK's `hestia::process` runs them. `exec` runs a program to completion, with output cut at 1 MiB per stream. The
  other calls handle long-lived agents: `spawn`, `stop`, `status`, `logs` and `list`.
- A call naming another program, arguments outside the patterns, or an undeclared environment name is refused with
  `permission-denied`.
- There is no shell, the working directory is `data/`, and the environment starts empty.
- A spawned agent is supervised with the owner `plugin:<id>`. Only its plugin can see or stop it, `list()` finds it again
  after a restart, and disabling or removing the plugin stops it.
- `env` may not name `LD_*`, `DYLD_*`, `PATH` or a `*JAVA*OPTIONS` variable, and a shell cannot be declared as a program.

The [`tunnel-demo`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/tunnel-demo) example plugin starts a stand-in agent when a server starts and stops it when the server
exits.

## Local endpoints

`ipc:connect` reaches only the local endpoints the manifest lists
([0106](../decisions/0106-the-plugin-api-is-mechanisms-not-features.md)):

```toml
permissions = ["ipc:connect"]
ipc = ["discord-ipc-*", "app/com.discordapp.Discord/discord-ipc-*"]
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
