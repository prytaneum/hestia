//! Cross-cutting: logging, app identity, path resolution. Linked by the daemon
//! and every client; zero UI or domain dependencies.

pub mod app;
pub mod appimage;
pub mod atomic;
pub mod crash;
pub mod logging;
pub mod paths;
pub mod process;
mod time;

pub use logging::{init_logging, FileLog, LogGuard, LogLevel};
pub use time::{local_midnight_unix, modified_unix, now_unix};
