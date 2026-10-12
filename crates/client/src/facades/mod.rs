//! One typed facade per domain, reached through `Client` accessors. Facade
//! methods are thin wrappers over `Session::call`, returning `proto` types
//! directly — mirroring the engine's domain modules on the other side of the
//! socket.

mod accounts;
mod app;
mod cache;
mod config;
mod content;
mod daemon;
mod files;
mod instance;
mod java;
mod jobs;
mod modpack;
mod net;
mod players;
mod process;
mod screenshot;
mod server;
mod skins;
mod sync;
mod transfer;
mod update;

pub use accounts::Accounts;
pub use app::App;
pub use cache::Cache;
pub use config::Config;
pub use content::Content;
pub use daemon::Daemon;
pub use files::Files;
pub use instance::{Instance, LaunchOptions};
pub use java::Java;
pub use modpack::Modpack;
pub use net::Net;
pub use players::Players;
pub use process::{Process, ProcessEvent};
pub use screenshot::Screenshot;
pub use server::Server;
pub use skins::Skins;
pub use sync::Sync;
pub use transfer::{Exported, Imported, Transfer};
pub use update::Update;
