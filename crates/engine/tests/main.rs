//! The engine's integration tests, linked into one binary: each `tests/*.rs`
//! would otherwise be its own binary carrying the whole engine.

mod auth_oracle;
mod fixture;
mod flows;
mod process;
mod store;
