//! Connections to programs running on the same computer, over a Unix socket
//! or a Windows named pipe.
//!
//! The plugin needs the `ipc:connect` permission and must list each endpoint
//! in the manifest's `ipc`, where a trailing `*` matches any plain characters:
//!
//! ```toml
//! permissions = ["ipc:connect"]
//! ipc = ["chat-ipc-*"]
//! ```
//!
//! The host keeps a connection open across calls, while every call runs on a
//! fresh instance of the plugin: keep the id with [`data`](crate::data), or find
//! it again with [`connections`], and take it back up with [`Connection::resume`].
//! Every connection closes when the plugin is disabled, removed or reloaded.
//!
//! ```no_run
//! use std::time::Duration;
//! use hestia::ipc::{self, Connection};
//!
//! let chat = match ipc::connections()?.pop() {
//!     Some(open) => open,
//!     None => ipc::open("chat-ipc-0")?,
//! };
//! chat.send(b"hello")?;
//! let reply = chat.receive(4096, Duration::from_millis(500))?;
//! # Ok::<(), hestia::Error>(())
//! ```

use std::time::Duration;

use crate::bindings::hestia::plugin::ipc as host;
use crate::Error;

/// One of the plugin's open connections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    id: String,
}

/// Connects to `endpoint`, a concrete name such as `chat-ipc-0`.
/// [`Error::NotFound`] when nothing listens there.
pub fn open(endpoint: &str) -> Result<Connection, Error> {
    Ok(Connection {
        id: host::open(endpoint)?,
    })
}

/// The plugin's open connections, oldest first.
pub fn connections() -> Result<Vec<Connection>, Error> {
    Ok(host::list()?
        .into_iter()
        .map(|id| Connection { id })
        .collect())
}

impl Connection {
    /// A connection an earlier call opened, by its id.
    pub fn resume(id: impl Into<String>) -> Self {
        Connection { id: id.into() }
    }

    /// The id the host knows the connection by.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Writes all of `bytes`, at most 1 MiB.
    pub fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        Ok(host::send(&self.id, bytes)?)
    }

    /// What has arrived, at most `max` bytes, waiting up to `wait` for the first
    /// byte; empty when nothing arrived in time. [`Error::NotFound`] once the
    /// peer closed the connection.
    pub fn receive(&self, max: u32, wait: Duration) -> Result<Vec<u8>, Error> {
        let wait = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        Ok(host::receive(&self.id, max, wait)?)
    }

    pub fn close(self) -> Result<(), Error> {
        Ok(host::close(&self.id)?)
    }
}
