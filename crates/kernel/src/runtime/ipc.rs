use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::bindings::hestia::kernel::ipc;
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;

const PERMISSION: &str = "ipc:connect";
const REQUEST_BYTES: usize = 1024 * 1024;
const ANSWER_BYTES: u64 = 1024 * 1024;

#[cfg(unix)]
async fn connect(endpoint: &str) -> std::io::Result<impl AsyncReadExt + AsyncWriteExt + Unpin> {
    tokio::net::UnixStream::connect(endpoint).await
}

#[cfg(windows)]
async fn connect(endpoint: &str) -> std::io::Result<impl AsyncReadExt + AsyncWriteExt + Unpin> {
    tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint)
}

impl ipc::Host for State {
    async fn request(
        &mut self,
        endpoint: String,
        request: Vec<u8>,
    ) -> Result<Vec<u8>, PluginError> {
        if !self.reach.endpoints.contains(&endpoint) {
            tracing::warn!(
                plugin = self.plugin,
                endpoint,
                "endpoint outside the grant refused"
            );
            return Err(PluginError::Denied(PERMISSION.to_owned()));
        }
        if request.len() > REQUEST_BYTES {
            return Err(PluginError::Invalid("request".into()));
        }
        let failed = |error: std::io::Error| PluginError::Failed(error.to_string());
        let mut stream = connect(&endpoint).await.map_err(failed)?;
        stream.write_all(&request).await.map_err(failed)?;
        stream.shutdown().await.map_err(failed)?;
        let mut answer = Vec::new();
        stream
            .take(ANSWER_BYTES)
            .read_to_end(&mut answer)
            .await
            .map_err(failed)?;
        Ok(answer)
    }
}
