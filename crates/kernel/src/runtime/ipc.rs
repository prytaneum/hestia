use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wasmtime::component::Resource;

use super::bindings::hestia::kernel::ipc;
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;

const PERMISSION: &str = "ipc:connect";
const WRITE_BYTES: usize = 1024 * 1024;
const READ_BYTES: u32 = 64 * 1024;
const CONNECTIONS_MAX: usize = 8;

#[cfg(unix)]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(unix)]
async fn open(endpoint: &str) -> std::io::Result<Stream> {
    Stream::connect(endpoint).await
}

/// `ERROR_PIPE_BUSY`: every instance of the pipe is in use, which clears when
/// the server accepts the next client. The call's own budget ends the wait.
#[cfg(windows)]
const ERROR_PIPE_BUSY: i32 = 231;
#[cfg(windows)]
const PIPE_BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(50);

#[cfg(windows)]
async fn open(endpoint: &str) -> std::io::Result<Stream> {
    use tokio::net::windows::named_pipe::ClientOptions;
    loop {
        match ClientOptions::new().open(endpoint) {
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(PIPE_BUSY_RETRY).await
            }
            opened => return opened,
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
            return Err(PluginError::Denied(PERMISSION.to_owned()));
        }
        if self.connections >= CONNECTIONS_MAX {
            return Err(PluginError::Busy);
        }
        let stream = open(&endpoint).await.map_err(failed)?;
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
