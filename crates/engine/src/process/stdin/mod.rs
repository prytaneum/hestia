//! A stdin endpoint a supervised process keeps for its whole life, reopenable
//! by a later daemon from the process record. Unix uses a FIFO the child holds
//! open read-write (so it never sees EOF); Windows uses a relay process that
//! owns the child's stdin and serves a named pipe, since a pipe dies with the
//! process that created it.

#[cfg(unix)]
mod fifo;
#[cfg(windows)]
mod relay;

#[cfg(unix)]
use fifo as platform;
#[cfg(windows)]
use relay as platform;

#[cfg(windows)]
pub use relay::run_relay;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use proto::process::ProcessSpec;

pub const MAX_LINE_BYTES: usize = 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub enum StdinError {
    #[error("a console line is {0}")]
    InvalidLine(&'static str),
    #[error("process '{0}' has no stdin endpoint")]
    NoEndpoint(String),
    #[error("process '{0}' is not running")]
    NotRunning(String),
    #[error("cannot write to the stdin endpoint: {0}")]
    Io(#[from] std::io::Error),
}

pub fn validate_line(line: &str) -> Result<(), StdinError> {
    if line.is_empty() {
        return Err(StdinError::InvalidLine("empty"));
    }
    if line.len() > MAX_LINE_BYTES {
        return Err(StdinError::InvalidLine("too long"));
    }
    if line.chars().any(char::is_control) {
        return Err(StdinError::InvalidLine("not a single plain line"));
    }
    Ok(())
}

pub(super) struct Launch {
    pub target: String,
    pub stdin: Stdio,
    /// What to actually spawn: the spec itself, or the relay wrapping it.
    pub spec: ProcessSpec,
}

pub(super) fn create(
    spec: &ProcessSpec,
    proc_dir: &Path,
    relay_program: Option<&Path>,
) -> std::io::Result<Launch> {
    let launch = platform::create(spec, proc_dir, relay_program)?;
    tracing::debug!(id = %spec.id, "created stdin endpoint");
    Ok(launch)
}

pub(super) async fn write_line(target: &str, line: &str) -> Result<(), StdinError> {
    validate_line(line)?;
    tracing::debug!(len = line.len(), "writing to stdin endpoint");
    match tokio::time::timeout(WRITE_TIMEOUT, platform::write(target, line)).await {
        Ok(result) => Ok(result?),
        Err(_) => Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into()),
    }
}

pub(super) fn remove(target: &str) {
    if platform::remove(target) {
        tracing::debug!("removed stdin endpoint");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_plain_line() {
        assert!(validate_line("list").is_ok());
        assert!(validate_line(&"a".repeat(MAX_LINE_BYTES)).is_ok());
    }

    #[test]
    fn refuses_what_is_not_one_plain_line() {
        for bad in ["", "a\nb", "a\r", "tab\there", "bell\u{7}", "nul\0"] {
            assert!(validate_line(bad).is_err(), "{bad:?}");
        }
        assert!(validate_line(&"a".repeat(MAX_LINE_BYTES + 1)).is_err());
    }
}
