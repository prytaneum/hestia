# The plugin kernel

*[← Architecture](../architecture.md)*

Hestia is a small kernel run by plugins. One registry holds two kinds: **built-in** plugins (Hestia's own native code,
registered through the same model) and **WASM** plugins (sandboxed, signed, approved by the user). This page covers the
`kernel` crate and the model; [contributing.md](../contributing.md#add-a-built-in-plugin) has the recipes and
[plugins/](../plugins/README.md) is the author guide.

## The model

|                          | Built-in                                | WASM                                         |
|--------------------------|-----------------------------------------|----------------------------------------------|
| Code                     | Rust compiled into `hestiad`            | a component in a `.hplugin` package          |
| Trust                    | Hestia itself: no grant, no prompt      | sandboxed, signed, the user approves a grant |
| Version                  | the app version                         | its own, updated from a repository           |
| Provides an API          | a WIT package it owns                   | not yet                                      |
| Defines extension points | yes: export interfaces others implement | no                                           |
| Fills extension points   | yes, natively                           | yes, by exporting the interface              |
| Can be disabled          | yes, unless an enabled one needs it     | yes                                          |
| Removed                  | no                                      | yes                                          |

- **The registry** (`crates/kernel/src/registry.rs`) lists every plugin with its id, kind, version, state, what it
  provides and what it requires. `plugin.list` reads it. States: `enabled`, `disabled`, `failed`, `needs_approval`,
  `needs_update`, `missing_dependency`.
- **Ids** are one namespace across both kinds; a WASM plugin cannot take a built-in's id or a CLI verb.
- **A contract names its owner** with `const OWNER: Option<&'static str>`; the router answers `plugin_disabled` for a
  contract whose owner is off.
- **The API** is typed WIT, one package per providing plugin, all at one version, with one import interface per
  permission. A component's permissions, dependencies and contributions are read from its imports and exports, never
  declared, so a manifest cannot ask for less or more than the code uses. Only `hestia:kernel` exists today; built-ins
  add their own packages.
- **Extension points** are export interfaces defined in the package of the plugin that consumes them. A plugin fills one
  by exporting it; exports are found per interface, so one added later reads as not provided. The kernel's own:
  `lifecycle`, `commands`, `settings-changed`, `on-schedule`, `on-job-done`.
- **Dependencies** are a package a component imports (provided by a plugin) or a `[dependencies]` entry in the manifest.
  They resolve at inspect, before any code runs, and one prompt covers the plugin and everything it brings.

The choices behind the model are recorded in [decisions/](../decisions/README.md).

## The kernel crate

`crates/kernel` holds the plugin registry and network vetting. Its runtime (the registry, wasmtime, the network policy)
sits behind the default `sandbox` feature. The `cli` depends on it with `default-features = false` and gets only what
reads a package: the manifest parse, the component shape, the grant derivation (for the current platform, or for every
one), the version gates, the `.hplugin` and index formats. Cargo unifies features within one invocation, so `cli` is
built in its own (`scripts/sidecars.sh`, `scripts/package.sh`); in a build with the daemon the sandbox comes back.

A built-in implements `Builtin` (a descriptor with `provides` and `requires`, plus `enable` and `disable`) and is
enabled in dependency order. Disabling is refused, naming every enabled plugin that needs it, unless the caller confirms
(`cascade`), which disables those first; it is also refused while it has work running. The disabled ids persist in
`plugins.json`; start-up never enables what the user turned off, so a plugin whose requirement is off stays `failed`.
The kernel depends on `proto` and `common` only, so it cannot reach the launcher. `hestiad`'s runtime builds the
registry from `engine::builtins()` and the router consults it for owner gates.

## Packages and the grant

WASM plugins sit in the same registry. A package is a directory with `plugin.toml` (identity, the API version it was
built against and the scope of each permission that takes one) and `plugin.wasm`, each read only as a regular file under
a size cap. `plugin.inspect` derives the **grant** from the component's top-level imports and exports without compiling
it: each `hestia:kernel` interface maps, through the package's table, to a permission with a tier, so a manifest cannot
ask for less or more than the code uses. Another `hestia:` package is a requirement an enabled built-in must provide;
anything else but `wasi:` is refused, and a `wasi:` package the table lists (`wasi:http`) goes through it like any other
without becoming a requirement. The `approval_digest` covers the id, the exact manifest and component bytes, each
permission with its scope (its unhashed hosts and, for each program, its arguments and pinned digest), and the
requirements, never Hestia's prompt text; `plugin.install` refuses a package that no longer derives it, and start-up
recomputes it and lists a changed package `failed` without running it. A permission the grant lacks links as a stub
whose functions answer `denied(<permission>)`; an interface with a function that has no error to answer with cannot be
denied. A manifest's `[dependencies]` (`id = "^1.2"`, caret ranges only, at most 16) names other plugins, built-ins
included, and is part of the grant. `plugin.inspect` (and the release and update inspections) resolve it before any code
runs: each dependency must be installed, enabled and at an accepted version, or be listed by an added repository (the
one that vouched for the dependent first, then the others, each index fetched once and the next repository tried when a
download fails), which then joins the plan after what it needs itself. What an installed dependency declares is walked
too. A cycle, a chain over 8, a plan of more than 32 plugins or a package that cannot be installed is refused
(`plugin_invalid`), an unresolvable dependency is `plugin_missing_dependency`. The inspection carries the `plan`, each
plugin to install with its own signer and grant, and its `approval_digest` covers all of it, so one prompt approves the
whole install and a plan that moved since the prompt is `plugin_changed`. Everything that can still refuse the call (a
built-in a plugin needs, the chosen optional permissions, a busy plugin, a dependent's range) is checked before anything
is installed. An install then adds the plan's plugins first, in order, each with no optional permission and no entries,
and removes them again, with their `sandbox/<id>`, if one fails or does not start; the members are written to
`plugins/pending.json` meanwhile, so a crash is undone at the next start, before anything starts, unless the plugin is
already stored under the recorded digest (the plan finished and only its record was left). Enabling starts what a plugin
needs first, and start-up orders installed plugins the same way; one whose dependency is not installed at an accepted
version, or whose imported package no enabled built-in provides, is listed `missing_dependency`, one whose dependency is
installed but off stays `failed`. A built-in declared as a dependency counts like any other: disabling it is refused or
cascades. Removing a plugin another installed plugin declares is refused, and so is an update that leaves a dependent's
range. Installs, enables, disables and removals run one at a time. The daemon runtime owns one `kernel::Sandbox` (none
if it cannot start, and WASM plugins are then listed but never run): compiled components under `meta/plugins/`, vouched
for by `plugins.key`; `app.info` reports online only while the network is reachable and offline mode is off; installed
plugins start after recovery, off the accept loop; a plugin that reaches the trap limit is disabled and stays so across
restarts, and removing one deletes its cache entry.

## Trust and repositories

A WASM plugin comes from a directory (a dev install) or from a repository. A dev install is refused unless
`plugins.developer` is on, and it runs only while it is: the daemon tells the registry when the setting changes, so
turning it off lists every dev install `failed` (enabling one answers why) and turning it on starts them again.
Installing a directory whose id is already a dev install replaces it as an update does (`.previous` swap): its settings,
secrets and schedule runs stay, `grant::carry` keeps the optional permissions granted, and the choice in the call is
ignored.

A repository is added with a pinned minisign key (`repositories.json`). Its `index.json`, signed under that key, lists
per plugin id the author's key and the releases: the URL, SHA-256 and size of each `.hplugin`. The signature's trusted
comment, `hestia-index <repository> <sequence> <expires>`, must match the body, so it cannot be lifted onto another
repository or a longer life; an index below the highest sequence seen is a replay, one past `expires` is dead, and one
that expires more than 30 days out is refused. Index and package fetches go through `vetting` on the repository's host
and the hosts its signed index names.

A release is a `.hplugin`: a zip of exactly `plugin.toml`, `plugin.wasm` and `plugin.minisig`. The author signs, with
their own key, `hestia-hplugin-v1\n<id>\n<version>\n<sha256 plugin.toml>\n<sha256 plugin.wasm>\n` (the manifest's id and
version), never the archive bytes. It is accepted only when the index verified under the repository key, the file
matches the index's digest and size, and the signature verifies under the author key the index lists for that id. An
entry beyond the three, a directory, a path with `/`, `\` or `..`, a symlink or a duplicate is refused, and each entry
is read under the caps of a loose package, counted as it decompresses. Keys are compared by their ed25519 bytes, never
by minisign's key id. `trust::OFFICIAL_KEYS` (none yet) are Hestia's: a repository cannot pin one, and only a package
signed by one is shown as official, so no index can make another key official.

The approval record (`approval.json`) keeps the approved grant, the choice made within it (the optional permissions
granted and the entries scope), the source (dev, or the repository with its key and the author's key) and any
revocation; the repository and author key enter the approval digest, and the prompt shows the repository and the
author's fingerprint. An update must be listed by the repository under the key that vouched for the install and signed
by the same author key, whatever the index now says; an author-key change is refused, not applied. It replaces the files
through a `.previous` directory: a version that fails to link or to start is rolled back, and a restart restores the
previous version of an update that never finished (a finished one renames `.previous` to `.staging` before deleting it,
and `remove` deletes both). It asks for approval again when `grant::widens` finds a permission, a scope entry (host,
endpoint, unchecked host, or program by path, arguments and digest), a required package, an export or a schedule (a new
one, or one that ticks more often) the approved grant lacked; nothing updates without the digest of what was shown.

Every repository's index is fetched when the daemon starts, after recovery, and every 6 hours while online. A version
the index revokes is marked in its record, disabled and listed in the `revoked` of `plugin.updates`, so it never starts
again. A repository that was removed and added again starts from sequence 0, which is also how to recover one whose
stored sequence a stolen key pushed out of reach.

## Network vetting

`vetting` is the one network policy: every request and download, core or plugin, is held to https, a granted host (a
wildcard matches one label and never a public suffix), a resolver that refuses addresses inside the machine, a time and
size cap, and a checksum or a declared unhashed host, rechecked on every redirect. The engine's core callers state their
hosts in `engine::net::hosts`.

## The sandbox

The kernel also holds the WASM sandbox (`runtime/`): one wasmtime engine for the daemon, a fresh store and instance per
call, run as async calls that yield on every 10 ms epoch tick, so each call's wall-clock budget (`init` 10 s, an event
such as `settings-changed` 5 s, a hook 2 s) also bounds a guest waiting inside a host call, and dropping the call stops
the guest. Memory is capped at 256 MiB under the pooling allocator. At most 4 calls per plugin and 16 overall run at
once; the rest wait for a slot within their budget. The third trap in ten minutes is reported as `TrapLimit`, and the
registry disables the plugin. WASI has no sockets and an empty environment; what reaches outside the plugin is
`wasi:http`, `download`, `programs` and `ipc`, below. Compiled components are cached under `meta/plugins/`, one entry
per plugin, each tagged with HMAC-SHA256 under a per-install key kept outside `meta/`; an entry whose tag does not cover
its source, the engine and its bytes is recompiled, never loaded. The API is `hestia:kernel@0.1.0` (`types`, `log`,
`app`, `data`, `settings`, `secrets`, `messages`, `progress`, `notify`, `jobs`, and `download`, `programs` and `ipc` in
a world of their own, `reach`, which a toolchain that keeps every world import, such as `jco`, leaves out of a plugin
that does not use them; exports `lifecycle`, `settings-changed`, `commands`, `on-schedule` and `on-job-done`) in
`crates/sdk/wit/kernel/`. `crates/sdk` (`hestia-sdk`, MIT OR Apache-2.0, a default member, built natively for its mock
and for `wasm32-wasip2` by plugins) always carries the imports and enables one export per cargo feature;
`hestia::export!` exports the enabled ones. A component's exports are found per interface, a missing one meaning not
provided. In developer mode a trap logs the guest backtrace (function names from the component's name section, file and
line when it carries DWARF; the engine always keeps DWARF, which costs some memory per compiled component, bounded by
the trap limit) and every `denied` a plugin gets, from a missing permission, an entry outside its scope or a host,
program or endpoint outside its grant, names its function (`denied: <interface>#<function>`) under the per-call log cap;
outside developer mode the error a call returns is the root cause alone. On native builds `hestia::mock` answers every
import from memory and denies what the grant a test sets lacks, scopes included; it reads each gated interface's
permission from the WIT doc ("Permission `x`"), which `crates/kernel/tests/wit.rs` keeps equal to the table, and
`crates/sdk/tests/mock.rs` runs in the root `cargo test`. The tests build the guests in `crates/kernel/tests/guest/` and
`guest-services/` once per run; `examples/typescript/` builds a `lifecycle`-only plugin with `jco` under Bun in CI.

## Version gates

A package's WIT version is the API version; the table of each package states it, and the stability of an interface
marked `@since`, `@unstable` or `@deprecated` (`Table::versioned`, `Table::stability`). The manifest's optional `api` is
the version the plugin was built against: `plugin.inspect`, install and start-up refuse an `api` that is not the version
the component imports, a `hestia:` import or export that is not on the host's `0.y` line (`0.0.z` exactly), is a
pre-release or is later than the host's, and an interface newer than the version that names it. An `@unstable` interface
is refused unless the package is a dev install (which runs only in developer mode); a real one needs `features: [...]`
in the host's `bindgen!` and is then protected only by this gate. An import of a deprecated interface is a
`PluginImportDeprecated` warning in the `Inspection`, which `hestia plugin` prints. Without `api` only the imports and
exports are checked. Version attributes live on interfaces only. The built-in's table must state its package's version
and parseable stabilities, or `Plugins::new` refuses it. `crates/kernel/tests/wit.rs` keeps the tables equal to the WIT
and fails a changed WIT of the kernel or a built-in package under an unchanged version (`crates/sdk/wit/snapshot.txt`;
the policy is in `crates/sdk/wit/README.md`).

## Built-in packages

A built-in exposes a WIT package of its own through `crates/kernel/src/api/<package>/`: its table (`Builtin::table`,
merged into the kernel's by `Plugins::new`, which refuses a row outside the package the built-in `provides`, an
interface or permission already in another table, and a provided package without a table), the adapter (the generated
`Host` traits, converting to its contracts and calling `HostCalls`) and the calls into the extension points it defines
(`Sandbox::extend`; a malformed extension export fails the load). Importing the package is a requirement on that
built-in: an install is refused without it, a restart leaves the plugin `failed`, and the built-in cannot be disabled
while an enabled plugin needs it. `HostCalls` is the router as the plugin: the daemon's `RouterCalls` routes as
`Caller::Plugin(chain)`, and `run_job` waits for the job's `done` and cancels the job if the plugin stops waiting. Every
call enters its plugin into the `Chain` before it runs, so a plugin already in the chain, or a chain deeper than 8, is
refused before anything runs. The `probe` feature compiles the test built-in `hestia:probe` and the adapter plumbing,
which no production package uses yet; the kernel tests drive it with `tests/guest-services/src/bin/consumer.rs`.

### Topics and hooks

Behind the `probe` feature a package also defines **topics** and **hooks**, export interfaces the emitting plugin calls;
like the rest of the plumbing above, no production package uses them yet. The table maps an exported interface to a
permission like an imported one, so exporting a topic needs its read permission (and appears in the grant); a topic
whose permission depends on its payload is split into one interface per permission. An exported `hestia:` package other
than the kernel's is a requirement on its built-in exactly like an import: an install is refused without it, and
disabling the built-in is refused or cascades. A topic is delivered to each enabled plugin that exports it, holds that
permission and has every entry the event names in its `entries` scope (the emitter passes stable ids), through a
per-plugin inbox of 256 with one delivering task. Events wait there for the plugin's event budget (30 in a burst, one
more each second, apart from its wake-ups, so a flood never starves its schedules or `jobs.start`); only a full inbox
drops, the oldest first. Each event is checked again just before it is delivered: it goes only to the guest that was
queued for, and only while that guest still exports the interface, holds the permission and has the entries in scope. A
relink (a permission change, an update, a reinstall) drops the queue; a narrowed scope drops only the events it no
longer covers. A hook (2 s) runs over the plugins that export it in id order, for a call made through the caller's
`Chain`, each seeing the result of the one before; a plugin that fails, traps, is already in the chain or times out is
skipped, and only an explicit veto stops the rest. The test built-in defines the topics `entry-opened` (`probe:entries`)
and `echoed` (`probe:read`) and the hook `before-echo` (`tests/events.rs`).

## Optional permissions and the entries scope

A permission the manifest marks `optional = true` (`[permissions."probe:read"]`, alone or beside its scope) arrives
declined: the user ticks it at approval (`Choice.optional`) or grants it later (`plugin.permission`, `hestia plugin
grant`), and the plugin is relinked, a declined interface answering `denied(<permission>)`. The mark must be on a
permission the component needs, and on one every gated interface of which can be answered `denied` (`grant::deniable`:
not `wasi:` nor `ipc`, which hold resources and fail the load instead); an exported interface is not checked, since a
declined one is simply not delivered or called. A choice naming a permission that is not optional is refused. Granting
waits for a plugin with no command running; a revoke narrows, so it halts them, and a call already in flight keeps the
link it began with until it ends. `hestia:kernel/permissions` (no permission) answers `granted(permission)` and
`entries()` (`ids` empty when `all`), so a plugin adapts to what was approved.

An update keeps a granted optional permission only while it is optional in both versions with a scope no wider
(`grant::carry`); one that was dropped and added back, or whose scope widened, arrives declined, and one that was
required and became optional stays granted. An update that adds an optional permission does not ask again; one that
turns an optional permission required does.

A permission whose table row has `ScopeKind::Entries` takes the **entries scope**, which the user chooses (all, or
listed stable ids, at most `EntryScope::MAX_IDS`) rather than the manifest: at approval, or later with `plugin.entries`
(`hestia plugin entries`), or with the update that adds the permission. The daemon resolves every reference a client
names (an id or a name) to a stable id and refuses one that resolves to nothing or to two entries. The scope is one per
plugin, stored with the approval (`approval.json`, `choice`) and kept outside the digest, so a permission that gains the
scope reaches nothing until the user picks. Every write to it, and to the optional choice, goes through `Store::amend`,
one lock over each record rewrite, a swap and a removal, so none loses another's field and none recreates a removed
plugin.

## The gate

The gate is the daemon's `RouterCalls`, not a handler. An adapter can only call a contract that implements
`proto::access::EntryAccess` (and `JobAccess` for a job), which has no defaults: `touches` lists the `EntryTarget`s
(kind and reference) a call acts on, and `creates` the ones it made, a contract that does neither returning an empty
list on purpose. `RouterCalls` resolves each target through the engine to the entry's stable id and refuses the call
with `entry_out_of_scope` (the plugin sees `denied(<permission of the interface>)`) unless every one is inside the scope
of every WASM plugin in the call chain, so a plugin cannot reach through another what it may not reach itself. What a
call or a job's `done` made joins the caller's scope, and persists. The registry's own `plugin.*` channels refuse a
plugin caller (`person_only`).

## Network, programs and endpoints

Three kernel permissions carry a manifest scope the sandbox enforces on every call (`runtime/reach.rs` reads it from the
grant). A plugin that is offline reaches nothing: `download` answers `offline` and `wasi:http` refuses.

- `network:connect` (`hosts`, and `unhashed` for the hosts, among them, that may serve a download with no checksum) is
  the plugin's `vetting::Policy`. `wasi:http` (through `wasmtime-wasi-http` with its own `send_request`; the guest
  follows redirects itself and each hop is vetted again) may reach any granted host. `download` streams into `/data`
  through a `cap-std` directory handle on it, so no link the plugin plants is followed out; the file is staged as
  `.part` (unlinked if the call stops) and renamed into place, and needs a checksum or an unhashed host.
