# The manifest

`plugin.toml` sits at the root of the plugin's folder and, packed, at the root of the `.hplugin`. The types are
`proto::plugin::Manifest` and its parts (`crates/proto/src/plugin/`); `Manifest::validate` is what `pack`, `dev` and
install run, and each refusal below is one of its `ManifestError`s. Keys are camelCase (`helpUrl`, `refreshEvery`). An unknown key is ignored.

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

`[permissions]` is a table. Each key is a permission name and its value is that permission's **scope**: an inline table
with an entry list for each kind of scope the permission takes, or `{}` for none. `{}` is the only spelling of "no
scope" (`true` and a bare list are refused). Which kinds a permission takes, and whether it needs one, is the table in
[permissions.md](permissions.md); a kind a permission does not take is `ScopeNotAccepted`, a required one left empty is
`ScopeRequired`, and a kind that is not one of the names below does not parse. The top-level lists of
the earlier manifest shape (`network`, `unhashed`, `ipc`, `files`, `[launch]`) are refused with `RetiredKey`, which names
the scope to declare instead.

```toml
[permissions]
"entry:read" = { files = ["whitelist.json", "ops.json"] }
"content:read" = {}
"network:connect" = { hosts = ["api.adoptium.net"], unhashed = ["meta.fabricmc.net"] }
"process:spawn" = { programs = ["git"] }
"ipc:connect" = { endpoints = ["discord-ipc-*"] }
"launch:patch" = { properties = ["my.property"] }
```

| Scope kind | Taken by | Entries |
|---|---|---|
| `hosts` | `network:connect` (required) | a domain name, or `*.domain` for the names one label below it (`*.example.com` reaches `api.example.com`, not `a.b.example.com`). An address is refused (`BadNetworkHost`), and so is a wildcard over a public suffix such as `*.co.uk` or `*.github.io` (`PublicSuffixWildcard`) |
| `unhashed` | `network:connect` | the `hosts` a flavor may serve a download from without a checksum; each must also be in `hosts` (`BadUnhashedHost`) |
| `files` | `entry:read` | the files of a server's directory it may read through `server.files.read`, as exact relative paths such as `whitelist.json` or `config/a.json`: no leading `/`, `\`, `:`, `.` or `..` segment, no glob. `server.properties` is refused whatever its case (`BadEntryFile`) |
| `endpoints` | `ipc:connect` (required) | the local endpoints it reaches, such as `discord-ipc-*` (`BadIpcEndpoint`) |
| `programs` | `process:spawn` (required) | the ids of the `[[programs]]` it may run; an id the manifest does not define is `ProgramNotDefined` |
| `properties` | `launch:patch` | the `-D` system property keys a launch hook may set (`BadLaunchProperty`, below) |

| Key | Type | Meaning |
|---|---|---|
| `[permissions]` | table of scopes | see above and [permissions.md](permissions.md) |
| `[[programs]]` | tables | the programs `process:spawn` may run, defined here and granted by naming the id in its `programs` |
| `[[settings]]` | tables | typed user settings |

Together with `contributes.hooks`, `events`, `schedules` and the theme ids, and the secret settings, the permissions with
their scopes form the grant the user approves; an update that adds a permission or any scope entry asks again. The
contribution entries are derived, never declared as scope. The old top-level `permissions = [...]`, `network`,
`unhashed`, `ipc`, `files` and `[launch]` keys are gone: `permissions` as a list does not parse, and the others are
ignored like any unknown key, granting nothing.

### `[[programs]]`

