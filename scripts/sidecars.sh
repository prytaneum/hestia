#!/usr/bin/env bash
# Build the CLI, daemon, and tray and stage them as Tauri sidecars, so
# `cargo tauri build` bundles them into every desktop installer.
#
# Release is the default (what installers ship). `--debug` stages debug builds
# instead: the daemon's `debug_assertions` then keep dev data under target/debug/data
# rather than the real ~/.hestia — used by `scripts/dev.sh --desktop`.
#
# Tauri's externalBin requires each binary to carry the target-triple suffix
# (e.g. hestiad-x86_64-unknown-linux-gnu); the installer strips it on install.
#
#   scripts/sidecars.sh                 # host target, release
#   scripts/sidecars.sh <target-triple> # cross target (passed to cargo --target)
#   scripts/sidecars.sh --debug         # host target, debug (dev sidecars)
#   scripts/sidecars.sh --ensure        # ensure the staged set is current
. "$(dirname "$0")/lib/common.sh"

profile="release"
triple=""
for arg in "$@"; do
  case "$arg" in
    --ensure) ;;
    --debug) profile="debug" ;;
    --release) profile="release" ;;
    *) triple="$arg" ;;
  esac
done

cross=1
[ -n "$triple" ] || { triple="$(rustc -vV | sed -n 's/^host: //p')"; cross=0; }
[ -n "$triple" ] || die "could not determine target triple"

build_args=()
[ "$profile" = "release" ] && build_args=(--release)

target_args=()
srcdir="target/$profile"
if [ "$cross" -eq 1 ]; then
  target_args=(--target "$triple")
  srcdir="target/$triple/$profile"
fi

ext=""
case "$triple" in
  *windows*) ext=".exe" ;;
esac

dest="crates/desktop/binaries"

log "building $profile sidecars for $triple"
cargo build "${build_args[@]}" "${target_args[@]}"

mkdir -p "$dest"
for bin in hestia hestiad hestia-tray; do
  cp "$srcdir/$bin$ext" "$dest/$bin-$triple$ext"
  echo "  staged $dest/$bin-$triple$ext"
done
