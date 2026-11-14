#!/usr/bin/env bash
# Thin cargo wrapper. Usage:
#   scripts/build.sh [cli|daemon|desktop|all] [extra cargo flags...]
# Examples:
#   scripts/build.sh cli --release
#   scripts/build.sh all
. "$(dirname "$0")/lib/common.sh"

target="${1:-all}"
shift || true

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
