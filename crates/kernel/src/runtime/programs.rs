use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};

use super::bindings::hestia::kernel::programs::{self, Output};
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;
use super::launch::Program;
use super::tree::{isolate, Tree};

const PERMISSION: &str = "process:spawn";
const OUTPUT_BYTES: u64 = 1024 * 1024;
/// How long output still in flight is read after the program ended; a process
/// that left its group (`setsid`) may hold the pipes open for good.
const DRAIN: Duration = Duration::from_millis(250);

fn denied() -> PluginError {
    PluginError::Denied(PERMISSION.to_owned())
}

async fn kept(mut stream: impl AsyncRead + Unpin, into: &mut Vec<u8>) {
    let _ = (&mut stream).take(OUTPUT_BYTES).read_to_end(into).await;
    let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
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
        let verified = tokio::task::spawn_blocking(move || Program::verify(&file, &pinned))
            .await
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        let Some(verified) = verified.ok().flatten() else {
            tracing::warn!(
                plugin,
                program,
                "program differs from the one approved; refused"
            );
            self.log.denied("hestia:kernel/programs", "run", PERMISSION);
            return Err(denied());
        };
        let data = self
            .shared
            .services
            .data_dir(plugin)
            .filter(|dir| dir.is_dir())
            .ok_or_else(|| PluginError::Failed("the plugin has no data directory".into()))?;
        let mut command = verified.command(&data);
        command
            .args(&args)
            .current_dir(data)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        isolate(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        let tree = Tree::of(&child).map_err(|error| PluginError::Failed(error.to_string()))?;
        tree.start(&child)
            .map_err(|error| PluginError::Failed(error.to_string()))?;
        drop(verified);
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(PluginError::Failed("no output pipes".into()));
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (exited, ended) = tokio::sync::oneshot::channel();
        let finished = async {
            let status = child.wait().await;
            tree.kill();
            let _ = exited.send(());
            status
        };
        let drained = async {
            let readers = async { tokio::join!(kept(stdout, &mut out), kept(stderr, &mut err)) };
            let abandoned = async {
                let _ = ended.await;
                tokio::time::sleep(DRAIN).await;
            };
            tokio::select! {
                _ = readers => {}
                _ = abandoned => tracing::warn!(plugin, program, "output cut off: something outside the program's group holds its pipes"),
            }
        };
        let (status, ()) = tokio::join!(finished, drained);
        let status = status.map_err(|error| PluginError::Failed(error.to_string()))?;
        tracing::info!(plugin, program, status = ?status.code(), "plugin program ran");
        Ok(Output {
            status: status.code().unwrap_or(-1),
            stdout: out,
            stderr: err,
        })
    }
}
