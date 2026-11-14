# scripts

Thin wrappers around `cargo` and `cargo tauri` for local development and
packaging.

```
scripts/
├── <verb>.sh        one file per verb — what you run
├── win.ps1          the Windows entry point
└── lib/             what a verb uses; never run directly
```

**A file at the top level is a verb.** `win.ps1` discovers `scripts/*.sh` and
forwards to whichever one matches, so a new script is reachable from PowerShell
the moment it exists — there is no list to update. Anything a verb *sources* or
*drives* is not a verb and lives in `lib/`, which the discovery glob does not
reach: that directory is the whole distinction, and it is why the split is worth
having.

| Verb            | What it does                                                        |
|-----------------|---------------------------------------------------------------------|
| `build.sh`      | `cargo build` a target (`cli`, `daemon`, `desktop`, or `all`)        |
| `run.sh`        | run a development build: a subshell, one `hestia` command, or the desktop (`--desktop`); `--plugins` installs the official plugins; all forms share one dev daemon |
| `clean.sh`      | `cargo clean` plus the frontend `dist`/`node_modules` and Tauri `gen` |
| `sidecars.sh`   | build + stage `hestia`/`hestiad`/`tray` as Tauri sidecars for bundling |
| `package.sh`    | release artifacts: Tauri installers + portable archive (`all`/`bundle`/`portable`) |
| `update.sh`     | self-update: serve a signed fake `latest.json` locally, and print the env to reach it |
| `sign.sh`       | minisign release artifacts (`<file>.sig`), or verify them (CI)      |
| `gen-types.sh`  | regenerate the TypeScript bindings for the `proto` wire types (ts-rs) |
| `gen-icons.sh`  | regenerate every shipped icon from `assets/icons/ember.svg`           |

| Helper             | Used by                                                          |
|--------------------|------------------------------------------------------------------|
| `lib/common.sh`    | sourced first by every verb: strict mode, the repo root as cwd, `log`/`die` |
| `lib/gen-barrels.py` | `gen-types.sh` — the per-module barrels over the generated types |

A verb starts with one line, and everything after it can assume the repo root:

```bash
#!/usr/bin/env bash
. "$(dirname "$0")/lib/common.sh"
```

Examples:

```bash
scripts/run.sh                    # subshell: hestia + hestiad on PATH, dev daemon running
scripts/run.sh java list          # one hestia command (builds first)
scripts/run.sh --desktop          # desktop app with frontend hot reload
scripts/run.sh --plugins=../hestia-plugins --desktop   # with the plugins of that checkout, for this run
scripts/run.sh --help

scripts/build.sh cli --release
scripts/package.sh                # sidecars + Tauri bundles + portable archive
scripts/package.sh portable       # portable .tar.gz only

eval "$(scripts/update.sh --env)"  # point this shell at the local feed
scripts/update.sh --serve         # fake release feed on 127.0.0.1:8788
```

**Testing self-update locally.** Opt-in, because a dev run should not offer a
fake update every time. Nothing here edits the source:
`--env` prints the endpoint *and* the key to trust, and a debug build honours
`HESTIA_UPDATE_PUBKEY` only alongside `HESTIA_UPDATE_ENDPOINT` — so there is no
constant to paste in and nothing to remember to put back. A release build reads
neither.

```bash
scripts/update.sh --serve &
eval "$(scripts/update.sh --env)"
scripts/run.sh
hestia update --yes
```

The served artifact is a throwaway file but it is **signed**, and the signature
is still checked — that is the part of the path most worth exercising. Drop
`HESTIA_UPDATE_PUBKEY` and the same run fails with *no trusted key verifies this
artifact*, which is the compiled-in key set doing its job.

A `target/` build is *unmanaged*, so that run stops at "download it at …" — the
check and the version comparison, and no more. To reach the download, the
verification and the apply, tell the daemon it is an AppImage, the one install
shape that needs no root:

```bash
printf 'old\n' > /tmp/Hestia.AppImage && chmod +x /tmp/Hestia.AppImage
APPIMAGE=/tmp/Hestia.AppImage scripts/run.sh
hestia update --yes
cat /tmp/Hestia.AppImage      # replaced, and still 0755
```

`deb`/`rpm` cannot be faked this way — detection asks `dpkg-query -S` / `rpm -qf`
who owns the binary, so it needs a real package install. Windows needs a real
NSIS install for the same reason: the uninstaller at the layout root is what
`update/install.rs` looks for.