A program is an identity (what runs) and one or more invocations (how it may be called). See
[processes.md](processes.md#declaring-a-program).

| Key | Type | Meaning |
|---|---|---|
| `id` | slug | what the SDK's `process` calls name, and what `process:spawn` lists in `programs`; unique |
| `command` | string | identity: a name on `PATH` (1 to 64 of letters, digits, `.`, `_`, `-`, `+`, not starting with `.` or `-`), or, with a `/`, a file inside `data/` |
| `java` | integer, `{ min, max }` or `"profile"` | identity, instead of `command`: a managed Java of that major, of a major in the range the call picks (1 to 99, `min <= max`), or of the game version's Java; installed when missing and never pinned. Exactly one identity (`ProgramIdentity`, `BadProgramJava`); no `sha256` |
| `sha256` | table, target triple to hex | required for a `data/` program (`ProgramNeedsHash`): 64 lowercase hex characters per triple |
| `requires` | `{ name, url }` | what to tell the user when a `PATH` program is missing; `url` is `https` (`BadProgramRequires`) |
| `args` | list of strings | the one invocation, written at the top level: patterns, a literal, `*` for one token, a trailing `**` for the rest; `**` anywhere else is refused |
| `env` | list of names | the environment variables that invocation may be passed. `LD_*`, `DYLD_*`, `PATH` and `*JAVA*OPTIONS` are refused (`BadProgramEnv`) |
| `path` | list of program ids | other `PATH` programs of this manifest whose pinned directories are that invocation's `PATH`, and nothing else; each must be another `PATH` program, named once (`BadProgramPath`) |
| `[[programs.invocations]]` | tables | several invocations instead of the top-level `args`/`env`/`path`, each with a unique slug `name` and its own `args`, `env`, `path` (`BadProgramInvocations`: mixed with top-level keys, an unnamed or repeated name, more than 16) |

A shell (`sh`, `bash`, `zsh`, `dash`, `fish`, `ksh`, `csh`, `tcsh`, `cmd`, `powershell`, `pwsh`, `wscript`, with or
without `.exe`) is refused as a command. Every error of every program is reported together. How programs run is in
[processes.md](processes.md).

### Launch properties

```toml
[permissions]
"launch:patch" = { properties = ["my.property"] }
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
| `min`, `max` | number | the bounds of a `number`, with `min` not above `max` |
| `pattern` | string | a regular expression the whole of a `string` or `secret` must match, at most 256 bytes |
| `placeholder` | string | shown in an empty field |
| `group` | string | the heading the setting is listed under; settings with none come first |
| `helpUrl` | string | an `https` page that explains the setting |

The constraints are checked for the user's writes and the plugin's own (`Setting::check`), and a manifest that puts one
on the wrong kind, a `min` above `max`, a pattern that does not compile or a default that fails its own constraints is
refused (`BadSetting`, naming the key).

A `secret` has no default and is listed only as set or unset; its value leaves the daemon only for this plugin. A
default that does not fit its kind is refused (`BadSetting`). The plugin reads a value with
`hestia::settings::get::<T>(key)` and is told of a change through `Plugin::on_settings_changed(key)`. It may also write
its own settings with `hestia::settings::set` ([settings.md](settings.md)).

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
kind = "string"        # string (default), number, bool, entry or words
required = true
help = ""
```

The `hello` example declares one; the fields are `proto::plugin::CommandContribution` and `CommandArg`. A scoped
command may not take a built-in verb of its scope as its id (`ReservedCommand`), and may not declare an argument named
`entry` (`ReservedArg`). Argument names are slugs, unique per command. A command id is one to three slugs separated by a
space (`players allow`; `BadCommandId`), unique as a whole, and the built-in check reads its first word. A `words`
argument is at most one per command and the last (`BadWordsArg`). See [commands.md](commands.md).

### Events and schedules

`events` entries are an exact topic (`job`, `process.exit`) or a prefix glob with a trailing `.*` (`process.*`), of
lowercase letters, digits, `.`, `-` and `_` (`BadEventPattern`).

```toml
[[contributes.schedules]]
id = "poll"
every = 300            # seconds, 1 to 2592000 (30 days); at most 8 schedules
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
permissions must be held (`ProviderPermissionMissing`); `network:connect` always needs its `hosts` (`ScopeRequired`).

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
`PageNeedsKind`. An icon path is checked like any package path. A page may also declare `topics`, a list of at most 8
topic globs, and `refreshEvery`, 5 to 3600 seconds, to be drawn again while it is open ([ui.md](ui.md#live-pages));
anything else is `BadPageTopics` or `BadPageRefresh`.

### Themes

```toml
[permissions]
"ui:css" = {}

[[contributes.themes]]
id = "ember-night"
name = "Ember Night"
file = "themes/ember-night.css"
appearance = "dark"
```

Source: `plugins/theme-ember-night/plugin.toml`. `appearance` is `light`, `dark` or `both` (default). Declaring a theme
without `ui:css` is `ThemeNeedsUiCss`.

## Validation, in order

`validate` checks every rule and reports all the mistakes, one per line, in this order: id shape, reserved id, version, `api` against `wasm`, package paths, page icons
and kinds, permissions against the scope table (kinds the permission does not take, each entry by its kind's rule, required scopes, unhashed hosts against hosts, program ids against `[[programs]]`), settings, programs, schedule count and range,
event globs, command ids and arguments, the theme permission, hooks, duplicate or malformed ids across every list, then
providers. `hestia plugin check`, `pack` and `dev` report the same errors the install would.
