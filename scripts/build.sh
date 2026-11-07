#!/usr/bin/env bash
# Thin cargo wrapper. Usage:
#   scripts/build.sh [cli|daemon|desktop|all] [extra cargo flags...]
# Examples:
#   scripts/build.sh cli --release
#   scripts/build.sh all
. "$(dirname "$0")/lib/common.sh"

target="${1:-all}"
shift || true

# A running hestiad holds the previous build in memory; stop every instance so
# the next client auto-spawns the freshly built daemon. SIGTERM is the daemon's
# graceful stop, so any supervised workloads keep running. This is the POSIX
# path — on Windows, win.ps1 stops the daemon natively before invoking us (Git
# Bash has no pkill, and a live hestiad.exe would hard-lock the overwrite).
stop_hestia() {
  pkill -TERM -x hestiad 2>/dev/null || true
  for _ in {1..20}; do
    command -v pgrep >/dev/null 2>&1 || return 0
    pgrep -x hestiad >/dev/null 2>&1 || return 0
    sleep 0.25
  done
}
stop_hestia

# The staged sidecars must match the requested profile: a debug build bundles
# debug sidecars (dev data under target/debug/data), a --release one the release
# set. sidecars.sh defaults to release, so forward the profile explicitly.
sidecar_profile="--debug"
for arg in "$@"; do
  [ "$arg" = "--release" ] && sidecar_profile="--release"
done

# Compiling the desktop crate requires the Tauri externalBin sidecars to be
# staged, or its build script fails on the missing resource paths.
case "$target" in
  cli|daemon) cargo build "$@" ;;
  desktop) scripts/sidecars.sh --ensure "$sidecar_profile"
           (cd crates/desktop && cargo tauri build "$@") ;;
  all)     scripts/sidecars.sh --ensure "$sidecar_profile"
           cargo build "$@"
           cargo build -p desktop "$@" ;;
  *) echo "usage: $0 [cli|daemon|desktop|all] [cargo flags]" >&2; exit 1 ;;
esac
