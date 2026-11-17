//! IPC transport + protocol envelope: the one public boundary between the daemon
//! and every client. Nothing domain-specific lives here.

pub mod endpoint;
pub mod errors;
#[cfg(windows)]
mod identity;
pub mod protocol;
pub mod transport;

pub use errors::IpcError;
pub use protocol::{DecodeError, Event, Request, Response, PROTOCOL_VERSION};
pub use transport::{bind, connect, Connection, FrameReader, FrameWriter, Listener, Peer};
