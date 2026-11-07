#!/usr/bin/env bash
# Build then run a binary. Usage:
#   scripts/run.sh cli [args...]         # the hestia CLI
#   scripts/run.sh daemon [args...]      # hestiad
#   scripts/run.sh desktop               # the Tauri shell against the Vite dev server
#   scripts/run.sh --release daemon serve   # run the release build instead of debug
. "$(dirname "$0")/lib/common.sh"

profile=""
rest=()
for arg in "$@"; do
  case "$arg" in
    --release) profile="--release" ;;
    *) rest+=("$arg") ;;
  esac
done
set -- "${rest[@]}"

target="${1:-cli}"
shift || true

case "$target" in
  cli)     cargo run $profile --bin hestia -- "$@" ;;
  daemon)  cargo build $profile
           cargo run $profile --bin hestiad -- "$@" ;;
  desktop)
    if [ -n "$profile" ]; then
      scripts/sidecars.sh --ensure
      (cd crates/desktop && cargo tauri build --no-bundle)
      exec ./target/release/hestia-desktop
    else
      scripts/sidecars.sh --ensure --debug
      (cd crates/desktop && cargo tauri dev)
    fi
    ;;
  *) die "usage: $0 [--release] [cli|daemon|desktop] [args]" ;;
esac
