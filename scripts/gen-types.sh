#!/usr/bin/env bash
# Regenerate the TypeScript bindings for the proto wire types (ts-rs). The flat
# per-type files land in frontend/src/api/types/generated/, wrapped by one
# per-module barrel each, plus channels.ts — the typed table of every contract,
# job and topic; all are committed. Run this after changing any
# `#[derive(ts_rs::TS)]` type or `impl Contract` in `crates/proto`.
. "$(dirname "$0")/lib/common.sh"

gen_dir="$PWD/frontend/src/api/types/generated"

# The export dir and the i64/u64 → number mapping live in .cargo/config.toml so
# the annotations stay a bare `#[ts(export)]`.
log "generating TypeScript bindings from proto…"
rm -rf "$gen_dir"
cargo test -p proto --features ts >/dev/null

python scripts/lib/gen-barrels.py
python scripts/lib/gen-channels.py

log "wrote $gen_dir, per-module barrels and the channel table"
