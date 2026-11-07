//! What a plugin's `ipc` import does: connect to the local endpoints its grant
//! names and keep those connections open across calls, which each run on a
//! fresh instance. A general capability, not a service: the plugin speaks the
//! peer's protocol itself
//! ([0106](../../../../docs/decisions/0106-the-plugin-api-is-mechanisms-not-features.md)).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proto::plugin::{ipc_matches, Grant, Permission};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_CONNECTIONS: usize = 8;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error)]
pub(crate) enum IpcError {
    #[error("{0}")]
    Denied(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Failed(String),
}

trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

type Connection = Arc<tokio::sync::Mutex<Box<dyn Stream>>>;

/// One plugin's open connections. Dropped with the plugin's instance, which
/// closes them all.
pub(crate) struct Connections {
    plugin: Arc<str>,
    grant: Arc<Grant>,
    open: Mutex<HashMap<String, Connection>>,
    next: AtomicU64,
}

impl Connections {
    pub(crate) fn new(plugin: Arc<str>, grant: Arc<Grant>) -> Self {
        Connections {
            plugin,
            grant,
            open: Mutex::default(),
            next: AtomicU64::new(1),
        }
    }

    fn allowed(&self) -> Result<(), IpcError> {
        if self.grant.permissions.contains(&Permission::IpcConnect) {
            return Ok(());
        }
        Err(IpcError::Denied(
            "connecting to a local endpoint needs the `ipc:connect` permission".into(),
        ))
    }

    fn connection(&self, id: &str) -> Result<Connection, IpcError> {
        self.allowed()?;
        self.open
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| IpcError::NotFound(format!("no open connection `{id}`")))
    }

    pub(crate) async fn open(&self, endpoint: &str) -> Result<String, IpcError> {
        self.allowed()?;
        if !self
            .grant
            .ipc
            .iter()
            .any(|pattern| ipc_matches(pattern, endpoint))
        {
            return Err(IpcError::Denied(format!(
                "`{endpoint}` is not an endpoint this plugin declares"
            )));
        }
        if self.open.lock().unwrap().len() >= MAX_CONNECTIONS {
            return Err(IpcError::Invalid(format!(
                "at most {MAX_CONNECTIONS} connections may be open"
            )));
        }
        let stream = platform::dial(endpoint).await?;
        let id = self.adopt(stream);
        tracing::debug!(plugin = %self.plugin, %endpoint, connection = %id, "plugin opened a local connection");
        Ok(id)
    }

    fn adopt(&self, stream: Box<dyn Stream>) -> String {
        let id = self.next.fetch_add(1, Ordering::Relaxed).to_string();
        self.open
            .lock()
            .unwrap()
            .insert(id.clone(), Arc::new(tokio::sync::Mutex::new(stream)));
        id
    }

    pub(crate) async fn send(&self, id: &str, bytes: &[u8]) -> Result<(), IpcError> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(IpcError::Invalid(format!(
                "a message is at most {MAX_MESSAGE_BYTES} bytes"
            )));
        }
        let connection = self.connection(id)?;
        let mut stream = connection.lock().await;
        let written = async {
            stream.write_all(bytes).await?;
            stream.flush().await
        };
        if let Err(error) = written.await {
            self.forget(id);
            return Err(IpcError::Failed(format!("connection `{id}`: {error}")));
        }
        Ok(())
    }

    pub(crate) async fn receive(
        &self,
        id: &str,
        max: u32,
        timeout: Duration,
    ) -> Result<Vec<u8>, IpcError> {
        let connection = self.connection(id)?;
        let mut buffer = vec![0; (max as usize).clamp(1, MAX_MESSAGE_BYTES)];
        let mut stream = connection.lock().await;
        match tokio::time::timeout(timeout.min(MAX_WAIT), stream.read(&mut buffer)).await {
            Err(_) => Ok(Vec::new()),
            Ok(Ok(0)) => {
                self.forget(id);
                Err(IpcError::NotFound(format!(
                    "connection `{id}` was closed by its peer"
                )))
            }
            Ok(Ok(read)) => {
                buffer.truncate(read);
                Ok(buffer)
            }
            Ok(Err(error)) => {
                self.forget(id);
                Err(IpcError::Failed(format!("connection `{id}`: {error}")))
            }
        }
    }

    pub(crate) fn close(&self, id: &str) -> Result<(), IpcError> {
        self.connection(id)?;
        self.forget(id);
        Ok(())
    }

    pub(crate) fn list(&self) -> Result<Vec<String>, IpcError> {
        self.allowed()?;
        let mut ids: Vec<String> = self.open.lock().unwrap().keys().cloned().collect();
        ids.sort_by_key(|id| id.parse::<u64>().unwrap_or(u64::MAX));
        Ok(ids)
    }

    /// Closes every connection; used when the plugin is taken out of service.
    pub(crate) fn close_all(&self) {
        self.open.lock().unwrap().clear();
    }

    fn forget(&self, id: &str) {
        self.open.lock().unwrap().remove(id);
    }
}

#[cfg(unix)]
mod platform {
    use std::path::PathBuf;

    use super::{IpcError, Stream};

    /// Where per-user endpoints live: the runtime directory first, then the
    /// temp directories a process may have been told to use.
    fn bases() -> Vec<PathBuf> {
        let mut bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
            .iter()
            .filter_map(std::env::var_os)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect();
        bases.push(PathBuf::from("/tmp"));
        bases.dedup();
        bases
    }

    pub(super) async fn dial(endpoint: &str) -> Result<Box<dyn Stream>, IpcError> {
        dial_in(&bases(), endpoint).await
    }

    pub(super) async fn dial_in(
        bases: &[PathBuf],
        endpoint: &str,
    ) -> Result<Box<dyn Stream>, IpcError> {
        for base in bases {
            let path = base.join(endpoint);
            if !path.exists() {
                continue;
            }
            match tokio::net::UnixStream::connect(&path).await {
                Ok(stream) => return Ok(Box::new(stream)),
                Err(error) => {
                    tracing::debug!(path = %path.display(), %error, "local endpoint refused a connection");
                }
            }
        }
        Err(IpcError::NotFound(format!(
            "nothing listens at `{endpoint}`"
        )))
    }
}

#[cfg(windows)]
mod platform {
    use tokio::net::windows::named_pipe::ClientOptions;

    use super::{IpcError, Stream};

    pub(super) async fn dial(endpoint: &str) -> Result<Box<dyn Stream>, IpcError> {
        if endpoint.contains('/') {
            return Err(IpcError::NotFound(format!(
                "`{endpoint}` names a socket path, which Windows does not have"
            )));
        }
        ClientOptions::new()
            .open(format!(r"\\.\pipe\{endpoint}"))
            .map(|pipe| Box::new(pipe) as Box<dyn Stream>)
            .map_err(|error| {
                IpcError::NotFound(format!("nothing listens at `{endpoint}`: {error}"))
            })
    }
}

#[cfg(test)]
mod tests;
