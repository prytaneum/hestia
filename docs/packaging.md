# Packaging & release

How Hestia turns into installable artifacts. Packaging is driven by the **Tauri v2 bundler** (`cargo tauri build`); the
desktop app and its sidecar binaries are bundled together, and CI ([
`.github/workflows/release.yml`](../.github/workflows/release.yml))
builds and publishes the artifacts on version tags.

## Artifacts

| Platform | Formats                                      |
|----------|----------------------------------------------|
| Linux    | `.deb`, `.rpm`, AppImage, portable `.tar.gz` |
| Windows  | NSIS `.exe`, portable `.zip`                 |

x86_64 only for now. Builds run on Linux and Windows runners.

## One installer, everything bundled

The desktop app is the product; the daemon, tray, and CLI ride along as Tauri **sidecars** (`bundle.externalBin` in
[`crates/desktop/tauri.conf.json`](../crates/desktop/tauri.conf.json)):

- `hestiad` — the resident daemon the desktop app drives over the socket.
- `hestia-tray` — the system-tray helper.
- `hestia` — the CLI/TUI.

Each is built with the target-triple suffix Tauri requires (`hestiad-x86_64-unknown-linux-gnu`, …) and staged into
`crates/desktop/binaries/`
by [`scripts/sidecars.sh`](../scripts/sidecars.sh); the bundler strips the suffix on install. `deb`/`rpm`/AppImage
install the **full set** with no component picker; the NSIS installer is customized — see below.

## What the binaries are called

Two names per binary, and which one ships depends on the platform — the source of truth is `common::app`
(`DESKTOP_BIN`, `TRAY_BIN`, `DAEMON_BIN`), which lists them most-preferred first so `common::paths::sibling_binary`
resolves a sibling in any layout, including a dev build under cargo's names.

|               | Windows           | Linux            |
|---------------|-------------------|------------------|
| desktop shell | `Hestia.exe`      | `hestia-desktop` |
| tray          | `Hestia Tray.exe` | `hestia-tray`    |
| CLI           | `hestia.exe`      | `hestia`         |
| daemon        | `hestiad.exe`     | `hestiad`        |

Windows names the two a user meets — in Program Files, in Task Manager — after the product, via `mainBinaryName` in
[`tauri.windows.conf.json`](../crates/desktop/tauri.windows.conf.json) for the shell and an `/oname=` rename in the NSIS
template for the tray. Linux keeps cargo's names, because `deb`/`rpm` install into `/usr/bin`, where a capital or a
space would be hostile to the shell that has to type them; nothing user-visible there comes from the binary name anyway
(the launcher reads `Name=` from the `.desktop` entry, and the AppImage is named from `productName`).

**This is why the `bin/` split exists.** `Hestia.exe` and `hestia.exe` differ only by case, and Windows paths are
case-insensitive — they cannot share a directory.

## The NSIS installer

NSIS is the **only** Windows installer. The WiX `.msi` was dropped: it installs every binary into one flat directory,
which the names above cannot survive — `Hestia.exe` and `hestia.exe` would collide — and fixing it meant a second forked
bundler template to re-diff on every tauri-cli bump. It bought nothing the NSIS installer does not already do better;
`installMode: both` covers per-user and per-machine, which was the MSI's only real advantage.

Windows uses a **custom NSIS template**
([`crates/desktop/windows/installer.nsi`](../crates/desktop/windows/installer.nsi)), a fork of tauri-bundler's stock
template rendered with the same handlebars context. It must track the pinned tauri-cli version (`2.11.4`, both locally
and in [`release.yml`](../.github/workflows/release.yml)) — re-diff the fork against upstream's
`crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi` when bumping.

What it adds over stock:

- **Components page** — *Hestia core* (`hestiad` + the tray, required), *Desktop app*, and *CLI* (both checked by
  default, deselectable). Choices are persisted in the uninstall registry key and become the defaults for the next run —
  and the effective selection for silent/passive updates. Deselecting a previously installed component on an update
  removes it.
- **Install mode `both`** — per-user or all-users, chosen at install time (`bundle.windows.nsis.installMode` in
  `tauri.conf.json`), remembered for updates and uninstall.
- **A split layout** — `Hestia.exe` and `Hestia Tray.exe` install to `$INSTDIR`, because a user launches them;
  `hestia.exe` and `hestiad.exe` to `$INSTDIR\bin`, because a shell or another binary reaches them. The portable archive
  lays down the same split.
- **CLI on PATH** — the CLI component appends `$INSTDIR\bin` to the user or machine `PATH` (matching the install mode)
  and removes it on uninstall or deselection, so `hestia` is reachable without also exposing the daemon, the tray and
  the shell.
- **Graceful daemon handling** — before files are swapped, a running `hestiad`
  is asked to stop via `hestiad stop` (supervised game servers keep running — the daemon re-adopts them on its next
  start) and only killed if it lingers; the tray is stopped too. If the daemon was running, the installer restarts it
  afterwards (unelevated, hidden window).
- **Data survives uninstall** — `%APPDATA%\Hestia` (instances, servers, worlds, accounts) is only removed when the
  uninstaller's *delete app data*
  box is explicitly ticked. Uninstall also drops the `Hestia Daemon`
  autostart scheduled task.
- **Update-friendly** — running a newer setup over an existing install upgrades in place (no uninstall detour), reusing
  the recorded install directory, install mode, and components; downgrades take the stock uninstall-first path.

## The portable archives

Tauri has no portable target, so [`scripts/package.sh`](../scripts/package.sh) builds them itself — and they are **not**
a copy of the installers' binaries. They are a separate `cargo build` under the
`portable` feature into `target/portable/`, because that feature changes where the binaries resolve their data home:

