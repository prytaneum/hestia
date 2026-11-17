# Contributing

Conventions and copy-and-adapt recipes. Read [architecture.md](architecture.md) first, then the page for whatever you
are touching.

## Conventions

- `rustfmt` and `clippy -D warnings` stay clean; CI also runs `cargo-deny`.
- Wire-in is one line per seam (table below). Adding a feature never touches the serve loop or the transport.
- One concept per file. An aggregation point (the daemon's router, the client's facades, the engine aggregate) is a
  module directory, never one growing file.
- Single-word module names (`config`, not `config_store`).
- Errors: `thiserror` enums in libraries, mapped to an `ErrorInfo` variant at the daemon's service boundary (handlers
  return `ServiceResult`); `anyhow` at binary edges and multi-step engine operations. No panics on recoverable errors.
- Logging: `tracing`. Never log tokens or secrets.
- Identity (name, version, id) comes from `common::app`; do not hard-code it.
- Validate external data where it enters: payloads decode through the contract, the config schema rejects unknown keys,
  client-supplied paths are checked for escape.

## The wire-in map

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

One module per domain in `crates/proto`; add `pub mod <domain>;` to `lib.rs`. A contract names its channel once and pairs
it with `Params`/`Result`.

```rust
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
}

pub struct InstanceList;
impl Contract for InstanceList {
    const CHANNEL: &'static str = "instance.list";
    type Params = Empty;
    type Result = Vec<Instance>;
}
```

Rules:

- **Camel case on the wire.** Every fielded struct carries `#[serde(rename_all = "camelCase")]`
  (`tests/casing.rs` enforces it). Enum variant values keep their existing `snake_case`/`lowercase` form. The one
  kebab-case vocabulary is the `config.*` keys.
- **`#[serde(default)]` on payloads**, so a peer that omits a field still decodes.
- **Only an `Option` is skipped when absent** (`skip_serializing_if = "Option::is_none"`). The generated TypeScript
  declares every other field present, so an omitted list would arrive `undefined` and throw when iterated
  (`tests/omission.rs` enforces it).
- **A result that is one list or one struct is that value, bare.** It stays a struct when it carries a second field, is
  an `Option`, `bool` or `Value`, or is a list on a mutating channel that may report `warnings`.
- **A slow handler says so on its contract:** `const TIMEOUT: Duration = Duration::from_secs(120);`, written as
  `Duration::from_secs(<int> [* <int>])` because `scripts/lib/gen-channels.py` reads it. The default is 10 s.
  `Session::call` and the desktop bridge both apply it; a job's start channel keeps the default.
- **A call that mutates state another client renders** sets `const CHANGES: Option<Scope> = Some(Scope::…)`; the daemon
  then publishes `changed`, and the desktop maps the scope in `queries/invalidation.ts`.
