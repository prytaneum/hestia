#!/usr/bin/env bash
#
# dev.sh — a terminal-first dev shell for the CLI + daemon.
#
#   scripts/dev.sh                  build daemon, tray + CLI (debug), open a subshell
#                                   with `hestia`/`hestiad` on PATH
#   scripts/dev.sh <hestia-args>    build, then run `hestia <args>` once
#   scripts/dev.sh --release <hestia-args>  build release binaries, then run `hestia <args>` once
#
# The CLI auto-spawns the sibling daemon, so `hestia java list` just works.
# Debug builds keep data under target/<profile>/data, so this never touches ~/.hestia.
. "$(dirname "$0")/lib/common.sh"

# Ignore an installed hestia entirely while developing: drop PATH entries that
# carry one, and pin a dev-only daemon endpoint so the dev CLI never reaches
# (or, via the exit trap, stops) an installed daemon.
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
# On Windows the dev endpoint is a named pipe, supplied by win.ps1 before it
# forwards here.
export HESTIA_SOCK="${HESTIA_SOCK:-${XDG_RUNTIME_DIR:-/tmp}/hestiad-dev-$(id -u).sock}"

mode=debug
flags=()

while [ "$#" -gt 0 ]; do
  case "$1" in
    --release) mode=release; flags=(--release); shift ;;
    *) break ;;
  esac
done

log "Building daemon + tray + CLI ($mode)"
cargo build "${flags[@]}"
bindir="$PWD/target/$mode"

# Nothing below `exec`s: bash does not run an EXIT trap across exec, so the dev
# daemon the subshell starts would outlive this script. The signals matter as
# much as EXIT: `dev.sh … | head` dies of SIGPIPE, and an untrapped one skips
# EXIT entirely.
cleanup() { :; }
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM HUP PIPE

# One-shot: `dev.sh java list` runs the CLI once and exits. It leaves the daemon
# up on purpose — one-shots are run back to back against the same one.
if [ "$#" -gt 0 ]; then
  "$bindir/hestia" "$@"
  exit $?
fi

log "hestia + hestiad on PATH ($bindir). Ctrl-D / 'exit' to leave."
# Redefined rather than re-trapped, so the signal handlers above pick this up
# too: leaving the subshell is also where the dev daemon should stop.
cleanup() {
  "$bindir/hestia" daemon stop > /dev/null 2>&1 || true
}
PATH="$bindir:$PATH" HESTIA_DEV=1 "${SHELL:-bash}" -i