```
hestia-<version>-<triple>/
├── Hestia.exe            what a user launches  (hestia-desktop on Linux)
├── Hestia Tray.exe                              (hestia-tray on Linux)
├── bin/
│   ├── hestia            what a shell or another binary reaches
│   └── hestiad
├── data/                 the data home — nothing is written outside the archive
├── LICENSE
└── README.md
```

`common::paths` anchors the data home on the layout *root*, stepping out of
`bin/`, so all four binaries agree on one `data/` regardless of which directory they sit in — and each finds the others
through `common::paths::sibling_binary`. Autostart is compiled out under the feature: registering it would write an
absolute path into the login session pointing at a directory the user can move or unplug.

Linux packages keep the platform convention instead — `deb`/`rpm`/AppImage install every binary flat into `/usr/bin`,
which the same lookup handles.

## Signing

Hestia has no self-update, so no release artifact carries a signature of its own. Windows installers currently ship
**unsigned**, which costs only warnings:

| Install mode | First install                             |
|--------------|-------------------------------------------|
| per-user     | SmartScreen once                          |
| per-machine  | SmartScreen once, UAC "Unknown publisher" |

`nsis.installMode` is `both`, so the user chooses. Signing does not remove the per-machine UAC prompt; it replaces
"Unknown publisher" with the publisher name and removes the SmartScreen block.

The intended route is [SignPath Foundation](https://signpath.org/terms.html), which signs open source projects in its own
pipeline (the key is never held here). Two things are outstanding: a release must exist first (the Foundation signs
already-released software), and its role-separation requirement (distinct Authors, Reviewers and Approvers with MFA, plus
a published signing policy). Expect OV, not EV: SmartScreen reputation accrues over downloads and time. Always timestamp
Authenticode signatures once a certificate is in use, or every shipped binary becomes untrusted when it expires.

## Runtime dependency: the system WebView

The desktop binary needs a system WebView — **WebView2** on Windows (present by default on Windows 10/11) and
**WebKitGTK** on Linux. The `.deb` declares the WebKitGTK/GTK packages under `bundle.linux.deb.depends`;
the AppImage carries what it can. The tray adds nothing to that list — it reaches the status area over D-Bus. The portable archives assume the WebView is already present.

## Building locally

```bash
# Sidecars + platform installers + portable archive:
scripts/package.sh

# Just the installers, or just the portable archive:
scripts/package.sh bundle
scripts/package.sh portable
```

`cargo tauri build` runs the frontend build itself (via `beforeBuildCommand`), so only `bun install` (in `frontend/`)
and a staged sidecar set are prerequisites — for the installers only; the portable build stages nothing. Bundles land in `target/release/bundle/{deb,rpm,appimage,nsis}/`; portable
archives in `target/package/`. On Windows use `scripts\win.ps1 package`.

On Linux the AppImage is rebuilt after the bundler runs, so **`squashfs-tools`** is a prerequisite there. The
linuxdeploy that Tauri pins predates `libwayland-client.so.0` joining the AppImage excludelist and bundles the library,
which breaks EGL init against a newer host Mesa — a blank window. Tauri exposes no way to exclude a library
([tauri-apps/tauri#15665](https://github.com/tauri-apps/tauri/issues/15665)), so
[`scripts/package.sh`](../scripts/package.sh) drops it from the `.AppDir` the bundler leaves behind and repacks the
image around the original's runtime.

## CI

- [`ci.yml`](../.github/workflows/ci.yml) — four jobs. `check` runs `fmt` + `clippy` + `test` on Linux and Windows, then `cargo check` of the production feature set (the all-targets builds enable the kernel's `probe` and `loopback`) and of the kernel with no features, with `sandbox` and with `probe`,
  excluding the `desktop` crate so no webview is needed, and needing no system libraries on either platform. `frontend` runs the Bun chain — `generate:messages` (`src/paraglide/` is generated and
  untracked, so it precedes anything resolving those imports), then `check` (biome), `typecheck` (tsc), `test` (vitest)
  and `build`. `desktop` covers what `check` excludes: it installs the WebKitGTK stack, builds the frontend
  (`generate_context!` embeds `frontend/dist`), then runs clippy and the crate's tests. tauri-build requires the
  `externalBin` files and `resources` to exist, but only the bundler reads them, so the job empties both through `TAURI_CONFIG`
  instead of compiling sidecars it never ships. `deny` runs `cargo-deny`. The workflow also declares
  `workflow_call`, so the release can reuse it verbatim.
- [`release.yml`](../.github/workflows/release.yml) — on a `v*` tag: `preflight`, then `gate` and `package` side by
  side, then `publish`.
  `preflight` refuses the release before anything is compiled if the tag, `Cargo.toml` and `tauri.conf.json` disagree on
  the version, or if `CHANGELOG.md` has no matching section. `gate` calls
  `ci.yml`, since that workflow's own triggers cover branches but not tags. `package` verifies `Cargo.lock` is
  current (`cargo fetch --locked`) and runs four jobs at once — `scripts/package.sh bundle` and
  `scripts/package.sh portable`, each on a Linux and a Windows runner, because the two are separate compiles of the
  whole tree — uploading the results as workflow artifacts. It does not wait for `gate`, and it publishes nothing:
  `publish` needs both, and is the one job that attaches artifacts to the GitHub Release, so a tag publishes a whole
  release or none of it. A manual `workflow_dispatch` is a dry run: the same gates run, but it uploads workflow
  artifacts without touching a Release. A tag can only restore a build cache saved on the default branch, so a dry
  run dispatched there is what makes the tagged build a warm one.
