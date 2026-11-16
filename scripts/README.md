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
| `run.sh`        | run a development build: a subshell, one `hestia` command, or the desktop (`--desktop`); all forms share one dev daemon |
| `clean.sh`      | `cargo clean` plus the frontend `dist`/`node_modules` and Tauri `gen` |
| `sidecars.sh`   | build + stage `hestia`/`hestiad`/`tray` as Tauri sidecars for bundling |
| `package.sh`    | release artifacts: Tauri installers + portable archive (`all`/`bundle`/`portable`) |
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
scripts/run.sh --help

scripts/build.sh cli --release
scripts/package.sh                # sidecars + Tauri bundles + portable archive
scripts/package.sh portable       # portable .tar.gz only
```
