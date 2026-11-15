# Permissions

A plugin reaches nothing it was not granted. The manifest's `[permissions]` table declares each permission with the
scope it applies to; the user approves that grant at install, and an update that asks for more (a permission, or one
more scope entry) asks again.

A daemon channel a plugin may call names the permission it needs; a channel that names none is closed to plugins. The
few **ambient** channels that hand out only harmless facts (`app.info`, `host.info`, `net.status`) need no permission
and never appear in an approval prompt.

An **unrestricted** permission lets a plugin act on the computer outside the sandbox, so the install states it as full
access to the computer.

| Permission | Tier | Scope | Allows |
|---|---|---|---|
| `entry:read` | sandboxed | `files` | See servers and instances and their settings; read the listed server files |
| `entry:write` | unrestricted | none | Create, change and delete servers and instances, including files and JVM arguments |
| `process:read` | sandboxed | none | See which servers and games are running |
| `process:control` | unrestricted | none | Start and stop servers and games, and run console commands |
| `process:spawn` | unrestricted | `programs` (required) | Run the declared programs |
| `content:read` | sandboxed | none | Search and inspect mods, packs and other content |
| `content:write` | unrestricted | none | Install, update and remove content |
| `account:read` | sandboxed | none | See the names of signed-in accounts, never tokens |
| `config:read` | sandboxed | none | See the launcher settings that are safe to share |
| `config:write` | unrestricted | none | Change the launcher settings that are safe to hand over |
| `java:read` | sandboxed | none | See installed and available Java runtimes |
| `java:manage` | sandboxed | none | Install and remove Java runtimes |
| `network:connect` | sandboxed | `hosts` (required), `unhashed` | Connect to the listed hosts |
| `ui:css` | sandboxed | none | Restyle the app with a stylesheet |
| `launch:patch` | sandboxed | `properties` | Adjust launch settings from an allowlist; a wrapper program also needs `process:spawn` |
| `ipc:connect` | unrestricted | `endpoints` (required) | Talk to the listed local programs |
| `flavor:provide` | unrestricted | none | Add game distributions, which choose the code the game runs |
| `java:provide` | unrestricted | none | Add Java distributions, which choose the runtime Hestia executes |
| `pack:provide` | unrestricted | none | Add pack formats, which choose the version, loader and files of imported instances |

A permission takes only the scope kinds in its row, and a required scope may not be empty. Entry rules for each scope
kind are in [manifest.md](manifest.md#permissions). A permission written as `"content:read" = {}` is held with no
narrowing. Hook points and provider kinds name the permissions they need; a manifest that declares one without them is
refused at `pack` and at install.

## Network

`network:connect` reaches only its `hosts`, over https. `unhashed` lists the hosts (each also in `hosts`) a flavor's
profile may serve a file from without a checksum; the user sees the list at approval.

## Entry files

`entry:read` does not expose a server's directory. `server.files.read` serves a plugin only the paths in the `files`
of its grant, matched by exact string, so `./ops.json` matches nothing. `server.properties` can never be listed because
it holds the RCON password. The scope names files of any server, not which servers.

## Local endpoints

`ipc:connect` reaches only the endpoints it lists. An entry is a relative name of letters, digits, `.`, `_` and `-`,
with `/` between segments; a trailing `*` matches any of those characters (`discord-ipc-*`); a bare `*` is refused. On
Linux and macOS the host looks for a Unix socket of that name under `$XDG_RUNTIME_DIR`, `$TMPDIR`, `$TMP`, `$TEMP` and
`/tmp`, in that order. On Windows the name is a named pipe, `\\.\pipe\<name>`, and a name containing `/` never matches.
A local service can do whatever it was built to do, which is why the permission is unrestricted.

## Programs

`process:spawn` runs only the programs the manifest declares and names in `programs`, with argument patterns and
environment names it declared, and no shell. A `PATH` program is resolved at approval to an absolute path and the hash of
that file, both shown to the user and covered by the approval; it runs from that file and no other. A program not
found at approval cannot run until the plugin is approved again with it installed. A `java` program is shown as
"Hestia-managed Java 17-21" and is not pinned. An update that changes a program's definition, pinned path or hash is
held for approval, and an unattended update never applies one. How to run them is in [api.md](api.md#programs).

## Launcher settings

A plugin reaches a launcher setting only as far as `proto::config::PLUGIN_ACCESS` allows (`none`, `read` or
`readwrite` per key; a missing key is `none`). `config.get` of a closed key answers permission-denied, and
`config.list` returns only readable keys. `config.set` needs `config:write` and a `readwrite` key. No key is
`readwrite` yet.
