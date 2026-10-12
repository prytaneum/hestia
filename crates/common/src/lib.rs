//! Cross-cutting: logging, app identity, path resolution. Linked by the daemon
//! and every client; zero UI or domain dependencies.

pub mod app;
pub mod crash;
pub mod logging;
pub mod paths;
mod time;

pub use logging::{init_logging, FileLog, LogGuard, LogLevel};
pub use time::local_midnight_unix;
