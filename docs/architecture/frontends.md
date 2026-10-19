# Front-ends

*[← Architecture](../architecture.md)*

Three ways to drive one daemon. None of them links the engine; all of them go
through [`client`](wire.md#client--the-typed-sdk) over the socket.

```mermaid
flowchart TD
    D["hestiad"]
    CLI["<b>hestia</b><br/>clap + ratatui<br/><i>scriptable, interactive when it can be</i>"]
    DESK["<b>hestia-desktop</b><br/>Tauri v2 shell + React<br/><i>the visual surface</i>"]
    TRAY["<b>tray</b><br/>tray-icon + tao<br/><i>always there when the daemon is</i>"]

    CLI <--> D
    DESK <--> D
    TRAY <--> D
    D -->|spawns on serve| TRAY
    TRAY -->|left-click / Open| DESK
    DESK -->|starts at launch| D
```

Each front-end declares what it means by an ambiguous action rather than
inheriting a wire default: the CLI asks before stopping a daemon with workloads
running, while the tray and desktop leave workloads running, because a menu item
cannot ask ([0036](../decisions/0036-stopping-the-daemon-has-three-meanings.md)).

---

# CLI — `hestia`

A thin client built on clap's derive API. `main.rs` defines a `Command` enum;
each variant is a module under `commands/` exposing a `Subcommand` and a `run()`.
A domain with many verbs is a directory whose `mod.rs` holds only grammar and
dispatch.

The full user-facing reference is [cli.md](../cli.md); this covers how it is
built.

## Grammar

Noun-first and **entry-first**: catalogue verbs read `hestia server
create|list|versions|flavors`, but anything acting on a specific entry names it
once, right after the noun.

```
hestia server smp start
hestia server smp config set memory 4G
hestia server smp backup create
hestia instance modded mod add sodium
```

The name occupies one fixed slot instead of floating to a different position per
subcommand. clap models this with an `external_subcommand` variant on the noun's
`Subcommand`: an unrecognised first token — the entry name — is captured and
re-parsed by a `no_binary_name` parser, so per-entry actions keep full clap help
and validation while catalogue verbs stay ordinary subcommands.

Two cross-cutting shortcuts sit on top, earned by how often they are used:

- `hestia play [instance]` — the launcher's single most common action, picking
  interactively when several instances exist.
- verb-first `hestia start|stop|restart|logs|rename <name>` — resolves a name
  across *both* registries and dispatches, so day-to-day driving need not recall
  which kind an entry is, nor that `server start` and `instance launch` differ.

Everything scriptable still has an explicit noun-first form
([0040](../decisions/0040-entry-first-cli-grammar.md)).

Anything a `create` needs but wasn't given is asked for interactively — on a
terminal the picker *is* the browser; piped invocations error naming the flag to
pass, so scripts stay explicit.

**Every daemon capability gets a scriptable verb, or a written reason it has
none.** Skins are the documented exception — a visual surface. A channel with no
CLI verb is fine when the architecture says why, and a bug when it does not
([0041](../decisions/0041-every-capability-gets-a-verb.md)).

## Connection and flags

Global `--verbose`/`--quiet`/`--home` sit on the root. `--home` is exported as
`$HESTIA_HOME` and only takes effect when `hestia daemon start` spawns the
daemon — a running daemon keeps its own directory.

No command auto-spawns: `connect()` and `connect_running()` require a running
daemon; `start()`, behind `daemon start`, is the one that spawns it.

`-vv` buys **wire visibility** rather than more volume — every frame with its
channel, correlation id, size and round-trip time. Payloads are never logged
([0042](../decisions/0042-vv-buys-wire-visibility.md)).

## Exit codes

A state query answers through its exit code, in systemd's vocabulary
(`cli/src/exit.rs`):

| Code | Meaning |
|---|---|
| 0 | did what was asked / the subject is running |
| 3 | answered, and the subject is **not** running |
| 1 | the command failed |
| 2 | usage (clap's own) |

Only the verbs that *assert* one subject's running-ness produce 3 — `daemon
status`, `server <name> status`. Verbs that merely describe stay 0, because
"inactive" is not a claim they make
([0043](../decisions/0043-state-queries-answer-through-exit-codes.md)).

## Presentation

Commands **never print directly**. They build a `View` (`Line`, `Note`,
`Detail`, `Table`, `Warning`) and hand it to `ui::show`, which owns all output.

Interactive surfaces run as **fullscreen sessions** on a small framework:

- `ui/session/` owns the terminal lifecycle — an RAII `TerminalGuard` for raw
  mode and the alternate screen, released on drop including on panic — plus the
  event loop (50 ms poll, drain-before-redraw, dirty-flag drawing, resize
  re-wrap) and the 80×24 minimum-size notice.
- A surface implements the `Screen` trait (`draw`, `on_key`, `on_mouse`,
  `on_event`, `tick`) and composes `ui/components/`: `TextInput`, `SelectList`,
  `LogView`, the searchable `Picker`, the in-session progress gauge.
- A session with async input (daemon events, search results) is fed through an
  mpsc channel by a driver future the command runs alongside it. Its outcome
  prints plainly to stdout after the terminal is restored, so scrollback keeps a
  record.

A widget — anything that draws a viewport and takes keys — owns the whole
alternate screen for as long as it runs. Line prompts (`ui::prompt`, `input`,
`confirm`) and streams (the `logs -f` follower, the attach after a launch)
do not: they print to the scrollback, so their output stays searchable and
copyable. Progress with **no** interaction is one stderr line rewritten in
place, since flashing the alternate screen for a spinner you cannot act on is
hostile ([0044](../decisions/0044-fullscreen-interaction-inline-progress.md)).

Piped or redirected, every surface degrades to plain text and widgets degrade to
arguments, so output stays scriptable.

This is also the seam for the planned TUI: bare `hestia` prints help today, but
the intended end state is a full-screen TUI over the same `Screen`s and `View`s.

---

# Desktop — `hestia-desktop`

A Tauri v2 shell hosting the React frontend in the root `frontend/`, wired to the
daemon through the same one-way boundary as the CLI.

## The Rust shell is a pipe

The whole wiring is one seam, `src/bridge.rs`:

- a shared `Client` held as Tauri managed state, started at shell launch — so
  opening the app brings its daemon up;
- **one generic** `ipc_call(channel, payload, timeout_ms)` command forwarding
  through the session's `call_raw` (a call that names no timeout gets
  `proto::contract::DEFAULT_TIMEOUT`; the TS layer names the contract's);
- event forwarding: on connect the bridge registers an unscoped
  session listener, subscribes to *every* daemon event, and re-emits each as a
  `hestia:event` webview event.

Mirroring ~120 channels as individual Tauri commands would add a third naming
seam that can drift from both sides while adding no safety — `invoke()` results
are untyped JSON regardless, and the daemon already validates every payload
through the wire contract. So the typed layer is TypeScript, checked against a
channel table generated from the contracts
([0046](../decisions/0046-desktop-bridge-is-one-generic-command.md),
[0084](../decisions/0084-the-ts-channel-table-is-generated-from-the-contracts.md)).

A watcher task notices a lost daemon between calls, emits `hestia:connection`
transitions, and passively reconnects. **Reconnection never spawns** — a daemon
stopped during the session was stopped on purpose. While the connection is down
the bridge answers `connection_lost` from held state rather than attempting the
socket, so a burst of reads costs one socket attempt per watch interval
([0050](../decisions/0050-offline-is-one-state.md)).

Everything else in `commands/` is there because it **cannot** be a daemon call —
each needs something only the shell process has:

| Command | Why it is not generic |
|---|---|
| `account_login_sisu` (`auth.rs`) | Microsoft sign-in opens in a native webview window and completes by reading that window's URL on redirect. A cross-origin webview's location is readable only from Rust ([0048](../decisions/0048-sisu-sign-in-is-a-shell-command.md)) |
| `prefs_list\|set\|remove` (`prefs.rs`) | UI state is the front-end's concern, so it never crosses the socket — written directly to `<data_home>/prefs.json`, resolving the same data home the engine uses ([0049](../decisions/0049-desktop-prefs-live-in-the-data-home.md)) |
| `icons_list`, `icon_set\|remove`, `icon_fetch`, `icon_set_bytes`, `icon_config` (`icons.rs`) | a picked image is copied to `<data_home>/icons/<entry-id>.<ext>` so it survives the original moving. `icon_fetch` downloads a modpack's icon there when an entry is created from the pack. `icon_set_bytes` stores the PNG the webview baked on a 256px canvas from a background and a symbol (the randomized icon a wizard-created entry starts with, or the editor's pick), format-sniffed like a fetch, with the sidecar JSON `icon_config` reads back. The webview loads them over the asset protocol, whose scope is widened to that directory per call |
| `crash_report`, `log_write` (`diagnostics.rs`) | a webview error kills the UI without touching the Rust stack, so the shell records it into the same crash directory the daemon writes to. `log_write` routes console logging into the process `tracing` subscriber under the `ui` target |
| `open_path`, `open_url` (`opener.rs`) | the host's default handler has to start with the host's environment, which inside an AppImage is not the shell's; `tauri-plugin-opener` passes the shell's on ([0080](../decisions/0080-only-the-shell-runs-in-the-appimage-environment.md)) |
| `screenshots_allow` (`screenshots.rs`) | widens the asset protocol's scope to an instance's own screenshots folder, which only the shell can do |
| `pending_archive` (`open.rs`) | hands the webview the `.hestia` archive the app was opened with, once |
| `ready` (`splash.rs`) | the webview reports it has painted, which reveals the window |
| `start_daemon` (`bridge.rs`) | the `Client::start()` spawn path behind the offline overlay's start button |
| `relaunch_daemon` (`bridge.rs`) | the same spawn, after a restart or a data-home move: it waits for the stopping daemon's connection to close first, since the endpoint is shared and a start before then would reach the daemon on its way out |

## The frontend

```mermaid
flowchart TD
    UI["features/ + routes/<br/><i>components render, nothing else</i>"]
    Q["queries/<br/>TanStack Query — one module per domain"]
    API["api/<br/>typed functions over ipc_call"]
    T["api/types/<br/>generated from proto by ts-rs"]
    BR["bridge.rs"]

    UI --> Q --> API --> BR
    T -.->|"scripts/gen-types.sh"| API
    BR -.->|"hestia:event"| BUS["event bus"] --> Q
```

**`api/`** — `core/` holds the `ipc_call` wrapper, the event bus, and a
`runJob` driver mirroring `Session::run_job` (client-generated job id,
subscribe-before-start). `types/` is **generated** from `proto`: ts-rs writes the
payload structs (the feature-gated `#[ts]` derives never enter a production
build) and `scripts/gen-types.sh` writes `channels.ts`, the `Channels` / `Jobs` /
`Topics` tables and each contract's timeout. `call('server.status', …)` takes its
params and returns its result from that table, applies the contract's timeout,
and `runJob('server.create', params, job)` starts the channel and resolves with
its `Done` payload; `onDaemonEvent` payloads are narrowed by topic. Then one
module per domain, mirroring the client facades. The browser mock's handlers
(`src/mock/channels`) and `queries/invalidation.ts` are typed against the same
tables, so a renamed channel or a drifted fixture fails typecheck.

**`queries/`** — one module per domain, for the channels a screen uses. Each
exports **factories** — options objects, not hooks (`serverQueries.detail(id)`,
`serverMutations.start(id)`) — passed to TanStack's own `useQuery`/`useMutation`.
One definition then serves a component, a route loader's `ensureQueryData` and an
imperative fetch alike. A named hook is added only where a factory cannot express
it: a read composing two sources (`useServer`, `useDaemon`) or one accumulating
live events (`useServerLogs`, `useProcessMetrics`).

- Keys come from one hierarchical factory, always keyed by **stable entry ids**,
  never the renameable display name, with an entry's sub-resources nested under
  its `detail(id)` prefix so one sweep refreshes the whole entry.
- Long-running operations are **job mutations** routed through a global job
  store, so an activity surface sees every in-flight job with live progress,
  surviving unmount and navigation. The store alone reports a job's outcome
  ([0089](../decisions/0089-a-job-outcome-is-reported-once.md)).
- Freshness is belt-and-suspenders: mutations invalidate their own key prefixes
  on settle, and `queries/invalidation.ts` maps daemon topics (a finished job by its start
  channel, a `changed` event by its scope) to key prefixes, so changes made by the CLI, the tray or a schedule land without
  polling. A reconnect invalidates everything.
- Streaming is hooks too — `useConnection()`, `useDaemonEvent(topic, handler)`,
  and log hooks that accumulate `process.output` onto the fetched tail. Components
  never touch the event bus.

[hooks.md](../hooks.md) is the usage guide for this layer.

**`features/` and `routes/`** — pages for the library, servers, instances,
content browse, skins and settings, over a shared app shell with
an offline overlay, a first-run sign-in prompt and route guards for the
account-gated instance surface.

Every feature has the same shape, and a directory only exists once it has
something in it:

```
features/<name>/
├── page.tsx          the route body; detail.tsx for a $id route
├── components/       presentational pieces
├── dialogs/          one file per dialog, named for what it does
├── hooks/            use-*.ts — stateful React
├── lib/              pure helpers, no React
├── tabs/             a detail page's tab bodies
└── <flow>/           a wizard: dialog.tsx · steps/ · its own hooks/ + lib/
```

Rules that keep it from drifting:

- **A directory is reached through its `index.ts`.** Every multi-file directory
  has one, and an import from outside names the directory, never a file inside
  it (`@/features/skins/components`). Siblings import each other relatively, so
  nothing imports its own barrel.
- **A dialog is a `Dialog`.** One word for the concept, matching the
  `components/ui/dialog` primitive it is built on.
- **Every dialog is an `AppDialog`** (`components/dialog`). The parent owns the
  chrome — the box, its `size` and `height`, the header, the close button — and
  a dialog supplies only its content: a `DialogBody` (the part that scrolls) and
  a `DialogFooter`. A feature never composes `DialogContent` itself, so a change
  to how dialogs are sized happens in one place. `ConfirmDialog` is the same box
  over the alert primitive.
- **Vocabulary two features share lives in `features/shared/`,** which is a leaf:
  `features/shared/*` may not import `features/<feature>/*`. `shared/entry` is
  what a server and an instance both are (cards, the create wizard, the detail
  sections, the settings tab); `shared/content` is the content vocabulary the
  browse pages and the entry content tabs resolve against. Everything else is
  one-directional — `library → instances`, `instances → content`,
  `servers → content` — so the feature graph is acyclic.
- **A dropped file goes through one pipeline** (`lib/file-drop`), shaped like
  `react-dropzone`: `useDropzone({ accept, onDrop, multiple, disabled, label })`
  returns `getRootProps()` to spread on the element that takes the drop and
  `isDragActive` to highlight it. The drop goes to the innermost dropzone under
  the pointer that accepts the file, so an open dialog shields the page behind
  it with no rule of its own. The library itself does not fit: only the
  webview's window-wide drag-drop event carries a real path, and the daemon is
  handed paths, not bytes — so bootstrap starts that one listener and the
  pipeline hit-tests the pointer by hand. A zone with a `label` gets the shell's
  `FileDropOverlay`.

## Two kinds of offline

The daemon being down and the *internet* being unreachable are separate states
with separate treatments, and conflating them is the mistake the UI is shaped to
avoid.

A lost **daemon** covers the window (`OfflineOverlay`): nothing works without it,
so every page would otherwise render its own empty state
([0050](../decisions/0050-offline-is-one-state.md)).

A lost **connection** must not block anything — instances still launch, servers
still run, installed content is still there. So it gets its own light in the
status bar beside the daemon's, seeded from `net.status` and kept live by the
`net.state` topic (`queries/net.ts`), and each surface that genuinely needs
upstream says so itself:

| Surface | Offline treatment |
|---|---|
| Content browse, project detail, the install picker, the version dialog | `OfflineState` — the whole surface, since there is nothing local to show |
| Skin library | `OfflineState` when nothing is cached; Mojang owns the profile |
| Version pickers in the create wizard | `StaleNotice` — the list answers from the cached catalogue |
| Java runtimes, update check, entry content updates, sign-in | `OfflineNotice` and a disabled action — the content is local, only the download is not |
| `/offline` | the dedicated page: which state, what still works, what does not, and a retry |

The `offline` error code is silenced in the query toast path for the same reason
a lost socket is — the status bar already reports it — but **not** for
mutations: a mutation is something the user asked for, so being offline is an
answer they are owed rather than a silent no-op.

## Browser dev — the fixture daemon

`frontend/src/mock/` fakes `window.__TAURI_INTERNALS__` so the UI runs under a
plain `vite dev` with no shell and no `hestiad` behind it. It is installed from
`main.tsx` only in a dev build, and only when the real shell is absent, so it
never reaches a desktop bundle.

It is laid out like the thing it replaces, one module per domain on each side:

| In the mock | Stands in for |
|---|---|
| `state/` | `crates/engine` — the mutable world (entries, content pools, processes, worlds, settings) |
| `channels/` | `crates/daemon/src/services/` — one registrar per domain |
| `commands/` | `crates/desktop/src/commands/` plus the bundled Tauri plugins |
| `router.ts` | the daemon's router and the shell's `ipc_call` bridge |
| `job.ts`, `bus.ts` | the job managers and the event hub |

It is a **stateful** fake, because the two things a canned response cannot do
are the two that matter in dev: a create must show up in the list it was made
from, and a job must **settle** — `runJob` blocks until a terminal event
arrives, so a fixture that answers the start call and publishes nothing hangs
the caller forever. Jobs here walk their phases on a timer, publish progress,
and settle; a started entry registers a supervised process and broadcasts
`process.started` / `.output` / `.metrics` / `.exit` like the supervisor does.
Fixtures are typed against the generated channel table, params and result both
(a payload reader only takes a key the channel's params have), so a wire change
fails the typecheck here rather than silently serving a stale shape. An unlisted
channel or command fails a test outright; in browser dev it answers with an
empty-proxy value that degrades instead of crashing the page, and warns.

## Messages

The catalogue (paraglide/inlang, `frontend/messages/`) is organised on **one
axis — where the string is rendered** — and split one file per root:

| Root | What goes in it |
|---|---|
| `app.*` | shell chrome and shared vocabulary — nav, window, action, label, status, toast, search, time, jobs, validation, daemon |
| `<feature>.*` | one root per `frontend/src/features/` directory |
| `domain.*` | vocabulary mirroring a `proto` enum — content kinds, flavors, gamemodes, difficulties, provision phases, entry types |
| `error.*` / `warning.*` | the daemon's own vocabulary, keyed by variant, looked up dynamically |

Nest with objects rather than underscores — paraglide flattens a dotted key to
underscores, so `entry_settings.x` and `entry.settings_x` would compile to one
identifier.

The guard is a test, not a convention (`frontend/tests/unit/messages.test.ts`): every
locale must cover the base locale exactly and interpolate the same
`{placeholders}`, every referenced key must exist, and every defined key must
have a call site. Dynamically reached tables must be declared in
`DYNAMIC_PREFIXES` — an undeclared table is exactly what makes dead keys
unprovable ([0047](../decisions/0047-messages-organised-by-render-surface.md)).

---

# Tray

A resident helper wearing the desktop app's icon, so both front-ends share one
face. It publishes a `org.kde.StatusNotifierItem` over D-Bus on Linux and uses
Tauri's native tray crates on Windows.

The menu is: **Open Hestia**, a status header (version + running/stopped), a
start/restart action, a start-at-login toggle bound to the reserved `autostart`
config key, and a quit that stops the daemon while leaving workloads running.

A worker thread polls the daemon every two seconds over the client SDK and
reports state changes through a `Sink` — the event loop on Windows, the tray
handle on Linux — so the polling loop itself is platform-agnostic. Menu actions
travel back over an mpsc channel, so nothing rendering the icon blocks on the
socket. Left-click launches the desktop shell.

**The daemon spawns the tray; the tray outlives the daemon.** A stopped daemon is
exactly when the tray is most useful — the greyed status plus a start action — so
only its own Quit removes it. A duplicate spawn after a daemon restart is
absorbed by an exclusive lock keyed by endpoint, so a dev daemon's tray and the
session's tray coexist ([0051](../decisions/0051-the-daemon-spawns-the-tray.md)).

**The tray speaks the tray protocol, and asks nothing of the machine.** On Linux
the status area is a D-Bus protocol, `org.kde.StatusNotifierItem`, and the tray
implements it directly (`ksni`) rather than through libappindicator — a library
most desktops do not ship, whose absence used to mean no icon and a crash report
per daemon start. The requirement is now a session bus and a desktop that hosts
a tray, never a package on the user's machine, which is why the icon appears
wherever an Electron app's does — Plasma and SteamOS, XFCE, Cinnamon, LXQt, a
Wayland bar, GNOME with the AppIndicator extension
([0068](../decisions/0068-the-tray-speaks-the-tray-protocol.md)).

**A session with no host yet is not a failure.** The item stays published and
registers the moment a watcher appears, so a bar that restarts or an extension
enabled mid-session finds the tray waiting, with nothing to restart.

Single-instance is enforced deliberately in each front-end: the tray by that
runtime lock, the desktop by `tauri-plugin-single-instance`, which focuses the
existing window rather than opening another. They must not share a GApplication
id, or on Linux each would block the other
([0052](../decisions/0052-tray-and-desktop-app-ids.md)).

## Decisions

- [0040 — Entry-first, with verb-first shortcuts for the hot path](../decisions/0040-entry-first-cli-grammar.md)
- [0041 — Every daemon capability gets a scriptable verb, or a written reason](../decisions/0041-every-capability-gets-a-verb.md)
- [0042 — `-vv` buys wire visibility, not more volume](../decisions/0042-vv-buys-wire-visibility.md)
- [0043 — A state query answers through its exit code](../decisions/0043-state-queries-answer-through-exit-codes.md)
- [0044 — Interaction is fullscreen; bare progress is one line](../decisions/0044-fullscreen-interaction-inline-progress.md)
- [0046 — The desktop bridge is one generic command](../decisions/0046-desktop-bridge-is-one-generic-command.md)
- [0047 — Messages are organised by render surface](../decisions/0047-messages-organised-by-render-surface.md)
- [0048 — Sign-in is the one bespoke shell command](../decisions/0048-sisu-sign-in-is-a-shell-command.md)
- [0049 — Front-end preferences are desktop-local](../decisions/0049-desktop-prefs-live-in-the-data-home.md)
- [0050 — Offline is one state, not a failure per read](../decisions/0050-offline-is-one-state.md)
- [0067 — Reachability is observed from real traffic, and offline is a state the whole system reads](../decisions/0067-reachability-is-observed-not-asked.md)
- [0051 — The daemon spawns the tray; the tray outlives the daemon](../decisions/0051-the-daemon-spawns-the-tray.md)
- [0052 — The tray and desktop must not share a GApplication id](../decisions/0052-tray-and-desktop-app-ids.md)
- [0068 — The tray speaks the tray protocol, not a library that might be installed](../decisions/0068-the-tray-speaks-the-tray-protocol.md)
- [0080 — Only the shell runs in the AppImage's environment](../decisions/0080-only-the-shell-runs-in-the-appimage-environment.md)
