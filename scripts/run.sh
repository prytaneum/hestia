#!/usr/bin/env bash
#
# run.sh — the one way to run a development build.
#
# Every form builds first and talks to the same dev daemon, so a shell in one
# terminal and the desktop in another are two views of one state. A daemon that
# is already running is restarted onto the new build. Debug builds keep their
# data in target/debug/data and never touch ~/.hestia.
. "$(dirname "$0")/lib/common.sh"

usage() {
  cat << 'EOF'
usage: scripts/run.sh [flags] [hestia args...]

  scripts/run.sh --shell          a subshell with hestia + hestiad on PATH (the default)
  scripts/run.sh --desktop        the desktop app, with frontend hot reload
  scripts/run.sh --daemon         hestiad in the foreground, logging to this terminal
  scripts/run.sh <hestia args>    run `hestia <args>` once, e.g. `run.sh server list`

what to run (one of):
  --shell           a subshell; leaving it stops the daemon it started
  --desktop         the desktop app
  --daemon          the daemon in the foreground; Ctrl-C stops it

options:
  --plugins[=DIR]   run with the official plugins: every plugin of a checkout of
                    the plugins repository (default: $HESTIA_PLUGINS_REPO, else
                    ../hestia-plugins) is installed for this run and removed
                    when it ends. Without it a run has no plugins
  --release         run the optimised build, as a release would be: `--release
                    --desktop` is the app without hot reload. Its data goes to
                    target/release/data, never ~/.hestia
  --home=DIR        use this data directory instead (`--home="$HOME/.hestia"` runs
                    the build against your real data)
  -h, --help        show this

Options combine with what to run: `scripts/run.sh --release --plugins --desktop`.
A flag of hestia's own goes after `--`: `scripts/run.sh -- --quiet daemon status`.
EOF
}

# An installed hestia must not answer for the dev build: drop PATH entries
# that carry one.
strip_installed_hestia() {
  local kept="" dir
  local IFS=':'
  for dir in $PATH; do
    if [ -e "$dir/hestia" ] || [ -e "$dir/hestia.exe" ] ||
      [ -e "$dir/hestiad" ] || [ -e "$dir/hestiad.exe" ]; then
      log "ignoring installed hestia in $dir" >&2
      continue
    fi
    kept="${kept:+$kept:}$dir"
  done
  printf '%s' "$kept"
}
PATH="$(strip_installed_hestia)"

mode=debug
flags=()
what=""
plugins=""
home=""

pick() {
  [ -z "$what" ] || die "--$what and --$1 are two things to run; pick one"
  what="$1"
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --release) mode=release; flags=(--release); shift ;;
    --shell) pick shell; shift ;;
    --desktop) pick desktop; shift ;;
    --daemon) pick daemon; shift ;;
    --home=*) home="${1#--home=}"; shift ;;
    --plugins) plugins="${HESTIA_PLUGINS_REPO:-../hestia-plugins}"; shift ;;
    --plugins=*) plugins="${1#--plugins=}"; shift ;;
    -h | --help) usage; exit 0 ;;
    --) shift; break ;;
    --*) usage >&2; die "unknown flag $1 (hestia's own flags go after \`--\`)" ;;
    *) break ;;
  esac
done

# A debug build scopes its endpoint to its own data directory, so every debug
# binary agrees on one daemon with nothing set. A release build is not
# contained: without its own endpoint and data directory it would reach the
# installed daemon and the real ~/.hestia. On Windows the endpoint is a named
# pipe, supplied by win.ps1.
if [ "$mode" = release ]; then
  export HESTIA_SOCK="${HESTIA_SOCK:-${XDG_RUNTIME_DIR:-/tmp}/hestiad-dev-$(id -u).sock}"
  [ -n "$home" ] || home="$PWD/target/release/data"
fi
if [ -n "$home" ]; then
  mkdir -p "$home"
  HESTIA_HOME="$(cd "$home" && pwd)"
  export HESTIA_HOME
  log "data directory: $HESTIA_HOME"
fi

[ "$#" -eq 0 ] || [ -z "$what" ] || die "--$what takes no hestia arguments (got: $*)"

bindir="$PWD/target/$mode"
hestia="$bindir/hestia"
before="$(mktemp)"
log "building ($mode)"
cargo build "${flags[@]}"
export PATH="$bindir:$PATH" HESTIA_DEV=1

daemon_running() { "$hestia" --quiet daemon status > /dev/null 2>&1; }

