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

## Auto-updates

The **daemon** owns self-update — `engine/src/update/`, reached over `update.check`,
`update.download` and `update.apply`. Every front-end is a caller; none reads the manifest, holds a key, or runs an
installer ([decision 0062](decisions/0062-the-daemon-owns-self-update.md)).

It polls the feed API, one manifest per release channel — `…/updates/stable` or `…/updates/beta`, chosen by the
`update.channel` setting ([decision 0066](decisions/0066-a-channel-picks-the-feed-not-the-entry.md)). On a `v*` tag the
release workflow's `manifest` job composes `latest.json`, attaches it to the Release as provenance, and **publishes** it
to the channel the tag names — a prerelease suffix (`v1.3.0-beta.1`) is beta, a plain `v1.3.0` is stable. Nothing reads
the repository's releases to decide what is current. The full HTTP contract, both directions, is in
[update-feed.md](update-feed.md).

Publishing needs the `UPDATE_FEED_URL` variable and the `UPDATE_FEED_TOKEN` secret — an API token minted for an admin
account on the feed API, sent as `x-api-key`. The `preflight` job refuses a **stable** release without them; a beta is
warned and ships as a download only, so a prerelease can go out before the feed it will be served from exists. Its
`latest.json` is still attached to the Release, and publishing it later is that same PUT by hand. The manifest itself,
which the API answers wrapped in its `{success, message, data}` envelope:

```json
{
  "version": "0.1.0",
  "notes": "…",
  "platforms": {
    "windows-x86_64": {
      "url": "…-setup.exe",
      "signature": "…"
    },
    "linux-x86_64": {
      "url": "….AppImage",
      "signature": "…",
      "formats": {
        "deb": {
          "url": "….deb",
          "signature": "…"
        },
        "rpm": {
          "url": "….rpm",
          "signature": "…"
        }
      }
    }
  }
}
```

A platform's top-level entry is its default artifact — the NSIS setup, the AppImage. `formats` carries the rest, and is
additive: a build that predates a format never asks for it and reads the same manifest a newer one does.

How a copy was installed is **detected, not recorded** (`update/install.rs`), and decides both which artifact is asked
for and how it is applied:

| Install             | Detected by                                                                 | Applied with                                                   |
|---------------------|-----------------------------------------------------------------------------|----------------------------------------------------------------|
| NSIS                | `uninstall.exe` at the layout root                                          | the setup, `/P /UPDATE`, via `ShellExecuteW` so UAC can prompt |
| AppImage            | `$APPIMAGE`                                                                 | staged beside the image, renamed over it                       |
| deb / rpm           | the binary is under a system prefix and `dpkg-query -S` / `rpm -qf` owns it | `dpkg -i` / `rpm -U`, escalated                                |
| portable, `target/` | anything else, and always under the `portable` feature                      | nothing — the front-end offers the download URL                |

Escalation on Linux tries `pkexec` first (the one step that can *prompt*), then passwordless `sudo`. When neither can
even ask, the daemon returns `ElevationRequired` carrying the exact command — `hestia update` then offers to run it,
because the CLI has a terminal the daemon does not.

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

Two independent signatures, often confused. Only one of them is mandatory.

|                                         | What it proves                   | Who checks it             | Required?                                    |
|-----------------------------------------|----------------------------------|---------------------------|----------------------------------------------|
| **Update signature** (minisign/Ed25519) | this download came from us       | `engine/src/signature.rs` | **yes** — the updater cannot work without it |
| **Authenticode** (code signing cert)    | Windows knows who published this | Windows SmartScreen + UAC | no — costs a warning, not a failure          |

### The update key

One minisign keypair. The **public** half lives in exactly one place —
`common::app::UPDATE_PUBKEY` — because only the daemon verifies anything;
`crates/common/tests/updater.rs` fails the build if a second copy reappears in `tauri.conf.json`.

The **private** half signs releases through [`scripts/sign.sh`](../scripts/sign.sh), either as the
`RELEASE_SIGNING_KEY` repository secret (with `RELEASE_SIGNING_KEY_PASSWORD`) or locally by whoever cuts the release. CI
signs the finished artifacts itself rather than letting the bundler do it: the bundler never signed `.deb` or `.rpm`
at all, and those are exactly what the `formats` map needs. Offline is the safer default, because:

> **A build trusts only the keys compiled into it.** Nothing sent later changes
> that, so a successor key has to ship *before* it is needed — which is what
> `UPDATE_PUBKEY_NEXT` is for. Lose both halves of both keys and the installs
> in the field can never accept another update.

### Rotating the signing key

Two keys ship in every build. `UPDATE_PUBKEY` signs releases;
`UPDATE_PUBKEY_NEXT` is the successor, whose private half stays offline and out of CI until the day it is needed. To
rotate:

1. Swap the values — the spare becomes `UPDATE_PUBKEY`, in `common::app`. That is the only file to touch.
2. Generate a fresh spare into `UPDATE_PUBKEY_NEXT`, private half offline.
3. Replace `RELEASE_SIGNING_KEY` / `..._PASSWORD` with the new signing key's, and cut a release.

Builds already in the field verify against the successor they were shipped with, so they accept the new release without
an intermediate version.

