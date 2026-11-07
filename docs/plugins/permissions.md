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
