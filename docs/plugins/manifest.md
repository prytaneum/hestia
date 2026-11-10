# The manifest

`plugin.toml` sits at the root of the plugin's folder and, packed, at the root of the `.hplugin`. The types are
`proto::plugin::Manifest` and its parts (`crates/proto/src/plugin.rs`); `Manifest::validate` is what `pack`, `dev` and
install run, and each refusal below is one of its `ManifestError`s. Keys are camelCase; none of the current ones has a
second word. An unknown key is ignored.

```toml
id = "tunnel-demo"
name = "Tunnel demo"
version = "0.1.0"
wasm = "plugin.wasm"
description = "Runs a stand-in tunnel agent while a server is up, to show the process:spawn model."
permissions = ["process:read", "process:spawn"]

[[programs]]
id = "agent"
command = "sleep"
args = ["*"]

[contributes]
events = ["process.started", "process.exit"]
```

Source: `plugins/tunnel-demo/plugin.toml` in the examples repository.

## Identity

| Key | Type | Required | Rule |
|---|---|---|---|
| `id` | string | yes | 2 to 40 lowercase letters, digits or dashes, no leading or trailing dash; also the directory name. Not one of `RESERVED_IDS` (the CLI's top-level verbs: `account`, `config`, `server`, `instance`, `plugin`, …) |
| `name` | string | yes | display name |
| `version` | string | yes | `major.minor.patch`; a pre-release or build suffix is allowed and ignored when ordering |
| `description` | string | no | one line |
| `authors` | list of strings | no | |
| `homepage` | string | no | |
| `wasm` | string | with a component | package-relative path of the component |
| `api` | string | with a component | caret requirement on the host ABI, `^N.M.P`. `pack` writes it from the `hestia-sdk` version cargo resolved; do not type it |

A plugin with a `wasm` must have an `api` (`MissingApi`), and the host's `API_VERSION` (`1.0.0` today) must satisfy it:
a guest built for `^N.M.P` runs on a host `N.m.p` at or above `N.M.P`, otherwise install is refused. A theme-only plugin
has neither `wasm` nor `api`. Paths (`wasm`, a theme's `file`, a page's icon file) must be relative and stay inside the
package: no leading `/`, no `\`, no `:`, no `.` or `..` segments.

## Access

| Key | Type | Meaning |
|---|---|---|
| `permissions` | list of permission names | see [permissions.md](permissions.md) |
| `network` | list of hosts | the hosts `network:connect` reaches: a domain name, or `*.domain` for its subdomains at any depth. An address is refused (`BadNetworkHost`) |
| `ipc` | list of names | the local endpoints `ipc:connect` reaches, such as `discord-ipc-*`; refused without the permission (`IpcPermissionMissing`) |
| `[[programs]]` | tables | the programs `process:spawn` may run; refused without the permission (`ProgramPermissionMissing`) |
| `[launch] properties` | list of strings | the `-D` system property keys a launch hook may set |
| `[[settings]]` | tables | typed user settings |

Together with `contributes.hooks`, `events`, `schedules` and the theme ids, these form the grant the user approves; an
update that changes it asks again.

### `[[programs]]`

| Key | Type | Meaning |
|---|---|---|
| `id` | slug | what the SDK's `process` calls name; unique |
| `command` | string | a name on `PATH` (1 to 64 of letters, digits, `.`, `_`, `-`, `+`, not starting with `.` or `-`), or, with a `/`, a file inside `data/`; omitted when `java` is set |
| `java` | integer, 1 to 99 | instead of `command`: the Java major of the runtime Hestia manages, installed when missing and resolved at call time, never pinned. Exactly one of `command` and `java` (`ProgramCommandOrJava`); no `sha256` |
| `args` | list of strings | patterns: a literal, `*` for one token, a trailing `**` for the rest; `**` anywhere else is refused |
| `env` | list of names | the only environment variables a call may pass. `LD_*`, `DYLD_*`, `PATH` and `*JAVA*OPTIONS` are refused (`BadProgramEnv`) |
| `path` | list of program ids | other `PATH` programs of this manifest whose pinned directories are the program's `PATH`, and nothing else; each must be another `PATH` program, named once (`BadProgramPath`) |
| `sha256` | table, target triple to hex | required for a `data/` program (`ProgramNeedsHash`): 64 lowercase hex characters per triple |

A shell (`sh`, `bash`, `zsh`, `dash`, `fish`, `ksh`, `csh`, `tcsh`, `cmd`, `powershell`, `pwsh`, `wscript`, with or
without `.exe`) is refused as a command. How programs run is in [processes.md](processes.md).

### `[launch]`

```toml
[launch]
properties = ["my.property"]
```

A key is 1 to 100 characters of letters, digits, `.`, `_` and `-`, and may not be one that loads code or weakens the
JVM: `java.library.path`, `java.system.class.loader`, `java.ext.dirs`, `java.endorsed.dirs`, a name starting
`java.security.`, `jdk.`, `sun.` or `log4j`, or containing `agent`, `classpath`, `class.path` or `module`
(`BadLaunchProperty`). No example plugin declares one; `my.property` is a placeholder.

### `[[settings]]`

```toml
[[settings]]
key = "client-id"
kind = "string"
label = "Discord application id"
help = "The id of a Discord application you created; its assets name the images."
```

Source: `plugins/discord-presence/plugin.toml`.

| Key | Type | Meaning |
|---|---|---|
| `key` | slug | unique; what `hestia::settings::get` reads |
| `kind` | `string` (default), `number`, `bool`, `select`, `secret` | the value type; the daemon checks every stored value against it, and a string is at most 4096 bytes |
| `default` | a value of that kind | used while the user has set nothing |
| `label`, `help` | string | shown in the app and `hestia plugin config` |
| `options` | list of strings | the choices of a `select`; it must not be empty |

A `secret` has no default and is listed only as set or unset; its value leaves the daemon only for this plugin. A
default that does not fit its kind is refused (`BadSetting`). The plugin reads a value with
`hestia::settings::get::<T>(key)` and is told of a change through `Plugin::on_settings_changed(key)`.

## `[contributes]`

What the plugin adds. Every contribution id is 1 to 40 lowercase letters, digits or dashes and unique within its list.

```toml
[contributes]
events = ["process.started", "process.exit", "process.metrics"]
```

| Key | Type | Page |
|---|---|---|
| `commands` | `[[contributes.commands]]` | [commands.md](commands.md) |
| `events` | list of topic globs | [events.md](events.md) |
| `schedules` | `[[contributes.schedules]]` | [events.md](events.md#schedules) |
| `hooks` | list of hook point names | [hooks.md](hooks.md) |
| `providers` | `[[contributes.providers]]` | [providers.md](providers.md) |
| `pages` | `[[contributes.pages]]` | [ui.md](ui.md) |
| `themes` | `[[contributes.themes]]` | [themes.md](themes.md) |

### Commands

```toml
[[contributes.commands]]
id = "greet"
title = "Greet"
description = ""
scope = "top"          # top (default), server or instance

[[contributes.commands.args]]
name = "name"
kind = "string"        # string (default), number, bool or entry
required = true
help = ""
```

The `hello` example declares one; the fields are `proto::plugin::CommandContribution` and `CommandArg`. A scoped
command may not take a built-in verb of its scope as its id (`ReservedCommand`), and may not declare an argument named
`entry` (`ReservedArg`). Argument names are slugs, unique per command. See [commands.md](commands.md).

### Events and schedules

`events` entries are an exact topic (`job`, `process.exit`) or a prefix glob with a trailing `.*` (`process.*`), of
lowercase letters, digits, `.`, `-` and `_` (`BadEventPattern`).

```toml
[[contributes.schedules]]
id = "poll"
every = 300            # seconds, 30 to 2592000 (30 days); at most 8 schedules
```

The schedule table has no example plugin; its fields are `ScheduleContribution`. `every` outside the range is
`BadScheduleInterval`, a ninth schedule is `TooManySchedules`.

### Hooks

`hooks` lists point names. An unknown name is `UnknownHook`; a point's permissions must be in `permissions`
(`HookPermissionMissing`). The points are `before-launch`, `before-server-start` and `before-content-install`
([hooks.md](hooks.md)).

### Providers

```toml
[[contributes.providers]]
kind = "content"
id = "index"
```

Source: `plugins/static-index/plugin.toml`. `kind` must be one this host offers (`UnknownProviderKind`) and its
permissions must be held (`ProviderPermissionMissing`); a kind that needs `network:connect` also needs at least one
`network` host (`ProviderNeedsHosts`).

### Pages

```toml
[[contributes.pages]]
id = "overview"
title = "Overview"
icon = "server"           # a name from the app's icon set, or a package-relative image path (anything with a / or a .)
slot = "entry"            # nav (default), entry or settings
kind = "server"           # required when slot = "entry": server or instance
```

The `dashboard` example declares a `nav` and an `entry` page; the fields are `PageContribution`. The icon names are listed in
[ui.md](ui.md#declaring-a-page). An `entry` page without a `kind` is
`PageNeedsKind`. An icon path is checked like any package path.

### Themes

```toml
permissions = ["ui:css"]

[[contributes.themes]]
id = "ember-night"
name = "Ember Night"
file = "themes/ember-night.css"
appearance = "dark"
```

Source: `plugins/theme-ember-night/plugin.toml`. `appearance` is `light`, `dark` or `both` (default). Declaring a theme
without `ui:css` is `ThemeNeedsUiCss`.

## Validation, in order

`validate` stops at the first failure: id shape, reserved id, version, `api` against `wasm`, package paths, page icons
and kinds, `network` hosts, `[launch]` keys, settings, `ipc`, programs and their permission, schedule count and range,
event globs, command ids and arguments, the theme permission, hooks, duplicate or malformed ids across every list, then
providers. `hestia plugin pack` and `hestia plugin dev` report the same error the install would.
