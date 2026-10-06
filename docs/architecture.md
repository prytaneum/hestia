# Architecture

Hestia is a Minecraft launcher built as **one resident daemon with several thin
front-ends**. This page is the map: what the system is, how the pieces fit, and
where to read further. Each subsystem has its own page; the reasoning behind
every non-trivial choice lives in [decisions/](decisions/README.md).

- New to the codebase? Read this page, then the subsystem you're touching.
- Extending it? [contributing.md](contributing.md) has the copy-and-adapt recipes.
- Driving it? [cli.md](cli.md) is the command reference.

## The shape of it

The launcher core — accounts, downloads, Java runtimes, servers, instances,
content — is a Rust library called the **engine**. It is owned by a single
long-lived process, `hestiad`, and it is never linked into anything the user
looks at. Every front-end is a thin client that talks to that daemon over a
local socket.

```mermaid
flowchart LR
    subgraph clients["Front-ends (thin clients)"]
        CLI["<b>hestia</b><br/>CLI"]
        DESK["<b>hestia-desktop</b><br/>Tauri + React"]
        TRAY["<b>tray</b><br/>system tray"]
    end

    SOCK{{"local socket<br/><i>unix socket · named pipe</i>"}}

    subgraph daemon["hestiad (resident daemon)"]
        ROUTER["router + services"]
        ENGINE["<b>engine</b><br/>launcher core"]
        SUP["process supervisor"]
    end

    GAME(["Minecraft<br/>servers &amp; game sessions"])

    CLI  --> SOCK
    DESK --> SOCK
    TRAY --> SOCK
    SOCK <--> ROUTER
    ROUTER --> ENGINE
    ENGINE --> SUP
    SUP -.->|"spawned, then independent"| GAME
```

Three properties follow from that shape, and most of the design is downstream of
them:

- **One core, many faces.** Business rules exist once. A server created from the
  CLI is the same server the desktop lists, with no second implementation to
  drift.
- **The boundary is enforced by the compiler, not by discipline.** Only the
  daemon crate depends on the engine, so a front-end *cannot* reach launcher
  logic except over the wire. `cargo tree -i engine` shows exactly one consumer.
- **Workloads outlive the launcher.** A running server or game session is not a
  child the daemon kills on exit. Stopping one is always something you asked
  for — see [0034](decisions/0034-workloads-outlive-the-daemon.md).

| Front-end | Binary | Stack | What it is for |
|---|---|---|---|
| CLI | `hestia` | clap + ratatui | scripting and terminal use; the fully-wired reference front-end |
| Desktop | `hestia-desktop` | Tauri v2 + React/Vite | the visual surface — library, entries, content browse, skins |
| Tray | `hestia-tray` | tray-icon + tao | status and quick actions beside every serving daemon |

## The crate graph

One cargo workspace under `crates/*`. Arrows are dependencies; the absence of an
arrow is the guarantee.

```mermaid
flowchart TD
    proto["<b>proto</b><br/>wire contracts + domain types"]
    ipc["<b>ipc</b><br/>transport + JSON envelope"]
    common["<b>common</b><br/>identity · logging · paths"]
    client["<b>client</b><br/>typed SDK"]
    engine["<b>engine</b><br/>launcher core"]
    cli["<b>cli</b> → hestia"]
    daemon["<b>daemon</b> → hestiad"]
    desktop["<b>desktop</b> → hestia-desktop"]
    tray["<b>tray</b> → hestia-tray"]

    client --> proto & ipc & common
    ipc --> common
    engine --> proto & common
    cli --> client & common & proto
    daemon --> engine & proto & ipc & common & client
    tray --> client & common & ipc
    desktop -.->|"webview → bridge"| client

    classDef leaf fill:#eef,stroke:#88a
    classDef core fill:#efe,stroke:#8a8
    class proto,ipc,common leaf
    class engine core
```

- **`proto`** is the *what* — typed payloads, one definition per channel, shared
  verbatim by both sides. **`ipc`** is the *how* — framing, the envelope, where
  the socket lives. Neither knows anything launcher-specific; `ipc` reads
  `common` only to scope the endpoint to the running install
  ([0063](decisions/0063-an-endpoint-is-scoped-like-its-data-home.md)).
- **`client`** re-exports `proto`, so a front-end takes one dependency and gets
  both the SDK and the domain types.
- **`engine`** never links `ipc` or `client`. It does not know a socket exists —
  the equivalent of Tailscale's `LocalBackend`.
- **`daemon`** is the only crate linking `engine`. It also links `client`, purely
  so `hestiad ping` can talk to an already-running daemon.

## A request, end to end

Every interaction crosses the same seam. A plain call is request/response; a
long-running one answers immediately with a job id and streams progress.

