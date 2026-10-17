# Contributing — conventions & recipes

Practical, copy-and-adapt guides for extending Hestia. Read
[architecture.md](architecture.md) first for the lay of the land, and the
[subsystem page](architecture.md#subsystem-pages) for whatever you are touching.

## Conventions

A few rules hold everywhere; the recipes below assume them.

- **`rustfmt` + `clippy -D warnings` stay clean.** No exceptions; CI enforces both, plus `cargo-deny`.
- **Wire-in is one line.** Each kind of thing has exactly one place it is added — a
  `Command` enum variant (CLI), a `handle::<C>` in the domain's registrar (daemon), a facade accessor (client). Adding a
  feature should not touch the serve loop or the transport.
- **One thing per file / module.** A CLI domain is its own module under
  `commands/`; an engine domain is its own module under `engine/src/`. An aggregation point (the daemon's router, the
  client's facades, the engine aggregate) is a module *directory*, never one growing file: the thing that aggregates
  stays thin and each domain gets its own file beside it.
- **Single-word module names** (`config`, not `config_store`). Follow Rust naming throughout.
- **Errors:** `thiserror` enums in library crates, mapped to an `ipc::errors` code at the daemon's service boundary (via
  `ServiceError`); `anyhow` at binary edges and for multi-step engine operations. Don't panic on recoverable errors.
- **Logging:** `tracing` at appropriate levels. **Never log tokens or secrets.**
- **Identity comes from one place:** `common::app` (`NAME`, `VERSION`, `ID`,
  `VENDOR`, `CHANNEL`). Don't hard-code the product name or version.
- **Immutable/at-the-edge validation:** validate external data where it enters (payloads decode through the contract;
  the config schema rejects unknown keys).

## The wire-in map

Most features touch the same five seams, one line each. The `config` channels are the shipped end-to-end reference
(`hestia config get home` round-trips
`config.get` → `ConfigService` handler → `engine.config()`).

| Seam           | Where                                                  | What                                                  |
|----------------|--------------------------------------------------------|-------------------------------------------------------|
| Wire contract  | `crates/proto/src/<domain>.rs`                         | a struct + `impl Contract` (serde codec)              |
| Engine domain  | `crates/engine/src/<domain>/`                          | a module hung off the `Engine` aggregate              |
| Daemon channel | `crates/daemon/src/services/<domain>.rs`               | one `on.handle::<C>(…)` in that domain's `register()` |
| Client facade  | `crates/client/src/facades/<domain>.rs`                | a one-liner over `Session::call::<C>()`               |
| CLI command    | `crates/cli/src/commands/<domain>.rs` (or `<domain>/`) | a `clap` `Subcommand` + a `run()`, wired in `main.rs` |
| Desktop API    | `frontend/src/api/<domain>.ts`                         | a typed function over the generic `ipc_call` bridge   |

---

## Add a wire contract

Contracts live in `crates/proto`, one module per domain. A call contract names its channel once and pairs it with
`Params`/`Result`; serde derive is the codec, so both sides marshal through this single definition and cannot drift.

```rust
// crates/proto/src/instances.rs
use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty};

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct InstanceListResult {
    pub instances: Vec<Instance>,
}

pub struct InstanceList;
impl Contract for InstanceList {
    const CHANNEL: &'static str = "instance.list";
    type Params = Empty;
    type Result = InstanceListResult;
}
```

Add `pub mod instances;` to `crates/proto/src/lib.rs`. Use `#[serde(default)]` on payloads so an older/newer peer that
omits a field still decodes (additive fields need no protocol bump). **Only an `Option` is skipped when
absent** (`skip_serializing_if = "Option::is_none"`): the generated TypeScript type declares every other field present,
so a list, string, number or flag omitted when empty arrives as `undefined` — and a list throws when iterated.
`tests/omission.rs` enforces it.

**A slow handler says so on its contract.** `Contract::TIMEOUT` defaults to `DEFAULT_TIMEOUT` (10 s). A contract whose
handler waits on the network, a disk tree or a person overrides it with
`const TIMEOUT: Duration = Duration::from_secs(120);`, written as `Duration::from_secs(<int> [* <int>])` because
`scripts/lib/gen-channels.py` reads it. `Session::call` and the desktop bridge both apply it, so no facade method or
API function passes a timeout of its own. A job's start channel answers at once and keeps the default.

**The wire is camelCase.** Every fielded proto struct carries
`#[serde(rename_all = "camelCase")]`, so the socket speaks camelCase and the front-ends consume it with no key
conversion (the frontend's type mirrors are camelCase). Rust field names stay `snake_case`; only the serialized form is
renamed. **Enums are the exception** — their variant *values* stay
`snake_case`/`lowercase` (the frontend's string-literal union types depend on them), so leave an enum's existing
`rename_all` alone. `tests/casing.rs` enforces the struct rule: a new serialized struct without the attribute fails the
build. The one deliberate non-camel exception is the `config.*` key vocabulary (`jvm-args`, `backup-interval`, …), which
stays kebab-case — see
[decision 0028](decisions/0028-camelcase-except-the-config-vocabulary.md). For a daemon→client push, implement `Topic` instead of
`Contract` — the type is its own event payload (a long-running operation is a `Job` instead, see
[Add a daemon channel](#add-a-daemon-channel)):

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessStartedEvent {
    pub id: String,
    pub pid: u32,
}
impl Topic for ProcessStartedEvent {
    const TOPIC: &'static str = "process.started";
}
```

A call that mutates state another client renders sets `const CHANGES: Option<Scope> = Some(Scope::…)` on its
contract; the daemon then publishes `changed` after it succeeds, and the desktop maps the scope in
`queries/invalidation.ts` ([0085](decisions/0085-a-mutating-call-names-what-it-changed.md)).

If you change an existing payload's shape, update `crates/proto/tests/` (the
`wire`/`golden` encodings are pinned on purpose).

---

## Add an engine domain

Launcher/domain logic lives in `crates/engine`, daemon-internal. A subsystem hangs off the `Engine` aggregate root
(`engine/mod.rs`), which the daemon owns and hands to every handler. The worked example adds an `instances` store; model
it on
`config`.

**1. Write the subsystem** as a module under `crates/engine/src/instances/` (or a single `instances.rs` if small). Take
a path under the data dir in the constructor, serialize access with a `Mutex` for concurrent clients, and offer a
`reload()` so a data-home change repoints it.

```rust
// crates/engine/src/instances/mod.rs
use std::path::PathBuf;
use std::sync::Mutex;

use proto::instances::Instance;

pub struct Instances {
    inner: Mutex<PathBuf>,
}

impl Instances {
    pub fn new(dir: PathBuf) -> Self {
        Instances { inner: Mutex::new(dir) }
    }

    pub fn reload(&self, dir: PathBuf) {
        *self.inner.lock().unwrap() = dir;
    }

    pub fn list(&self) -> Vec<Instance> {
        // ... scan self.inner.lock().unwrap() ...
        Vec::new()
    }
}
```

Add `mod instances;` and a `pub use` to `crates/engine/src/lib.rs`.

**2. Hang it off `Engine`** (`engine/mod.rs`) — a field, a getter, construction in
`new()`, and a `reload()` line in `set_data_home()`. This is the *only* change to the engine's wiring; `HandlerContext`
already carries the `Engine`.

```rust
// crates/engine/src/engine/mod.rs — inside struct Engine
instances: Instances,

// in new():
let instances = Instances::new(data_home.join("instances"));

// in set_data_home(), alongside the other reloads:
self .instances.reload(resolved.join("instances"));

// a getter:
pub fn instances(&self) -> &Instances { &self.instances }
```

A helper with no state of its own (like `minecraft`, which is handed the `meta` root per call) can be constructed without a path; a stateless *free*
function needs no aggregate member at all.

A subsystem with an **open set of implementations** — a content platform, an
archive format — gets a trait plus a registry list, so adding one is a module
beside the others and a line in that list ([0010](decisions/0010-one-content-provider-trait.md),
[0057](decisions/0057-an-archive-format-is-a-module.md)). Keep what needs the
aggregate out of the implementations: express it as data the flow matches on,
and the flow stops growing a branch per implementation.

**3. A flow that spans subsystems** — provisioning, launching, backups, content — is *not* a method on the aggregate. It
goes in `engine/flows/<concern>.rs` as an
`impl Engine` block (Rust lets an inherent impl span modules in a crate), so the aggregate stays wiring and callers
still write `engine.provision_server(…)`.

---

## Persist something, or change its shape

Anything of the user's that the engine writes to the data home is a
`schema::Document`: it declares its name, and `schema::load`/`save` handle the stamping,
the temp-file write and the quarantine of a file this build cannot read
([0060](decisions/0060-a-managed-document-carries-its-schema-version.md)).

```rust
impl Document for InstanceRecord {
    const NAME: &'static str = "instance.json";
}
```

For a record living under an entry directory, that is all — `registry::read_record`/`write_record`/`scan` take the file
name from `NAME`, so there is no second constant to pass. A document with its own path (the settings, the accounts)
calls `schema::load`/`save` directly.

**An additive field needs no migration.** `#[serde(default)]` already decodes an older file. There is no migration
chain; it is re-added, keyed on the stamp already on every file, when a released document first needs one.

---

## Add a daemon channel

One `on.handle::<C>(…)` in the domain's `register()`
(`crates/daemon/src/services/<domain>.rs`). The registrar decodes `C::Params` (a malformed payload answers `bad_request`
for you) and encodes the returned
`C::Result`; the handler reaches collaborators through `ctx.runtime.*()` and returns a `ServiceError` for a typed
failure.

```rust
// crates/daemon/src/services/instance.rs — inside register()
use proto::instances::{InstanceList, InstanceListResult};

on.handle::<InstanceList, _, _ > ( | _: Empty, ctx| async move {
Ok(InstanceListResult {
instances: ctx.runtime.engine().instances().list(),
})
});
```

A brand-new domain adds `mod <domain>;` plus one `<domain>::register(&mut on);`
line to `services/mod.rs` — the only change `make_router()` ever needs. A handler that touches an entry resolves it
through `guards::server_for` / `guards::instance_for` with an `Intent`, which applies every exclusion that intent
implies — never a checklist of its own.

Map engine errors to codes with `ServiceError::not_found` / `bad_request` /
`handler_error`. For a long-running operation, make the start contract a `Job`
in `proto`:

```rust
impl Contract for JavaInstall {
    const CHANNEL: &'static str = "java.install";
    type Params = JavaInstallParams;
    type Result = JobStarted;
}

impl Job for JavaInstall {
    type Progress = JavaInstallProgress;
    type Done = JavaInstallDone;
}
```

Then follow `JavaInstallManager` (`runtime/managers/java.rs`): hand
`Runner::start::<JavaInstall>` a `Spec` and a closure returning `JavaInstallDone`;
the runner publishes the `job` events (progress, done, failed, cancelled) with
the contract's channel, and the handler answers `JobStarted { id }`. A manager
that admits one job per entry takes its key from `InFlight`
(`runtime/managers/job.rs`), whose `claim()` guard releases on drop. Document
on the contract what a cancel leaves behind.

---

## Add a client facade method

Facade methods are one-liners over `Session::call::<C>()` that return `proto` types directly
(`crates/client/src/facades/<domain>.rs`).

```rust
pub struct Instance<'a> {
    pub(crate) session: &'a Session,
}

impl Instance<'_> {
    pub async fn list(&self) -> Result<Vec<proto::instances::Instance>, IpcError> {
        Ok(self
            .session
            .call::<proto::instances::InstanceList>(&proto::Empty {})
            .await?
            .instances)
    }
}
```

If the domain is new, add its module and `pub use` to `facades/mod.rs`, the accessor on `Client`
(`crates/client/src/lib.rs`), and the export from the crate root. Use `try_call` when a `not_found` should surface as
`None`, and `run_job::<C>` to block on a job — a facade method is that call and
nothing else (see `Java::install`). A long call is long on its contract (`Contract::TIMEOUT`), not at the call site.

---

## Add a CLI command

Commands are clap `Subcommand` enums, one module per domain under
`crates/cli/src/commands/`, dispatched from `main.rs`. Commands **never print directly** — they build a `View` and hand
it to `ui::show`.

**1. The module** (`crates/cli/src/commands/example.rs`):

```rust
use anyhow::Result;
use clap::Subcommand;

use crate::commands::connect;
use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum ExampleCmd {
    /// List instances
    List,
}

pub async fn run(cmd: ExampleCmd) -> Result<()> {
    match cmd {
        ExampleCmd::List => list().await,
    }
}

async fn list() -> Result<()> {
    let client = connect().await?;
    let instances = client.instance().list().await?;
    if instances.is_empty() {
        return ui::show(View::note("no instances yet"));
    }
    ui::show(View::table(
        "Instances",
        ["ID", "NAME"],
        instances.into_iter().map(|i| vec![i.id, i.name]).collect(),
    ))
}
```

Use `connect()` for a command that needs a running daemon (it never spawns one), or `start()` for the deliberate
`daemon start` path that spawns `hestiad`. Build
`View::line` / `note` / `detail` / `table`; call
`ui::select` for an interactive pick (it errors when stdin is not a terminal, so offer an argument as the fallback), and
`ui::human_bytes` for sizes.

Once a domain grows past a handful of verbs, make it a directory: `mod.rs` keeps the `Subcommand` enum and the dispatch,
and each verb group gets its own file — as `commands/server/` (`create`, `update`, `backup`, `config`, `lifecycle`,
`console`) and `commands/instance/` do, each over an `entry` module for the select/render helpers they share.

**2. Wire it in** `crates/cli/src/main.rs`:

```rust
#[derive(Subcommand)]
enum Command {
    // ...
    /// Minecraft instances
    Instance {
        #[command(subcommand)]
        cmd: commands::instance::InstanceCmd,
    },
}

// in dispatch():
Command::Instance { cmd } => commands::instance::run(cmd).await,
```

Add `pub mod instance;` to `crates/cli/src/commands/mod.rs`.

---

## Add a desktop API method

The desktop's Rust side is a fixed, generic bridge (`crates/desktop/src/bridge.rs`
— one `ipc_call` command over the shared client, plus event forwarding); it never grows per feature. A desktop feature
is TypeScript in `frontend/src/api/` (and, usually, a hook in `frontend/src/queries/`) — the desktop's equivalent of a
client facade method. See [the desktop front-end](architecture/frontends.md#desktop--hestia-desktop) and
[decision 0046](decisions/0046-desktop-bridge-is-one-generic-command.md);
[hooks.md](hooks.md) is the usage guide for *consuming* the queries layer (patterns, the job store, the full hook
inventory).

**1. The typed function**, in the domain's module (`frontend/src/api/<domain>.ts`). Payload types are **generated** from
`proto` by ts-rs into
`frontend/src/api/types/` (a flat `generated/` dir plus one per-module barrel); run `scripts/gen-types.sh` after
changing a wire type or a contract. They carry the proto names verbatim (camelCase fields).

The same script writes `api/types/channels.ts`, a projection of every `impl Contract`, `impl Job` and `impl Topic` in
`crates/proto`: `Channels` maps each channel to its params and result, `Jobs` to its progress and done payloads, `Topics`
to its event payload, and `TIMEOUTS` carries the contracts that override `TIMEOUT`. `call`, `tryCall`, `runJob`,
`onDaemonEvent` and the browser mock are checked against it, so a function names a channel and the compiler supplies the
rest — no `call<{ … }>` assertion, no timeout:

```ts
// frontend/src/api/instance.ts
export async function list(): Promise<InstanceInfo[]> {
    const result = await call("instance.list");
    return result.instances;
}
```

A request struct with a struct-level `#[serde(default)]` is typed `Partial<…>` in the table (and so are any such structs
nested in it): the daemon fills what is omitted, so the API function may too. Params that are `Empty` or all-default can
be left out of the call entirely. Use `tryCall` when a `not_found` should surface as `null`, and wrap a
progress-streaming operation in `runJob(channel, params, job)` (`core/jobs.ts`), which adds the job id to the params,
starts the channel and resolves with its `Done` payload — see `server.create` or `java.install`. Add a fixture for the
channel to `src/mock/channels/` only when a screen needs it; it is typed against the same table.

**2. The query bindings**, in the domain's `frontend/src/queries/<domain>.ts` — a factory exists when a screen uses
it, so a channel nothing renders gets no factory, key or mock handler until a screen needs it. A read is a `queryOptions` maker in `<domain>Queries` (its key from `keys.ts` —
per-entry keys take the **stable id**, never the display name, and nest under `detail(id)` so one sweep refreshes the
whole entry). A write is a `mutation({ mutationKey, mutationFn, invalidates })` maker in `<domain>Mutations`, where
`invalidates` declares the key prefixes swept on settle. Components pass these to TanStack's own
`useQuery`/`useMutation`; add a named hook only when a factory cannot express it (a read composing two sources, or one
accumulating live events):

```ts
// frontend/src/queries/instance.ts
export const instanceQueries = {
    list: () =>
        queryOptions({ queryKey: keys.instances.list(), queryFn: () => api.list() }),
};

export const instanceMutations = {
    rename: (id: string) =>
        mutation<InstanceInfo, string>({
            mutationKey: [ ...keys.instances.detail(id), "rename" ],
            mutationFn: (name) => api.rename(id, name),
            invalidates: () => [ keys.instances.all ],
        }),
};
```

```tsx
// at the call site
const { data: instances } = useQuery(instanceQueries.list());
const rename = useMutation(instanceMutations.rename(id));
```

A progress-streaming operation is a `jobMutation({ mutationKey, meta, run,
invalidates })` consumed through `useJobMutation` — the run registers in the global job store (so `useJobs`/
`useEntryJobs` and any activity surface see it, outliving the component) and the hook's result adds `progress`/`job` for
the inline case; see `serverMutations.create` or `javaMutations.install`. Factories are the source of truth: a route
loader preloads through them (`context.queryClient.ensureQueryData(serverQueries.list())`). If a daemon event should
refresh a query, add its topic to the map in `queries/invalidation.ts` — for a job, its start channel to the done (or
failed) table.

A brand-new domain adds one module file per layer plus its `export * as <domain>`
line in `frontend/src/api/index.ts` — nothing in the Rust shell changes.

---

## Add a user-facing string

Every string the desktop renders is a message (paraglide/inlang), reached as
`m['<root>.<path>']()`. Messages live in `frontend/messages/{locale}/<root>.json`, one file per root, and there are four
roots — put the string where it is *rendered*, not where it happens to be defined:

| Root                    | What goes in it                                                                                                                                         |
|-------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------|
| `app.*`                 | shell chrome and shared vocabulary — `nav`, `window`, `action`, `label`, `status`, `toast`, `search`, `time`, `jobs`, `validation`, `daemon`            |
| `<feature>.*`           | one root per `frontend/src/features/` directory — `library`, `entry`, `server`, `instance`, `content`, `skin`, `settings`, `account` |
| `domain.*`              | vocabulary mirroring a `proto` enum — content kinds, flavors, gamemodes, difficulties, provision phases, entry types                                    |
| `error.*` / `warning.*` | the daemon's own `ErrorInfo`/`WarningInfo` vocabulary, keyed by variant (`kind`, `code`, `token`, `hint`)                                               |

`entry.*` is what a server and an instance share (the create wizard, per-entry settings, the stop dialogs); a string
only one side renders goes in `server.*` or
`instance.*`. Nest with objects rather than underscores — `entry.settings.remove.title`, not
`entry_settings.remove_title` — since paraglide flattens a dotted key to underscores and the two spellings compile to
one identifier.

Add the key to **every** locale, and run `bun run test`: the guard (`frontend/tests/messages.test.ts`) fails a locale
that has fallen behind, a translation whose `{placeholders}` differ, a referenced key that does not exist, and a defined
key with no call site. A key reached dynamically (``m[`error.kind.${kind}`]``) has no literal call site, so its table
must be listed in that test's `DYNAMIC_PREFIXES` — otherwise the whole table reads as dead.

---

## Build & run

The core loop needs no webview or frontend deps:

```bash
cargo build -p cli -p daemon
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Iterate interactively with `scripts/dev.sh` — it opens a subshell with `hestia`
and `hestiad` on `PATH` (debug builds keep data in `target/<profile>/data`), or
`scripts/dev.sh java list` for a one-shot. The daemon is never auto-spawned — start it with `hestia daemon start` (or
login autostart); commands error pointing there when it is down. `hestia daemon status|start|stop|restart`
manages it explicitly.

The desktop app needs the system webview (WebKitGTK on Linux, WebView2 on Windows)
and the Bun-built frontend:

```bash
(cd frontend && bun install && bun run build)
scripts/dev.sh --desktop        # Tauri shell with frontend HMR
```

The frontend's own checks — the same ones CI's `frontend` job runs, in this order:

```bash
cd frontend
bun run generate:messages   # src/paraglide/ is generated and untracked
bun run check               # biome: lint + format
bun run typecheck           # tsc --noEmit
bun run test                # vitest
bun run build
```

`generate:messages` comes first because the app imports from `src/paraglide/`,
which is compiled from `messages/` and never committed — a fresh checkout has no
such directory, so a typecheck or build before it fails on unresolved imports.
(`vite dev`/`vite build` run the same compile through the paraglide plugin;
the script is what makes it available on its own.) `generate:routes` regenerates
`routeTree.gen.ts`, which *is* committed — run it after adding a route.

See [packaging.md](packaging.md) for installers and sidecar bundling.

## Git hooks

The hooks live in [`.husky/`](../.husky) and are tracked, so they are the same
for everyone. `husky-rs` (a dev-dependency of `common`) points `core.hooksPath`
at that directory from its build script, so **one `cargo test` installs them** —
there is nothing to run by hand. `NO_HUSKY_HOOKS=1` skips the install, which is
what CI sets.

| Hook | What it does |
|---|---|
| `pre-commit` | refuses staged conflict markers, then `cargo fmt` and `biome check --write` over the *staged* files, re-staging what they rewrite; runs the message-catalogue test when `frontend/messages/` is touched |
| `commit-msg` | enforces `<type>(<scope>): <description>`, exempting git's own merge/revert/fixup subjects |
| `pre-push` | the CI gates: `fmt --check` + `clippy -D warnings` + `test` for Rust, and the full Bun chain for the frontend |

The split is by cost. A commit pays for formatters and millisecond greps; a push
pays for clippy and the test suites. `pre-push` reads the ranges being pushed and
runs only the side that changed, so a docs-only push is instant, and it skips the
`desktop` crate on purpose — that one needs the system webview and a staged
sidecar set, so CI covers it in its own job rather than every push paying for it.
A hook stricter than CI would block pushes CI would pass; looser is fine.

Either hook takes `--no-verify` when you need to bypass it
(`git commit --no-verify`, `git push --no-verify`).

## Recording a decision

When a non-trivial architectural choice is made, write it down in
[decisions/](decisions/README.md) — what changed, why, and what you rejected —
and link it from the subsystem page in [architecture/](architecture.md) it explains. The architecture pages stay a
description of the system; the reasoning lives beside them, not in commit messages or chat logs.
