use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::bindings::hestia::kernel::programs::{self, Output};
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;
use crate::grant::program::digest;

const PERMISSION: &str = "process:spawn";
const OUTPUT_BYTES: u64 = 1024 * 1024;

fn denied() -> PluginError {
    PluginError::Denied(PERMISSION.to_owned())
}

async fn kept(mut stream: impl AsyncRead + Unpin) -> Vec<u8> {
    let mut kept = Vec::new();
    let _ = (&mut stream)
        .take(OUTPUT_BYTES)
        .read_to_end(&mut kept)
        .await;
    let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
    kept
}

impl programs::Host for State {
    async fn run(&mut self, program: String, args: Vec<String>) -> Result<Output, PluginError> {
        let plugin = self.plugin.as_str();
        let declared = self
            .reach
            .programs
            .iter()
            .any(|invocation| invocation.program == program && invocation.args == args);
        let pin = self.reach.pins.iter().find(|pin| pin.path == program);
        let (true, Some(pin)) = (declared, pin) else {
            tracing::warn!(plugin, program, "undeclared program invocation refused");
            return Err(denied());
        };
        let file = program.clone();
        let current = tokio::task::spawn_blocking(move || digest(Path::new(&file)))
            .await
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        if current.as_deref().ok() != Some(pin.sha256.as_str()) {
            tracing::warn!(
                plugin,
                program,
                "program differs from the one approved; refused"
            );
            return Err(denied());
        }
        let mut command = Command::new(&program);
        command
            .args(&args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(data) = self
            .shared
            .services
            .data_dir(plugin)
            .filter(|dir| dir.is_dir())
        {
            command.current_dir(data);
        }
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command
            .spawn()
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(PluginError::Failed("no output pipes".into()));
        };
        let (stdout, stderr, status) = tokio::join!(kept(stdout), kept(stderr), child.wait());
        let status = status.map_err(|error| PluginError::Failed(error.to_string()))?;
        tracing::info!(plugin, program, status = ?status.code(), "plugin program ran");
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }
}