```mermaid
sequenceDiagram
    autonumber
    participant F as Front-end
    participant S as client::Session
    participant D as hestiad router
    participant E as engine

    F->>S: client.server().list()
    S->>D: {v, channel:"server.list", payload, id}
    D->>D: decode C::Params · check peer · check account gate
    D->>E: engine.servers().list()
    E-->>D: Vec<ServerInfo>
    D-->>S: {v, ok:true, payload, id}
    S-->>F: typed proto result

    Note over F,E: long-running work is a job

    F->>S: client.server().create(…)
    S->>D: server.create
    D-->>S: { jobId }
    loop until terminal
        D--)S: server.create.progress
        S--)F: progress callback
    end
    D--)S: server.create.done | .error | .cancelled
```

## Where things live on disk

The data home separates what is *yours* from what the launcher can regenerate.
It resolves as `--home` → `$HESTIA_HOME` → a persisted pointer (`config set
home`) → the platform default (`~/.hestia`, `%APPDATA%\Hestia` on Windows).
Debug builds anchor at `target/<profile>/data` so development never touches your
real directory.

```
<data_home>/
├── config.json          settings
├── accounts.json        signed-in accounts (owner-only)
├── servers/<id>/        one directory per server   ── yours
├── instances/<id>/      one directory per instance ──
├── skins/               the skin library
├── shared/              the shared settings store (sync)
├── java/                installed Java runtimes
├── meta/                materialised game files — regenerable
│   ├── versions/ libraries/ assets/ natives/
│   └── spigot/          the shared BuildTools work tree
├── cache/               content-addressed download cache
├── processes/           supervisor records + captured logs
└── logs/                daemon, CLI and desktop logs; crash reports
```

Everything under `meta/` and `cache/` is derived and re-downloadable — one
obvious unit to reclaim ([0054](decisions/0054-meta-root-for-materialised-files.md)).

## Subsystem pages

| Page | Covers |
|---|---|
| [The socket boundary](architecture/wire.md) | `proto` contracts, the `ipc` envelope, the `client` SDK, jobs, events, cancellation, warnings |
| [Cross-cutting foundations](architecture/common.md) | app identity, logging, crash reports, path resolution |
| [The engine](architecture/engine.md) | the aggregate root, config, cache, downloads, Java, the process supervisor, self-update |
| [The daemon](architecture/daemon.md) | the serve loop, router, runtime, job managers, service registrars, autostart |
| [Minecraft providers](architecture/minecraft.md) | flavors, version catalogues, profile resolution, materialize, launch plans, RCON |
| [Servers & instances](architecture/entries.md) | entry stores, directory layout, provisioning, ports, backups, sync, worlds |
| [Content & modpacks](architecture/content.md) | content sources, the managed-dir install model, datapacks, modpacks |
| [Import & export](architecture/transfer.md) | archive formats, detection, what an archive carries, importing another launcher's instance |
| [Accounts & skins](architecture/accounts.md) | Microsoft sign-in, token rotation, the skin library |
| [Front-ends](architecture/frontends.md) | CLI grammar and presentation, the desktop bridge and query layer, the tray |

## Project status

Hestia is at its second beta (`v1.0.0-beta.2`), and the vertical slice is
complete: every capability below works end to end, from the wire contract to at
least one front-end.

**Working:** the workspace and its enforced dependency graph · logging, identity
and path resolution · the wire protocol and typed SDK · config, cache and
downloads · Java runtime management · Microsoft sign-in with token rotation ·
skin and cape management · the process supervisor · the provider layer (vanilla,
fabric, neoforge on both sides; paper, folia, spigot, bukkit for servers) ·
server management with an RCON console · instance management with concurrent
sessions · joining a world, a listed server or a Realm straight from a launch,
and going back to whichever was played lately · in-place
version updates both ways · server backups, on demand and
scheduled · content install and management from Modrinth, CurseForge, a URL or a
local file · modpacks into a new or existing entry · instance import and export
(hestia, `.mrpack`, Prism/MultiMC) · shared instance settings (`sync`) ·
self-update and
the announcement feed · network reachability as a first-class state, with
offline launch and cached version catalogues · the legacy client era, both its
unpacked natives and its named asset tree · the CLI over all of it · the desktop
shell and its pages · the system tray.

There is no known gap: the oldest launchable versions are covered, down to
their unpacked natives and their pre-hash asset layout.

## Conventions that hold everywhere

- **The disk is the registry.** Java runtimes, backups, servers, instances and
  finished processes are all discovered by scanning a directory, not by
  consulting a separate index that can disagree with it.
- **Write through a temp, rename to commit.** Downloads, backups, installs and
  every persisted record stage to `.part`/`.staging` and rename on success, so a
  failure leaves nothing half-written — and a restart reclaims anything abandoned
  ([0024](decisions/0024-temp-artifacts-are-reclaimed-at-startup.md)).
- **A document says which schema it is.** Everything of the user's on disk carries
  a `schemaVersion` and migrates forward as it is read; one this build cannot read
  is set aside, never overwritten
  ([0060](decisions/0060-a-managed-document-carries-its-schema-version.md)).
