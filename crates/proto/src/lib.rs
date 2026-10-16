//! Wire contracts + domain types shared by both sides of the socket. Pure data:
//! no I/O, no async. `serde` derive is the marshalling layer.

pub mod accounts;
pub mod app;
pub mod backup;
pub mod cache;
pub mod config;
pub mod content;
pub mod contract;
pub mod daemon;
pub mod download;
pub mod error;
pub mod events;
pub mod files;
pub mod health;
pub mod host;
pub mod instance;
pub mod java;
pub mod job;
pub mod keys;
pub mod minecraft;
pub mod modpack;
pub mod naming;
pub mod net;
pub mod players;
pub mod process;
pub mod screenshot;
pub mod server;
pub mod skins;
pub mod sync;
pub mod tasks;
pub mod transfer;
pub mod update;
pub mod warning;

pub use contract::{Contract, Empty, Topic};
