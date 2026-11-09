//! The Windows endpoint. A named pipe dies with the process that created it, so
//! a detached relay (`hestiad stdin-relay`) parents the real program, owns its
//! stdin, and serves the pipe the daemon reconnects to after a restart. As the
//! parent it also keeps the program inside the supervisor's job object.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use proto::process::ProcessSpec;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};
use tokio::process::{ChildStdin, Command};
use tokio::sync::Mutex;

use super::{validate_line, Launch, MAX_LINE_BYTES};

const ERROR_PIPE_BUSY: i32 = 231;
const CONNECT_RETRIES: u32 = 40;
const CONNECT_BACKOFF: Duration = Duration::from_millis(50);
const MAX_CONNECTION_BYTES: u64 = 64 * 1024;

fn pipe_name(id: &str) -> String {
    let scope = common::paths::install_scope()
        .map(|scope| format!("{scope}-"))
        .unwrap_or_default();
    format!(r"\\.\pipe\hestia-{scope}{id}")
}

pub fn create(
    spec: &ProcessSpec,
    _proc_dir: &Path,
    relay_program: Option<&Path>,
) -> std::io::Result<Launch> {
    let relay_program = relay_program
        .ok_or_else(|| std::io::Error::other("no stdin relay program is configured"))?;
    let target = pipe_name(&spec.id);
    let mut args = vec![
        "stdin-relay".to_string(),
        "--pipe".to_string(),
        target.clone(),
        "--".to_string(),
        spec.program.clone(),
    ];
    args.extend(spec.args.iter().cloned());
    Ok(Launch {
        target,
        stdin: Stdio::null(),
        spec: ProcessSpec {
            program: relay_program.to_string_lossy().into_owned(),
            args,
            ..spec.clone()
        },
    })
}

pub async fn write(target: &str, line: &str) -> std::io::Result<()> {
    let mut attempt = 0;
    let mut client = loop {
        match ClientOptions::new().open(target) {
            Ok(client) => break client,
            Err(e)
                if attempt < CONNECT_RETRIES
                    && (e.raw_os_error() == Some(ERROR_PIPE_BUSY)
                        || e.kind() == std::io::ErrorKind::NotFound) =>
            {
                attempt += 1;
                tokio::time::sleep(CONNECT_BACKOFF).await;
            }
            Err(e) => return Err(e),
        }
    };
    client.write_all(format!("{line}\n").as_bytes()).await?;
    client.flush().await
}

pub fn remove(_target: &str) -> bool {
    false
}

/// Run the relay: spawn `program` with an anonymous pipe as stdin, serve `pipe`,
/// forward each received line to it, and return the program's exit code.
pub async fn run_relay(pipe: &str, program: &str, args: &[String]) -> std::io::Result<i32> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;
    let stdin = Arc::new(Mutex::new(
        child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("child has no stdin"))?,
    ));
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(pipe)?;
    tracing::debug!("stdin relay serving its pipe");
    loop {
        tokio::select! {
            status = child.wait() => {
                let code = status?.code().unwrap_or(1);
                tracing::debug!(code, "relayed program exited");
                return Ok(code);
            }
            connected = server.connect() => {
                connected?;
                let connection = std::mem::replace(
                    &mut server,
                    ServerOptions::new().reject_remote_clients(true).create(pipe)?,
                );
                tokio::spawn(forward(connection, stdin.clone()));
            }
        }
    }
}

async fn forward(connection: impl tokio::io::AsyncRead + Unpin, stdin: Arc<Mutex<ChildStdin>>) {
    let mut lines = BufReader::new(connection.take(MAX_CONNECTION_BYTES)).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if let Err(e) = validate_line(&line) {
            tracing::warn!(
                len = line.len().min(MAX_LINE_BYTES + 1),
                "relay dropped a line: {e}"
            );
            continue;
        }
        let mut stdin = stdin.lock().await;
        if let Err(e) = stdin.write_all(format!("{line}\n").as_bytes()).await {
            tracing::warn!("relay cannot write to the program's stdin: {e}");
            return;
        }
        let _ = stdin.flush().await;
    }
}