- `process:spawn` (`programs`, unrestricted) lists invocations: `{ path, args, os }`, an absolute path, its exact
  arguments and, optionally, the one OS (`linux`, `windows`, `macos`) it is for (without `os`, the path's form decides:
  `/...` is unix, `C:\...` is Windows); only the current OS's entries are granted. The path is judged for its own
  platform on every machine (`grant/program.rs`), before any file is opened: unix paths start with `/`; Windows paths
  are `C:\...\name.exe` drive paths, with no UNC, `\\?\`, `\\.\`, stream, wildcard or device name. `derive` reads no file; the registry then pins each program by its sha256 (a regular file under 256 MiB,
  checked on the opened handle) off the async executor, so a program that changed fails its approval at the next start
  and is refused again at the call. A program inside a plugin's sandbox (`sandbox/`), or one of Hestia's own binaries,
  is refused, since the plugin could rewrite the first and the second would reach Hestia without its gates. What runs is what
  was hashed: on Linux the program is copied into a sealed `memfd` while it is hashed and that copy is executed; on
  Windows the file is held open without write or delete sharing until the process has started; elsewhere the start is
  by path, after a check that the path is still the hashed file. It runs with no shell, an empty environment (Windows
  keeps `SystemRoot`, `SystemDrive`, `windir`, `ComSpec` and `PATHEXT`, and `TEMP`/`TMP` are `/data`), `/data` as its
  directory and output capped at 1 MiB, in its own process group (a job object on Windows, joined while the process is
  still suspended) that is killed when it ends or the call stops. Output is read for a short grace after the program
  ends, so a process that left the group cannot hold the call.
- `ipc:connect` (`endpoints`, unrestricted) names exact unix socket paths or Windows pipe names; only the current
  platform's form is granted. The daemon's own endpoint, any pipe in Hestia's `hestia-` namespace and a pipe name Windows
  would normalise (`.`/`..`, separators, trailing dots) are refused, and a socket is judged again at connect time as the
  file it resolves to, so a symlink to Hestia's socket is refused; the busy-pipe wait ends after a few seconds.
  `connect` returns a connection resource with
  `write` and `read`, closed when dropped or when the call ends. A plugin whose `ipc` permission was not granted fails
  to load, like one whose `wasi:http` was not: those interfaces hold resources, which cannot be answered with `denied`.

## Services

The services a plugin calls reach the host through the `Services` trait the sandbox is given (`with_services`); the
defaults provide nothing, so `settings` answers `unsupported` and `confirm` is declined. `/data` is the only WASI
preopen and holds the plugin's own files (`<data_home>/sandbox/<id>/`); settings and secrets are schema documents in
`<data_home>/plugins/<id>/`, apart from `/data` so no link in it reaches them, read once and kept in memory per plugin
(`secrets.json` owner-only, an unreadable file set aside). An id is lowercase letters, digits and `-`, never a name
Windows reserves (`kernel::id`). The grant, not `secrets`, refuses a plugin the user did not grant it. `data.try-lock`
takes a named lock that lasts until `unlock` or the end of the call, however it ends. `messages` renders at most 16
arguments into 4 KiB. `notify`, `confirm` and `open-url` never draw anything themselves: the daemon publishes
`plugin.notice`, `plugin.question` (answered by `plugin.answer`, which names the plugin, and withdrawn by
`plugin.question.closed`) and `plugin.link`, and the desktop draws each with the plugin's name. A call allows five
notices, one question and one link; only a call a person started may ask (`settings-changed` is one), its budget pauses
while the person decides, and the sandbox declines after two minutes. `open-url` takes what the network policy allows
for a URL nobody granted. Only a front-end that subscribes as drawing plugins (`events.subscribe` with `drawsPlugins`,
the desktop) is asked; with none connected a question is declined and a link refused. Progress counts only in a job, and
`cancelled` is the call's own. `Sandbox::change_setting` stores a value the user changed and then calls
`settings-changed`; `Sandbox::store_secret` stores one the user entered.

## Commands, schedules and jobs

A plugin's manifest declares its **commands** (a name, a summary and typed arguments: string, integer or boolean) and
its **schedules** (`every = "30m"`, a whole number of minutes, hours or days, counted from the last run). A manifest
declares at most 32 commands and 8 schedules, and no command is named `help` nor argument `help` or `version`, which the
CLI's parser keeps. The component must export `commands` for the first and `on-schedule` for the second, or the install
is refused. Schedules are part of the grant (`Grant.schedules`: id and interval), because they act without anyone
asking: the prompt lists them and `widens` flags a new one or one that ticks more often; a new command needs no new
approval, a person starts it. `plugin.run` runs a command as a job (for a person, never for a plugin that calls the
channel): it has the command budget of an hour, reports `progress` (only the latest report is kept, so a plugin
reporting in a loop queues nothing), may `confirm` because a person started it, and is cancelled cooperatively through
`progress.cancelled`. A cancel gives the plugin a grace of 5 s (the event budget) to return; one still running is
dropped, which stops the guest and releases the named locks it held, and the job reports `cancelled`. What it wrote to
`/data` stays. A plugin with a command running cannot be disabled, removed or updated (`PluginBusy`); developer mode
turned off, the plugin's trap limit and the daemon stopping stop its commands outright. `jobs.start` (in `base`: no
permission, only the budget below) queues one of the plugin's own commands the same way (never another plugin's: the
plugin is whoever the call runs as), not as a person, and when that job ends, outside the job, the plugin's
`on-job-done` hears its id and outcome. Each `jobs.start` and each schedule tick spends a wake-up: 30 in a burst, one
refilled per second per plugin, and a tick that finds none is skipped. The daemon checks the schedules once a minute
(`Plugins::run_due`, plugins side by side). The last run of each schedule is kept as unix time in
`plugins/<id>/schedules.json`, started at install and at enable: a run missed while the daemon was down or the machine
asleep is made up once at the next check, and a clock set back counts from its new time and never repeats a run.
Everything a command run shows (`plugin.notice`, `plugin.question`, `plugin.link`, and its `closed`) carries the job id,
passed to the host through the `Services` calls, so a front-end that started the run subscribes to that id and draws
only its own, and counts as a surface only for that run's questions; the CLI does this for `hestia <plugin> <command>`,
the desktop still draws every plugin's. A plugin id may not equal a top-level `hestia` command
(`proto::naming::CLI_VERBS`, kept equal to the parser by a CLI test). Commands and schedules are exports, so an update
that adds the `commands` or `on-schedule` export widens the grant and asks again; only an enabled plugin runs either, so
a directory install left `failed` while developer mode is off answers no command and ticks no schedule.
