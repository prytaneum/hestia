use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::bindings::hestia::kernel::programs::{self, Output};
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;
use super::tree::{isolate, Tree};
use crate::grant::digest;

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
        let granted = self
            .reach
            .programs
            .iter()
            .find(|granted| granted.path == program && granted.args == args);
        let Some(granted) = granted else {
            tracing::warn!(plugin, program, "undeclared program invocation refused");
            self.log.denied("hestia:kernel/programs", "run", PERMISSION);
            return Err(denied());
        };
        let (file, pinned) = (program.clone(), granted.sha256.clone());
        let current = tokio::task::spawn_blocking(move || digest(file.as_ref()))
            .await
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        if current.as_deref().ok() != Some(pinned.as_str()) {
            tracing::warn!(
                plugin,
                program,
                "program differs from the one approved; refused"
            );
            self.log.denied("hestia:kernel/programs", "run", PERMISSION);
            return Err(denied());
        }
        let data = self
            .shared
            .services
            .data_dir(plugin)
            .filter(|dir| dir.is_dir())
            .ok_or_else(|| PluginError::Failed("the plugin has no data directory".into()))?;
        let mut command = Command::new(&program);
        command
            .args(&args)
            .current_dir(data)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        isolate(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        let tree = Tree::of(&child).map_err(|error| PluginError::Failed(error.to_string()))?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(PluginError::Failed("no output pipes".into()));
        };
        let finished = async {
            let status = child.wait().await;
            tree.kill();
            status
        };
        let (stdout, stderr, status) = tokio::join!(kept(stdout), kept(stderr), finished);
        let status = status.map_err(|error| PluginError::Failed(error.to_string()))?;
        tracing::info!(plugin, program, status = ?status.code(), "plugin program ran");
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }
}
