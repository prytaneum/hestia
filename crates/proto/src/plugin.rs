//! The plugin ABI version. The WIT package in `crates/sdk/wit` is the single
//! source; `tests/plugin.rs` fails if this constant drifts from it.

pub const API_VERSION: &str = "0.1.0";
