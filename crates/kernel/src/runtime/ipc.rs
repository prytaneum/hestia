use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wasmtime::component::Resource;

use super::bindings::hestia::kernel::ipc;
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;

const PERMISSION: &str = "ipc:connect";
const WRITE_BYTES: usize = 1024 * 1024;
const READ_BYTES: u32 = 64 * 1024;
const CONNECTIONS_MAX: usize = 8;

enum Opening {
    Refused,
    Failed(std::io::Error),
}

use platform::{open, Stream};

#[cfg(unix)]
mod platform {
    use std::path::Path;

    use super::Opening;
    use crate::grant::Reserved;

    pub type Stream = tokio::net::UnixStream;

    /// A grant names a path, but what the path leads to can change after the
    /// approval, so it is judged again as the file it resolves to: a symlink, or a
    /// path through one, to Hestia's own socket is refused. A directory can still
    /// be swapped for a symlink between that check and the connect, so the path
    /// the server is bound to is read back from the connection and judged too.
    /// An unnamed or abstract address is allowed: Hestia's sockets are always
    /// bound to a path.
    pub async fn open(endpoint: &str, own: &Reserved) -> Result<Stream, Opening> {
        let path = Path::new(endpoint);
        let linked = std::fs::symlink_metadata(path)
            .map(|meta| meta.file_type().is_symlink())
            .unwrap_or(false);
        if linked || own.covers_endpoint(endpoint) {
            return Err(Opening::Refused);
        }
        let resolved = path.canonicalize().map_err(Opening::Failed)?;
        if resolved
            .to_str()
            .is_none_or(|resolved| own.covers_endpoint(resolved))
        {
            return Err(Opening::Refused);
        }
        let stream = Stream::connect(resolved).await.map_err(Opening::Failed)?;
        let bound = stream.peer_addr().map_err(Opening::Failed)?;
        if bound.as_pathname().is_some_and(|bound| {
            bound
                .to_str()
                .is_none_or(|bound| own.covers_endpoint(bound))
        }) {
            return Err(Opening::Refused);
        }
        Ok(stream)
    }
}

#[cfg(windows)]
mod platform {
    use std::time::{Duration, Instant};

    use tokio::net::windows::named_pipe::ClientOptions;

    use super::Opening;
    use crate::grant::Reserved;

    pub type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

    /// `ERROR_PIPE_BUSY`: every instance of the pipe is in use, which clears when
    /// the server accepts the next client.
    const ERROR_PIPE_BUSY: i32 = 231;
    const PIPE_BUSY_RETRY: Duration = Duration::from_millis(50);
    const PIPE_BUSY_WAIT: Duration = Duration::from_secs(5);

    pub async fn open(endpoint: &str, own: &Reserved) -> Result<Stream, Opening> {
        if own.covers_endpoint(endpoint) {
            return Err(Opening::Refused);
        }
        let deadline = Instant::now() + PIPE_BUSY_WAIT;
        loop {
            match ClientOptions::new().open(endpoint) {
                Err(error)
                    if error.raw_os_error() == Some(ERROR_PIPE_BUSY)
                        && Instant::now() < deadline =>
                {
                    tokio::time::sleep(PIPE_BUSY_RETRY).await
                }
                opened => return opened.map_err(Opening::Failed),
            }
        }
    }
}

/// An open endpoint, held in the call's resource table.
pub struct Connection(Stream);

fn failed(error: impl ToString) -> PluginError {
    PluginError::Failed(error.to_string())
}

impl ipc::Host for State {
    async fn connect(&mut self, endpoint: String) -> Result<Resource<Connection>, PluginError> {
        if !self.reach.endpoints.contains(&endpoint) {
            tracing::warn!(
                plugin = self.plugin,
                endpoint,
                "endpoint outside the grant refused"
            );
            self.log.denied("hestia:kernel/ipc", "connect", PERMISSION);
            return Err(PluginError::Denied(PERMISSION.to_owned()));
        }
        if self.connections >= CONNECTIONS_MAX {
            return Err(PluginError::Busy);
        }
        let own = self.shared.own.get().cloned().unwrap_or_default();
        let stream = match open(&endpoint, &own).await {
            Ok(stream) => stream,
            Err(Opening::Refused) => {
                tracing::warn!(
                    plugin = self.plugin,
                    endpoint,
                    "endpoint that reaches Hestia refused"
                );
                self.log.denied("hestia:kernel/ipc", "connect", PERMISSION);
                return Err(PluginError::Denied(PERMISSION.to_owned()));
            }
            Err(Opening::Failed(error)) => return Err(failed(error)),
        };
        let connection = self.table.push(Connection(stream)).map_err(failed)?;
        self.connections += 1;
        Ok(connection)
    }
}

impl ipc::HostConnection for State {
    async fn write(
        &mut self,
        this: Resource<Connection>,
        bytes: Vec<u8>,
    ) -> Result<(), PluginError> {
        if bytes.len() > WRITE_BYTES {
            return Err(PluginError::Invalid("bytes".into()));
        }
        let connection = self.table.get_mut(&this).map_err(failed)?;
        connection.0.write_all(&bytes).await.map_err(failed)
    }

    async fn read(&mut self, this: Resource<Connection>, max: u32) -> Result<Vec<u8>, PluginError> {
        if !(1..=READ_BYTES).contains(&max) {
            return Err(PluginError::Invalid("max".into()));
        }
        let connection = self.table.get_mut(&this).map_err(failed)?;
        let mut buffer = vec![0u8; max as usize];
        let read = connection.0.read(&mut buffer).await.map_err(failed)?;
        buffer.truncate(read);
        Ok(buffer)
    }

    fn drop(&mut self, this: Resource<Connection>) -> wasmtime::Result<()> {
        self.table.delete(this)?;
        self.connections -= 1;
        Ok(())
    }
}