**Tauri's own procedure is different and weaker.** Upstream
([tauri#7585](https://github.com/tauri-apps/tauri/issues/7585), open) documents a staggered rotation: sign v2 with the
*old* key while shipping the *new*
pubkey, then sign v3 with the new key. It needs no extra code, but it requires still holding the old key — useless if
the key was lost or leaked — and it strands anyone who skips the intermediate version, which for a launcher is routine.
Carrying the successor instead is what OpenBSD's signify does, what
[sparkle#1501](https://github.com/sparkle-project/Sparkle/issues/1501) proposes for Sparkle, and what tauri#7585 asks
for (`pubkey: Option<Vec<String>>`, "try the first, and if it fails try the second").

`engine/src/signature.rs` accepts any key in the set, so a rotation costs nothing at the verification site — this was
awkward only while the desktop verified through `tauri-plugin-updater`, whose config holds exactly one key.

Testing the update path locally never means editing these constants:
[`scripts/update.sh --env`](../scripts/update.sh) hands the endpoint and the key to trust to a **debug** build through
`HESTIA_UPDATE_ENDPOINT` and `HESTIA_UPDATE_PUBKEY`, and the second is honoured only alongside the first — so there is
nothing to paste in and nothing to put back. A release build reads neither.

`scripts/package.sh` builds unsigned, since signing is a separate step. CI refuses a **tagged** release with no key
configured — the guard is in the `preflight` job, which runs before anything is built, so a release that could never
publish a `latest.json` fails in seconds rather than after every platform has compiled.

### Authenticode — the path, not the requirement

Windows installers currently ship **unsigned**. This is deliberate and costs only warnings:

| Install mode | First install                             | Auto-update      |
|--------------|-------------------------------------------|------------------|
| per-user     | SmartScreen once                          | silent           |
| per-machine  | SmartScreen once, UAC "Unknown publisher" | UAC every update |

`nsis.installMode` is `both`, so the user chooses; the template's
`MULTIUSER_EXECUTIONLEVEL Highest` means an admin account elevates at launch. Signing does **not** remove the UAC
prompt — a per-machine install requires elevation regardless — it changes the dialog from "Unknown publisher" to the
publisher name, and it removes the SmartScreen block. The recurring per-machine update prompt is the real argument for a
certificate; the install-day scare is the lesser one.

The intended route is [SignPath Foundation](https://signpath.org/terms.html), which issues a free OV certificate to
qualifying open source projects and signs in its own pipeline (the key is never held here). Hestia meets the licence
(GPL-3.0-only, no dual-licensing), no-proprietary-components, maintained and documented conditions. Two things are
outstanding, and their order is forced:

1. **A release must exist first** — the Foundation signs already-released software, so the first tags necessarily ship
   unsigned.
2. **Role separation** — Authors, Reviewers and Approvers as distinct people with MFA, plus a published code signing
   policy page. This is the condition a single-maintainer project has to resolve with them.

Expect OV, not EV: SmartScreen reputation accrues per certificate over downloads and time, so warnings fade rather than
vanish on approval. Switching or renewing a certificate later is cheap — reputation resets, nothing breaks — which is
the opposite of the updater key above. Always timestamp Authenticode signatures once a certificate is in use, or every
shipped binary becomes untrusted the day it expires.

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

**`HESTIA_CURSEFORGE_API_KEY`** is read at compile time by the `engine` crate: a distributor that has registered for a
[CurseForge key](https://console.curseforge.com/) sets it in the build environment and the CurseForge content source
works out of the box. Built without it, the source stays hidden until a user sets `content.curseforge-key` themselves,
which overrides the baked-in key either way. It is baked into the binary, so it is extractable — do not use a key you
are not willing to distribute.

Releases take it from the `CURSEFORGE_API_KEY` repository secret, which
[`release.yml`](../.github/workflows/release.yml) passes into the build as that variable. Only `hestiad` links `engine`,
so the key rides in the daemon sidecar and nowhere else. `engine/build.rs` declares
`rerun-if-env-changed`, so a warm CI cache still recompiles against the current value rather than shipping a stale one.
A tagged release without the secret warns rather than fails.

## CI

- [`ci.yml`](../.github/workflows/ci.yml) — four jobs. `check` runs `fmt` + `clippy` + `test` on Linux and Windows,
  excluding the `desktop` crate so no webview is needed, and needing no system libraries on either platform. `frontend` runs the Bun chain — `generate:messages` (`src/paraglide/` is generated and
  untracked, so it precedes anything resolving those imports), then `check` (biome), `typecheck` (tsc), `test` (vitest)
  and `build`. `desktop` covers what `check` excludes: it installs the WebKitGTK stack, builds the frontend
  (`generate_context!` embeds `frontend/dist`), then runs clippy and the crate's tests. tauri-build requires the
  `externalBin` files to exist, but only the bundler reads them, so the job empties that list through `TAURI_CONFIG`
  instead of compiling sidecars it never ships. `deny` runs `cargo-deny`. The workflow also declares
  `workflow_call`, so the release can reuse it verbatim.
- [`release.yml`](../.github/workflows/release.yml) — on a `v*` tag: `preflight`, then `gate` and `package` side by
  side, then `manifest`.
  `preflight` refuses the release before anything is compiled if the tag, `Cargo.toml` and `tauri.conf.json` disagree on
  the version, if `CHANGELOG.md` has no matching section, if the updater signing key is missing, or if a stable release
  has no feed credentials to publish itself with. `gate` calls
  `ci.yml`, since that workflow's own triggers cover branches but not tags. `package` verifies `Cargo.lock` is
  current (`cargo fetch --locked`) and runs four jobs at once — `scripts/package.sh bundle` and
  `scripts/package.sh portable`, each on a Linux and a Windows runner, because the two are separate compiles of the
  whole tree — uploading the results as workflow artifacts. It does not wait for `gate`, and it publishes nothing:
  `manifest` needs both, and is the one job that attaches artifacts to the GitHub Release, so a tag publishes a whole
  release or none of it. A manual `workflow_dispatch` is a dry run: the same gates run, but it uploads workflow
  artifacts without touching a Release. A tag can only restore a build cache saved on the default branch, so a dry
  run dispatched there is what makes the tagged build a warm one.
