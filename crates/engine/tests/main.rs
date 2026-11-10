//! The engine's integration tests, linked into one binary: each `tests/*.rs`
//! would otherwise be its own binary carrying the whole engine and wasmtime.

mod auth_oracle;
mod fixture;
mod flows;
mod official;
mod plugins;
mod process;
mod store;
