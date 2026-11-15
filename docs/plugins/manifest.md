# The manifest

`plugin.toml` sits at the root of the plugin's folder and, packed, at the root of the `.hplugin`. The types are
`proto::plugin::Manifest` and its parts (`crates/proto/src/plugin/`). `hestia plugin check`, `pack` and `dev` run the
same validation as install and report every mistake at once. Keys are camelCase (`helpUrl`, `refreshEvery`); an unknown
key is ignored.

```toml
id = "tunnel-demo"
name = "Tunnel demo"
version = "0.1.0"
wasm = "plugin.wasm"
description = "Runs a stand-in tunnel agent while a server is up, to show the process:spawn model."

[permissions]
"process:read" = {}
"process:spawn" = { programs = ["agent"] }

[[programs]]
id = "agent"
command = "sleep"
args = ["*"]

[contributes]
events = ["process.started", "process.exit"]
```

Source: `plugins/tunnel-demo/plugin.toml` in the examples repository.

## Identity

| Key | Required | Rule |
|---|---|---|
| `id` | yes | 2 to 40 lowercase letters, digits or dashes, no leading or trailing dash; also the directory name. Not a top-level CLI verb (`account`, `config`, `server`, `instance`, `plugin`, ...) |
| `name` | yes | display name |
| `version` | yes | `major.minor.patch`; a pre-release or build suffix is allowed and ignored when ordering |
| `description`, `authors`, `homepage` | no | |
| `wasm` | with a component | package-relative path of the component |
| `api` | with a component | caret requirement on the host ABI, `^N.M.P`. `pack` writes it from the `hestia-sdk` version cargo resolved; do not type it |

The host's `API_VERSION` must satisfy `api`, or install is refused. A theme-only plugin has neither `wasm` nor `api`.
Every package path (`wasm`, a theme `file`, a page icon) must be relative and stay inside the package: no leading `/`,
no `\`, no `:`, no `.` or `..` segment.

## Permissions

`[permissions]` maps a permission name to its **scope**: an inline table with an entry list per scope kind, or `{}` for
none (`true` and bare lists are refused). Which permissions exist and which scope kinds each takes is in
[permissions.md](permissions.md).

```toml
[permissions]
"entry:read" = { files = ["whitelist.json", "ops.json"] }
"network:connect" = { hosts = ["api.adoptium.net"], unhashed = ["meta.fabricmc.net"] }
"process:spawn" = { programs = ["git"] }
"ipc:connect" = { endpoints = ["discord-ipc-*"] }
"launch:patch" = { properties = ["my.property"] }
```

| Scope kind | Taken by | Entries |
|---|---|---|
| `hosts` | `network:connect` (required) | a domain, or `*.domain` for names one label below it (`*.example.com` reaches `api.example.com`, not `a.b.example.com`). An address is refused, and so is a wildcard over a public suffix such as `*.co.uk` |
| `unhashed` | `network:connect` | hosts a flavor may serve a download from without a checksum; each must also be in `hosts` |
| `files` | `entry:read` | exact relative paths in a server's directory, such as `whitelist.json` or `config/a.json`: no leading `/`, no `\`, no `:`, no `.` or `..` segment, no glob. `server.properties` is always refused |
| `endpoints` | `ipc:connect` (required) | local endpoint names such as `discord-ipc-*` |
| `programs` | `process:spawn` (required) | ids of the plugin's own `[[programs]]` |
| `properties` | `launch:patch` | `-D` system property keys a launch hook may set: 1 to 100 letters, digits, `.`, `_`, `-`. Keys that load code or weaken the JVM (`java.library.path`, `java.security.*`, `jdk.*`, `sun.*`, `log4j*`, anything containing `agent`, `classpath`, `class.path` or `module`) are refused |

Together with the declared hooks, events, schedules, themes and secret settings, the permissions form the grant the user
approves. An update that adds a permission or any scope entry asks again.

## `[[programs]]`

A program is an identity (what runs) and one or more invocations (how it may be called). Running them is in
[api.md](api.md#programs).

| Key | Meaning |
|---|---|
| `id` | unique slug; what the SDK's `process` calls name and `process:spawn` lists |
| `command` | identity: a name on `PATH` (1 to 64 of letters, digits, `.`, `_`, `-`, `+`, not starting with `.` or `-`), or, with a `/`, a file inside `data/` |
| `java` | identity, instead of `command`: a Hestia-managed Java. An integer is that major; `{ min, max }` (1 to 99) lets the call pick a major in the range; `"profile"` is the Java of the game version, valid only in a flavor's `build`. Installed when missing, never pinned; no `sha256` |
| `sha256` | table of target triple to hex digest; required for a `data/` program |
| `requires` | `{ name, url }`: what to tell the user when a `PATH` program is missing; `url` is https |
| `args` | the one invocation, at the top level: literal tokens, `*` for one token, a trailing `**` for the rest |
| `env` | environment names that invocation may be passed. `LD_*`, `DYLD_*`, `PATH` and `*JAVA*OPTIONS` are refused |
| `path` | ids of other `PATH` programs whose pinned directories become that invocation's `PATH` |
| `[[programs.invocations]]` | several invocations instead of the top-level keys: each has a unique slug `name` and its own `args`, `env`, `path`; at most 16 |

Exactly one identity per program. A shell (`sh`, `bash`, `zsh`, `fish`, `cmd`, `powershell`, `pwsh`, ...) is refused as a
command.

```toml
[[programs]]
id = "git"
command = "git"
args = ["**"]
requires = { name = "Git", url = "https://git-scm.com/downloads" }