- **Validate at the edge.** Payloads decode through the contract; the config
  schema rejects unknown keys; paths from a client are checked for escape.
- **Wire-in is one line, in one place** — but that place is a module *directory*,
  never a growing file
  ([0031](decisions/0031-an-aggregation-point-is-a-directory.md)).
- **Errors are typed.** `thiserror` enums in libraries, mapped to an
  `ipc::errors` code at the service boundary; `anyhow` at binary edges and for
  multi-step engine operations. A degraded-but-successful outcome rides on the
  result as a structured warning, never only in the log
  ([0026](decisions/0026-degraded-outcomes-ride-on-the-result.md)).
- **Never log tokens or secrets.** Access tokens and RCON passwords never reach a
  log line, including the `-vv` wire trace, which reports frame sizes rather than
  contents. A launch must put the account's token in the game's argv, so that one
  is masked everywhere it would outlive or travel beyond the session — the
  tombstone kept for post-mortem, and the `ProcessInfo` a client renders. Only
  the live record keeps it, because an `on-failure` restart relaunches from those
  args; it is owner-only and gone when the process ends. Captured process logs
  are owner-only too: an old client prints its own session id to stdout.

## Tech stack

**Rust** (edition 2021), a cargo workspace kept clean under `rustfmt`,
`clippy -D warnings` and `cargo-deny`.

| Area | Crates |
|---|---|
| Async + transport | [tokio](https://tokio.rs/) |
| Serialization | [serde](https://serde.rs/) / serde_json |
| Logging | [tracing](https://github.com/tokio-rs/tracing), flexi_logger |
| HTTP | [reqwest](https://github.com/seanmonstar/reqwest) (rustls) |
| Crypto | [p256](https://github.com/RustCrypto/elliptic-curves) (Xbox proof keys), sha1/sha2, minisign verification |
| Archives | `tar`+`flate2`, `zip` — all in-process, no shelling out |
| Game data | [fastnbt](https://github.com/owengage/fastnbt) — reading a world's `level.dat` |
| CLI | [clap](https://github.com/clap-rs/clap) + [ratatui](https://ratatui.rs/) |
| Desktop | [Tauri v2](https://tauri.app/) + [React](https://react.dev/)/[Vite](https://vitejs.dev/), built with [Bun](https://bun.sh/); [three.js](https://threejs.org/) for skin previews |
| Tray | [tray-icon](https://github.com/tauri-apps/tray-icon) + [tao](https://github.com/tauri-apps/tao) |

## Testing

| Where | What it pins |
|---|---|
| `crates/proto/tests/` | `wire` and `golden` — the envelope and contract encodings, so a wire change is caught |
| `crates/engine/tests/` | `store` (config/cache/java/server/instance persistence), `auth_oracle` (the sign-in state machine), `process` (tree termination), `flows` (provisioning and content install against fixture providers, over `Engine::over`) |
| `crates/engine/src/**` | unit tests beside the code: archive detection and round trips, the Prism component mapping, the export ignore rules, launch-plan assembly (including the quick-play target and its version gate), document schema stamping, migration and quarantine, the `servers.dat` round trip, server-address parsing, the Log4Shell-safe session config, sync reconciliation and its per-key merge, Modrinth and CurseForge mapping, pack-format detection and `.mrpack`/URL parsing, version picking, per-flavor accepted kinds, JVM-args precedence, PaperMC and SpigotMC catalogue parsing |
| `crates/client/tests/` | the SDK against a scripted daemon over an in-memory duplex: contract marshalling, typed failures, timeouts, a version mismatch, and every way a job settles |
| `crates/daemon/tests/e2e.rs` | a client-to-daemon round trip over a real socket |
| `frontend/tests/` | the message catalogue (locale coverage, placeholder parity, no dead or missing keys), the export dialog's tree/exclusion conversions, and the quick-play version gate |

```bash
cargo build -p cli -p daemon                              # the fast core
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The frontend carries its own chain. `src/paraglide/` is generated and untracked,
and the app imports from it, so the messages compile before anything that
resolves those imports:

```bash
cd frontend
bun install
bun run generate:messages   # src/paraglide/ — required before typecheck/build
bun run check               # biome: lint + format
bun run typecheck           # tsc --noEmit
bun run test                # vitest
bun run build
```

CI (`.github/workflows/ci.yml`) runs both sides: `check` (fmt, clippy, test on
Linux and Windows), `frontend` (the chain above), `desktop` (clippy and tests for
the Tauri crate `check` excludes), and `deny`. The release workflow calls the
same file as its gate — see [packaging.md](packaging.md#ci).

## Recording a decision

When you make a non-trivial architectural choice, write it down in
[decisions/](decisions/README.md) — what changed, why, and what you rejected —
and link it from the subsystem page it explains. The architecture pages stay a
description of the system; the reasoning lives beside them where it can be read
on purpose rather than stumbled over.