- **A daemon-to-client push** implements `Topic` (the type is its own event payload). A long-running operation is a
  `Job` (see [Add a daemon channel](#add-a-daemon-channel)).
- **Changing an existing payload** means updating `crates/proto/tests/` (`wire` and `golden` are pinned on purpose).

---

## Add an engine domain

Launcher logic lives in `crates/engine` and is daemon-internal. Write the subsystem as `crates/engine/src/<domain>/`,
taking its path under the data home as a plain `PathBuf` and guarding only mutable state with a `Mutex`. Add `mod` and
`pub use` to `lib.rs`, then hang it off `Engine` (`engine/mod.rs`): a field, a getter, construction in `new()`. That is
the only wiring change.

```rust
pub struct Instances { dir: PathBuf }

impl Instances {
    pub fn new(dir: PathBuf) -> Self { Instances { dir } }
}

// engine/mod.rs: field `instances`, built in new() with data_home.join("instances"), and
pub fn instances(&self) -> &Instances { &self.instances }
```

- A flow that spans subsystems (provisioning, launching, backups, content) is not a method on the aggregate. It goes in
  `engine/flows/<concern>.rs` as an `impl Engine` block.
- A subsystem with an open set of implementations (a content platform, an archive format) gets a trait plus a registry
  list, so adding one is a module and a line in that list. Express what needs the aggregate as data the flow matches on.
- The flavor, content, Java and pack seams are open to providers registered at runtime under an owner (see
  [Minecraft providers](architecture/minecraft.md), [content](architecture/content.md) and
  [transfer](architecture/transfer.md)). Tests drive them with the native stand-ins in `crates/engine/tests/fixture/`,
  so a flow runs without the network. Flavors, content sources, pack formats and Java distributions beyond the
  built-ins return with the new plugin system.

## Persist something, or change its shape

Anything of the user's that the engine writes is a `schema::Document`; `schema::load`/`save` handle the schema stamp,
the temp-file write and quarantine of a file this build cannot read.

```rust
impl Document for InstanceRecord {
    const NAME: &'static str = "instance.json";
}
```

For a record under an entry directory that is all: `registry::read_record`/`write_record`/`scan` take the name from
`NAME`. A document with its own path (settings, accounts) calls `schema::load`/`save` directly. An additive field needs
no migration (`#[serde(default)]`); there is no migration chain until a released document first needs one.

---

## Add a daemon channel

One `on.handle::<C>(…)` in the domain's `register()` (`crates/daemon/src/services/<domain>.rs`). The registrar decodes
`C::Params` (a malformed payload answers `bad_request`) and encodes the result; the handler reaches collaborators through
`ctx.runtime.*()` and returns an `ErrorInfo` variant for a typed failure.

```rust
on.handle::<InstanceList, _, _>(|_: Empty, ctx| async move {
    Ok(ctx.runtime.engine().instances().list())
});
```

A new domain adds `mod <domain>;` and `<domain>::register(&mut on);` to `services/mod.rs`. A handler that touches an
entry resolves it through `guards::server_for` / `guards::instance_for` with an `Intent`; never write an exclusion
checklist of your own.

**Long-running work is a job.** Make the start contract a `Job` in `proto`:

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

Then add a `start_*` function on `Jobs` (model: `Jobs::start_java_install` in `runtime/managers/java.rs`) that hands
`Jobs::run::<JavaInstall>` a `Spec` and a closure returning the `Done` payload. The runner publishes the `job` events;
the handler answers `JobStarted { id }`. A job that changes an entry takes its claim in the handler
(`guards::server_held` / `instance_held` / `entry_held`) and passes it in the `Spec`. Document on the contract what a
cancel leaves behind.

## Add a client facade method

A one-liner over `Session::call::<C>()` in `crates/client/src/facades/<domain>.rs`, returning `proto` types directly.

```rust
impl Instance<'_> {
    pub async fn list(&self) -> Result<Vec<proto::instances::Instance>, IpcError> {
        self.session.call::<proto::instances::InstanceList>(&proto::Empty {}).await
    }
}
```

A new domain adds its module and `pub use` to `facades/mod.rs`, an accessor on `Client` (`crates/client/src/lib.rs`) and
a root export. Use `try_call` when `not_found` should surface as `None`, and `run_job::<C>` to block on a job. The
timeout lives on the contract, never at the call site.

## Add a CLI command

One module per domain under `crates/cli/src/commands/`. Commands never print directly: they build a `View`
(`line`, `note`, `detail`, `table`) and hand it to `ui::show`.

```rust
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
    ui::show(View::table(
        "Instances",
        ["ID", "NAME"],
        instances.into_iter().map(|i| vec![i.id, i.name]).collect(),
    ))
}
```

- `connect()` needs a running daemon and never spawns one; `start()` is the deliberate `daemon start` path.
- `ui::select` is an interactive pick and errors off a terminal, so offer an argument as the fallback.
- Past a handful of verbs, make the domain a directory: `mod.rs` keeps the `Subcommand` enum and dispatch, each verb
  group gets a file (see `commands/server/`).
- Wire in with a variant on `Command` in `main.rs` (`Command::Instance { cmd } => commands::instance::run(cmd).await`)
  and `pub mod <domain>;` in `commands/mod.rs`.

---

## Add a desktop API method

The desktop's Rust side is one generic bridge (`crates/desktop/src/bridge.rs`: an `ipc_call` command plus event
forwarding) and never grows per feature. A feature is TypeScript: a typed function in `frontend/src/api/` and,
usually, a factory in `frontend/src/queries/`. See [the front-ends page](architecture/frontends.md).

**1. The typed function** in `frontend/src/api/<domain>.ts`. Payload types are generated from `proto` by ts-rs into
`frontend/src/api/types/`; run `scripts/gen-types.sh` after changing a wire type or contract. The same script writes
`api/types/channels.ts`: `Channels` maps each channel to its params and result, `Jobs` to progress and done payloads,
`Topics` to event payloads, and `TIMEOUTS` carries overridden timeouts. `call`, `tryCall`, `runJob`, `onDaemonEvent` and
the browser mock are checked against it, so a function names a channel and the compiler supplies the rest:

```ts
export function list(): Promise<InstanceInfo[]> {
    return call("instance.list");
}
```

A request struct with a struct-level `#[serde(default)]` is typed `Partial<…>`, so params that are `Empty` or all-default
can be left out. Use `tryCall` when `not_found` should surface as `null`; wrap a progress-streaming operation in
`runJob(channel, params, job)` (`core/jobs.ts`). A brand-new domain adds its module and an `export * as <domain>` line
in `api/index.ts`. Add a fixture to `src/mock/channels/` only when a screen needs it.

**2. The query bindings** in `frontend/src/queries/<domain>.ts`. A factory exists only when a screen uses it. A read is a
`queryOptions` maker in `<domain>Queries` (key from `keys.ts`); a write is a `mutation({ mutationKey, mutationFn,
invalidates })` maker in `<domain>Mutations`.

```ts
export const instanceQueries = {
    list: () => queryOptions({ queryKey: keys.instances.list(), queryFn: () => api.list() }),
};

export const instanceMutations = {
    rename: (id: string) =>
        mutation<InstanceInfo, string>({
            mutationKey: [...keys.instances.detail(id), "rename"],
            mutationFn: (name) => api.rename(id, name),
            invalidates: () => [keys.instances.all],
        }),
};
```

If a daemon event should refresh a query, add its topic to the map in `queries/invalidation.ts` (for a job, its start
channel to the done or failed table).

### Consuming the queries layer

`frontend/src/queries/` holds `client.ts` (the `QueryClient`), `keys.ts` (hierarchical keys), `core.ts` (`mutation()`),
`jobs.ts` (the job store, `jobMutation()`/`useJobMutation`), `invalidation.ts`, `connection.ts`, `events.ts`, `logs.ts`
and one `<domain>.ts` per domain. Import from the barrel: `import { serverQueries, serverMutations, keys } from '#/queries'`.

Factories are options objects, not hooks; pass them to TanStack's own `useQuery`/`useMutation`. The same definition then
serves a component, a route loader and an imperative fetch. Named `use*` hooks exist only where a factory cannot
express it (a read composing two sources, or one accumulating live events).

- **Per-entry keys take the stable `id`**, never the display name, and nest under `detail(id)` so one sweep refreshes
  the entry.
- **Errors are `HestiaError`** (`error.code` is the daemon's code vocabulary). No retries: the daemon is a local socket,
  so an error is real the first time. An errored query clears when something invalidates it.
- **Freshness is automatic.** Mutations invalidate on settle and `invalidation.ts` sweeps on daemon events; components
  never call `invalidateQueries`.

```tsx
const { data: servers, isPending, error } = useQuery(serverQueries.list());
const rename = useMutation(entryMutations.rename('server', id));
rename.mutate('Cozy SMP');          // variables are what the verb needs beyond the bound entry
```

**Jobs.** Anything that streams progress is a job mutation. Every run lands in the global job store whether or not the
component stays mounted (`useJobs()`, `useBusyJob(kind, id)`, `useEntryJobs(kind, id)`). `useJobMutation` adds the
local `progress` and `job` for the run this call site started. A dialog that renders a job's progress claims it with
`useJobDisplay(job, shown)` so the status bar does not show it twice.

```tsx
const create = useJobMutation(serverMutations.create());
create.mutate(form, { onSuccess: (server) => navigate({ to: `/servers/${server.id}` }) });
// create.isPending, create.progress?.phase, create.progress?.current / total
```

The store reports a job's outcome once: a failure toasts with Retry on its own, and a success toasts only what the
factory's `notify.done(done, variables)` returns, or nothing when it returns `null`. Never toast a job outcome at the
call site, and never import `sonner` outside `lib/notify.ts` (biome refuses it); a write the user is looking at confirms
inline with `useSaveState`. A new job channel needs `app.jobs.kind.<channel>` and `app.jobs.verb.<channel>` in every
locale.

**Live data.** `useConnection()` reports `'connected' | 'disconnected'` (the layer invalidates everything on reconnect).
`useServerLogs(id, { tail, follow })` and `useInstanceLogs(id, { session, follow })` merge the fetched tail with
`process.output` events (`lines`); following is scoped to the entry, so leave it on while stopped. `useDaemonEvent(topic,
handler)` subscribes to a raw topic.

**Finding a factory.** Each `queries/<domain>.ts` is an object literal of named factories, and a job mutation is marked
by `jobMutation(...)`. A channel with no factory is one no screen uses yet.

---

## Add a user-facing string

Every string the desktop renders is a paraglide message, `m['<root>.<path>']()`, in
`frontend/messages/{locale}/<root>.json`. Put it where it is rendered:

| Root                    | What goes in it                                                                                          |
|-------------------------|----------------------------------------------------------------------------------------------------------|
| `app.*`                 | shell chrome and shared vocabulary: `nav`, `window`, `action`, `label`, `status`, `toast`, `jobs`, ...   |
| `<feature>.*`           | one root per `frontend/src/features/` directory (`entry` is what servers and instances share)            |
| `domain.*`              | vocabulary mirroring a `proto` enum: content kinds, flavors, gamemodes, entry types                      |
| `error.*` / `warning.*` | the daemon's `ErrorInfo`/`WarningInfo` vocabulary, keyed by variant                                      |

Nest with objects, not underscores (`entry.settings.remove.title`), since paraglide flattens dots to underscores. Add
the key to every locale and run `bun run test`: it fails a lagging locale, mismatched `{placeholders}`, a missing key
and a dead key. A key reached dynamically (``m[`error.kind.${kind}`]``) needs its table listed in `DYNAMIC_PREFIXES` in
`frontend/tests/unit/messages.test.ts`.

---

## Add a built-in plugin

A built-in is Hestia's own code registered through the plugin registry, so it can be turned off and others can depend on
it (see [the plugin kernel](architecture/plugins.md)). Implement `kernel::Builtin` and add it to `engine::builtins()`
(`crates/engine/src/builtin.rs`); that list is the only wiring.

```rust
pub struct Example;

impl Builtin for Example {
    fn descriptor(&self) -> Descriptor {
        Descriptor { id: "example", name: "Example", provides: Some("hestia:example"), requires: &[] }
    }
    fn table(&self) -> Option<Table> { Some(table()) }
    fn enable(&self) -> anyhow::Result<()> { Ok(()) }
    fn disable(&self) -> Result<(), Busy> { Ok(()) }
}
```

- `provides` names the WIT package it owns and needs a `table()`; a built-in that owns no package returns `None` from
  both. `requires` lists built-ins that must be enabled first. The registry refuses duplicate ids, a missing or cyclic
  requirement and a table that does not match `provides`.
- `enable` and `disable` run under the registry's lock and the router reads that lock on every owned call, so both only
  register or unregister and return at once, and never reach the registry. `disable` answers `Busy` while work is
  running; the registry then refuses to turn it off.
- Name the built-in as the owner of its contracts in `proto` with
  `const OWNER: Option<&'static str> = Some("example");`. The router answers `plugin_disabled` for them while it is off.

## Add a built-in's WIT package

A package is what lets a WASM plugin call a built-in. It lives in the kernel crate at `crates/kernel/src/api/<package>/`
(the model is `api/probe/`, the test built-in behind the `probe` feature); nothing in `engine` or `daemon` changes.

1. **The WIT**, `wit/<package>.wit`: `package hestia:<name>@<version>;`, one interface per permission (an interface with
   no permission is open), `use hestia:kernel/types@<kernel version>.{plugin-error}` for errors, and a `host` world
   (imports and the provider exports) plus a `full` world the sandbox's `bindgen!` reads. An addition after release is a
   new interface marked `@since`; see the [version policy](../crates/sdk/wit/README.md).
2. **The table**, `table.rs`: a `Table::new(interfaces, permissions)` mapping each interface to `Some(permission)` or
   `None`, with each `Permission`'s tier (`Sandboxed`, or `Unrestricted` for what reaches native code or deletes data),
   its scope kind (`ScopeKind::Entries` takes the user's entries scope) and prompt text, closed by
   `.versioned("hestia:<name>", "<version>")`. Mark `@unstable`/`@deprecated` interfaces with `.stability(...)`.
   `Builtin::table` returns it and `Plugins::new` merges it into the kernel's.
3. **The adapter**, `host.rs` and `convert.rs`: implement the generated `Host` trait of each interface on `State`. Each
   function takes a link (`self.link(Some(PERMISSION), (interface, function))`), converts the arguments to the
   contract's `Params`, calls `link.call::<C>(...)` (or `link.run_job::<C>(...)` for a job, which waits for `done`) and
   converts the result back. The router decides; the adapter only converts. `link()` adds the package's interfaces to
   the linker.
4. **The contracts** in `proto` implement `EntryAccess` (`touches` and `creates` list the `EntryTarget`s a call acts on
   or made; return an empty list on purpose when it does neither) and, for a job, `JobAccess`. The daemon needs no
   plugin-specific handler: `RouterCalls` routes the channel like any client call, after checking the owner is enabled
   and every touched entry is inside the plugin's scope.
5. **Wire it in** `api/mod.rs`: `mod <package>;`, and a line each in `link()` (the package's `link`) and
   `check_exports()` (its extension points, so a malformed export fails the load).
6. **Snapshot.** `crates/kernel/tests/wit.rs` lists the packages it checks (add the new one to `packages()`) and fails a
   changed WIT under an unchanged version. Append the printed `<package> <version> <hash>` line to
   `crates/sdk/wit/snapshot.txt`, and keep the package's `Table` version equal to the WIT's.

Importing the package, or exporting one of its interfaces, is then a requirement on the built-in: an install is refused
without it and it cannot be disabled while an enabled plugin needs it.

## Add an extension point

An extension point is an export interface the built-in's package defines for plugins to fill. Define it in the package's
WIT with one typed function, and add its row to the table: an exported interface maps to a permission like an imported
one, so exporting it needs that permission and appears in the grant. A topic whose permission depends on its payload is
split into one interface per permission.

- **A provider** (the built-in asks a plugin for an answer): write a type implementing `runtime::extension::Extension`
  (`budget()`, and `call()` that looks the export up with the generated `GuestIndices` and calls it), and a method on
  `Sandbox` that runs it through `Sandbox::extend` (see `Sandbox::fill` in `api/probe/exports.rs`). A component that
  does not export the interface answers `None`.
- **A topic** (the built-in tells plugins something happened): an `Extension` that is also a `registry::Point`
  (`interface()`, and `entries()` with the stable ids the event is about), and a method on `Plugins` that calls
  `self.publish(Arc::new(event))`. It reaches each enabled plugin that exports the interface, holds its permission and
  has every named entry in scope, through the plugin's inbox and event budget.
- **A hook** (a plugin may change or veto what the built-in is about to do): a `Point` that is also a `registry::Hook`
  (`vetoed()`), run with `self.fold(&hook, chain)` in plugin-id order, each seeing the previous result; a plugin that
  fails or times out is skipped, and only an explicit veto stops the rest. Hooks get the `Hook` budget, shorter than an
  event's.
- Register the export's type check in `check_exports()` so a wrong signature fails the load, not each call.

`api/probe/events.rs` and `exports.rs` are the working examples, driven by `tests/events.rs`. Neither the plumbing nor
the probe package is compiled without the `probe` feature, because no production package uses them yet.

---

## Build & run

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
```

The kernel's tests build their guest plugins for `wasm32-wasip2`; `rust-toolchain.toml` installs that target.

Run these at the root without `-p` (every member but `desktop` is a default member; `-p` re-resolves features and
compiles a second copy of the dependency tree). Narrow a test run by name: `cargo test content::`.

`scripts/run.sh` is the one way to run a development build. Every form builds first and uses the same dev daemon:

```bash
scripts/run.sh --shell          # a subshell with hestia and hestiad on PATH, daemon running (default)
scripts/run.sh --desktop        # the desktop app, with frontend hot reload
scripts/run.sh --daemon         # hestiad in the foreground
scripts/run.sh server list      # one `hestia` command
scripts/run.sh --release --desktop   # the optimised app, on its own data
```

Debug builds keep data in `target/debug/data` and scope their daemon endpoint to it, so nothing touches an installed
Hestia; `--release` uses `target/release/data`, and `--home=DIR` picks another. The `hestia` CLI never starts a daemon
itself (`hestia daemon status|start|stop|restart`).

The desktop needs the system webview, [Bun](https://bun.sh/) and `cargo install tauri-cli --version '^2'`. The
frontend's checks, in CI's order:

```bash
cd frontend
bun run generate:messages   # src/paraglide/ is generated and untracked; run first
bun run check               # biome
bun run typecheck
bun run test
bun run build
```

`generate:routes` regenerates the committed `routeTree.gen.ts`; run it after adding a route. See
[packaging.md](packaging.md) for installers.

## Git hooks

The tracked hooks in [`.husky/`](../.husky) are installed by one `cargo test` (`NO_HUSKY_HOOKS=1` skips it).

| Hook         | What it does                                                                                              |
|--------------|-----------------------------------------------------------------------------------------------------------|
| `pre-commit` | refuses conflict markers; `cargo fmt` and `biome check --write` over staged files; message test if `frontend/messages/` changed |
| `commit-msg` | enforces `<type>(<scope>): <description>`, exempting merge/revert/fixup subjects                          |
| `pre-push`   | `fmt --check`, `clippy -D warnings`, `test`, and the Bun chain, for whichever side changed (skips `desktop`) |

`--no-verify` bypasses either hook.