[[programs]]
id = "agent"
command = "bin/playit"           # a file inside data/
args = ["**"]
env = ["PLAYIT_SECRET"]
[programs.sha256]
x86_64-unknown-linux-gnu = "..."   # one per target triple the plugin supports
x86_64-pc-windows-msvc = "..."

[[programs]]
id = "buildtools"
java = { min = 17, max = 21 }

[[programs.invocations]]
name = "build"
args = ["-jar", "${data}/BuildTools.jar", "--rev", "*", "--output-dir", "${build}"]
```

Patterns match the placeholders `${data}`, `${build}` and `${vanilla}` as written; the host substitutes the mount path
afterwards.

## `[[settings]]`

```toml
[[settings]]
key = "client-id"
kind = "string"
label = "Discord application id"
help = "The id of a Discord application you created; its assets name the images."
```

Source: `plugins/discord-presence/plugin.toml`.

| Key | Meaning |
|---|---|
| `key` | unique slug; what `hestia::settings::get` reads |
| `kind` | `string` (default), `number`, `bool`, `select` or `secret`. The daemon checks every stored value against it; a string is at most 4096 bytes |
| `default` | used while nothing is set; a `secret` has none |
| `label`, `help`, `placeholder`, `helpUrl` | text shown in the app and `hestia plugin config`; `helpUrl` is https |
| `options` | the choices of a `select`; not empty |
| `min`, `max` | bounds of a `number` |
| `pattern` | a regular expression (at most 256 bytes) the whole `string` or `secret` must match |
| `group` | heading the setting is listed under; ungrouped settings come first |

A constraint on the wrong kind, `min` above `max`, a pattern that does not compile or a default that fails its own
constraints is refused. A value resolves to what the user stored, else the default the distributor packed
([publishing.md](publishing.md#setting-defaults)), else `default`. A `secret` is listed only as set or unset and leaves
the daemon only for this plugin. Reading and writing settings is in [api.md](api.md#settings).

## `[contributes]`

Every contribution id is 1 to 40 lowercase letters, digits or dashes, unique within its list.

| Key | Type | Page |
|---|---|---|
| `commands` | `[[contributes.commands]]` | [commands.md](commands.md) |
| `events` | list of topic globs | [events.md](events.md) |
| `schedules` | `[[contributes.schedules]]`: `id`, `every` (seconds, 1 to 2592000; at most 8) | [events.md](events.md#schedules) |
| `hooks` | list of hook point names; each point's permissions must be declared | [events.md](events.md#hooks) |
| `providers` | `[[contributes.providers]]`: `kind`, `id`; the kind's permissions must be declared | [providers.md](providers.md) |
| `pages` | `[[contributes.pages]]` | [ui.md](ui.md) |
| `themes` | `[[contributes.themes]]`; needs `ui:css` | [themes.md](themes.md) |