# A running daemon is the old build in memory: restart it, but only when this
# build actually produced a new one.
started_here=0
if [ "$bindir/hestiad" -nt "$before" ] && daemon_running; then
  log "restarting the dev daemon onto this build"
  "$hestia" --quiet daemon restart
fi
rm -f "$before"

ensure_daemon() {
  daemon_running && return 0
  log "starting the dev daemon"
  "$hestia" --quiet daemon start
  started_here=1
}

# Nothing below `exec`s: bash does not run an EXIT trap across exec. The signals
# matter as much as EXIT: `run.sh … | head` dies of SIGPIPE, and an untrapped
# one skips EXIT entirely. A run leaves things as it found them: the plugins it
# installed are removed, and only a daemon it started is stopped, so leaving a
# shell never takes the daemon away from a desktop that is using it.
installed=()
developer_was=""
cleanup() {
  local id
  for id in ${installed[@]+"${installed[@]}"}; do
    "$hestia" --quiet plugin remove "$id" --yes > /dev/null 2>&1 || true
  done
  installed=()
  if [ "$developer_was" = false ]; then
    "$hestia" --quiet config set developer false > /dev/null 2>&1 || true
    developer_was=""
  fi
  [ "$started_here" -eq 1 ] || return 0
  started_here=0
  "$hestia" --quiet daemon stop --keep > /dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM HUP PIPE

# A run has no plugins unless it asks for them. The official plugins are signed
# in CI, so a local build has none: `--plugins` installs each one as a dev
# plugin, read in place from the checkout and registered under the bare ids the
# signed builds use (`fabric`, not `fabric.fabric`), for this run only.
install_plugins() {
  local repo="$1" dir id
  [ -d "$repo/plugins" ] || die "no plugins/ in $repo — pass --plugins=<checkout of the plugins repository>"
  repo="$(cd "$repo" && pwd)"
  local sdk="$PWD/crates/sdk"
  developer_was="$("$hestia" --quiet config get developer 2> /dev/null | tr -d '[:space:]')"
  "$hestia" --quiet config set developer true
  for dir in "$repo"/plugins/*/; do
    [ -f "$dir/plugin.toml" ] || continue
    id="$(basename "$dir")"
    log "installing $id"
    (cd "$repo" && "$hestia" plugin dev "$dir" --official --yes --once --sdk "$sdk" > /dev/null)
    installed+=("$id")
  done
  log "${#installed[@]} plugins installed for this run; they are removed when it ends"
}

if [ "$what" = daemon ]; then
  if daemon_running; then
    log "stopping the background dev daemon"
    "$hestia" --quiet daemon stop --keep > /dev/null
    for _ in $(seq 100); do
      daemon_running || break
      sleep 0.1
    done
    daemon_running && die "the dev daemon did not stop; see \`hestia daemon status\`"
  fi
  log "hestiad in the foreground; Ctrl-C stops it"
  # Its own session, so the terminal's Ctrl-C reaches this script alone: the
  # plugins of this run are removed while the daemon is still there to ask.
  if command -v setsid > /dev/null 2>&1; then
    setsid "$bindir/hestiad" serve &
  else
    "$bindir/hestiad" serve &
  fi
  daemon_pid=$!
  started_here=1
  if [ -n "$plugins" ]; then
    for _ in $(seq 100); do
      daemon_running && break
      sleep 0.1
    done
    install_plugins "$plugins"
  fi
  wait "$daemon_pid" || true
  exit 0
fi

if [ -n "$plugins" ]; then
  ensure_daemon
  install_plugins "$plugins"
fi

if [ "$what" = desktop ]; then
  [ -d frontend/node_modules ] || (cd frontend && bun install)
  if [ "$mode" = release ]; then
    scripts/sidecars.sh --ensure
    (cd crates/desktop && cargo tauri build --no-bundle)
    ./target/release/hestia-desktop
  else
    scripts/sidecars.sh --ensure --debug
    (cd crates/desktop && cargo tauri dev)
  fi
  exit $?
fi

if [ "$#" -gt 0 ]; then
  # A one-shot leaves the daemon up: one-shots are run back to back against it.
  # `run.sh daemon …` is the exception, since it manages the daemon itself.
  [ "$1" = daemon ] || ensure_daemon
  started_here=0
  "$hestia" "$@"
  exit $?
fi

ensure_daemon
log "hestia + hestiad on PATH ($bindir), daemon running. Ctrl-D or 'exit' to leave."
"${SHELL:-bash}" -i
