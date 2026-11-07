use std::sync::Arc;

use async_trait::async_trait;
use proto::plugin::{Grant, Permission, PluginRunProgress};
use serde_json::Value;
use tokio::sync::watch;
use tracing::Level;
use wasmtime::component::ResourceTable;
use wasmtime::StoreLimits;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{WasiHttpCtx, WasiHttpCtxView, WasiHttpView};

use super::chain::CallChain;
use super::http::PluginHttp;
use super::ipc::{Connections, IpcError};
use super::output::PluginLog;
use super::process::{ProgramError, Programs};
use super::settings::PluginSettings;
use crate::cancel::Cancel;

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "../sdk/wit",
        world: "plugin",
        imports: { default: async },
        exports: { default: async },
    });
}

use bindings::hestia::plugin::types::PluginError;

/// Why the daemon could not serve a plugin's `host.call` or `host.run-job`.
#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Failed(String),
}

/// What the daemon lends a plugin: its channels and jobs. Implemented by the
/// daemon so the engine never learns the router exists.
#[async_trait]
pub trait HostCalls: Send + Sync {
    /// The permission `channel` needs, or `None` when plugins may not reach it.
    fn permission(&self, channel: &str) -> Option<Permission>;

    /// `chain` already includes `plugin`; the daemon carries it in the request
    /// it synthesises so anything that fires a plugin in turn can refuse a loop.
    async fn call(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError>;

    /// Starts a job channel and resolves with its `Done` payload.
    async fn run_job(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError>;

    /// A line a dev plugin logged, already rate-limited and truncated.
    fn dev_log(&self, _plugin: &str, _level: &str, _text: &str) {}
}

const PROGRESS_MESSAGE_CHARS: usize = 512;

/// What a command run lends the guest: whether it was asked to stop, and where
/// its progress goes. Progress keeps only the latest report, so a guest
/// reporting in a tight loop cannot queue memory in the daemon.
pub struct CommandRun {
    pub cancel: Cancel,
    pub progress: watch::Sender<Option<PluginRunProgress>>,
    /// How long the guest may keep running after the cancel before its call is
    /// dropped.
    pub grace: std::time::Duration,
}

/// Per-call store data; dropped with the call.
pub(crate) struct PluginState {
    pub(crate) run: Option<Arc<CommandRun>>,
    pub(crate) wasi: WasiCtx,
    pub(crate) http: WasiHttpCtx,
    pub(crate) http_hooks: PluginHttp,
    pub(crate) table: ResourceTable,
    pub(crate) limits: StoreLimits,
    pub(crate) log: Arc<PluginLog>,
    pub(crate) settings: Arc<PluginSettings>,
    pub(crate) gate: Arc<Gate>,
    pub(crate) programs: Arc<Programs>,
    pub(crate) ipc: Arc<Connections>,
}

/// What a `host.call` is checked against. Apart from [`PluginState`] so an
/// awaiting import holds only this: the WASI context is not `Sync`.
pub(crate) struct Gate {
    pub(crate) plugin: Arc<str>,
    pub(crate) host: Arc<dyn HostCalls>,
    pub(crate) grant: Arc<Grant>,
    pub(crate) chain: CallChain,
    pub(crate) log: Arc<PluginLog>,
}

impl WasiView for PluginState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl WasiHttpView for PluginState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: &mut self.http_hooks,
        }
    }
}

enum Dispatch {
    Call,
    RunJob,
}

impl Gate {
    async fn dispatch(
        &self,
        kind: Dispatch,
        channel: &str,
        payload: &str,
    ) -> Result<String, PluginError> {
        let Some(needed) = self.host.permission(channel) else {
            self.log
                .line(Level::WARN, &format!("reached closed channel `{channel}`"));
            return Err(PluginError::PermissionDenied(format!(
                "channel `{channel}` is not available to plugins"
            )));
        };
        if !self.grant.permissions.contains(&needed) {
            self.log.line(
                Level::WARN,
                &format!("lacks the permission for channel `{channel}`"),
            );
            return Err(PluginError::PermissionDenied(format!(
                "channel `{channel}` needs the `{needed}` permission"
            )));
        }
        let payload: Value = serde_json::from_str(payload)
            .map_err(|error| PluginError::Invalid(format!("payload is not JSON: {error}")))?;
        let reply = match kind {
            Dispatch::Call => {
                self.host
                    .call(&self.plugin, &self.chain, channel, payload)
                    .await
            }
            Dispatch::RunJob => {
                self.host
                    .run_job(&self.plugin, &self.chain, channel, payload)
                    .await
            }
        };
        match reply {
            Ok(value) => Ok(value.to_string()),
            Err(HostError::NotFound(message)) => Err(PluginError::NotFound(message)),
            Err(HostError::Invalid(message)) => Err(PluginError::Invalid(message)),
            Err(HostError::Failed(message)) => Err(PluginError::Failed(message)),
        }
    }
}

impl bindings::hestia::plugin::host::Host for PluginState {
    async fn call(&mut self, channel: String, payload: String) -> Result<String, PluginError> {
        let gate = self.gate.clone();
        gate.dispatch(Dispatch::Call, &channel, &payload).await
    }

    async fn run_job(&mut self, channel: String, payload: String) -> Result<String, PluginError> {
        let gate = self.gate.clone();
        gate.dispatch(Dispatch::RunJob, &channel, &payload).await
    }
}

impl bindings::hestia::plugin::progress::Host for PluginState {
    async fn report(&mut self, fraction: Option<f32>, message: String) {
        if let Some(run) = &self.run {
            run.progress.send_replace(Some(PluginRunProgress {
                fraction: fraction
                    .filter(|fraction| fraction.is_finite())
                    .map(|fraction| fraction.clamp(0.0, 1.0)),
                message: message.chars().take(PROGRESS_MESSAGE_CHARS).collect(),
            }));
        }
    }

    async fn cancelled(&mut self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.cancel.is_cancelled())
    }
}

impl bindings::hestia::plugin::settings::Host for PluginState {
    async fn get(&mut self, key: String) -> Option<String> {
        self.settings.get(&key).map(|value| value.to_string())
    }
}

impl bindings::hestia::plugin::log::Host for PluginState {
    async fn trace(&mut self, message: String) {
        self.log.line(Level::TRACE, &message);
    }

    async fn debug(&mut self, message: String) {
        self.log.line(Level::DEBUG, &message);
    }

    async fn info(&mut self, message: String) {
        self.log.line(Level::INFO, &message);
    }

    async fn warn(&mut self, message: String) {
        self.log.line(Level::WARN, &message);
    }

    async fn error(&mut self, message: String) {
        self.log.line(Level::ERROR, &message);
    }
}

impl bindings::hestia::plugin::types::Host for PluginState {}

impl From<ProgramError> for PluginError {
    fn from(error: ProgramError) -> Self {
        match error {
            ProgramError::Denied(message) => PluginError::PermissionDenied(message),
            ProgramError::NotFound(message) => PluginError::NotFound(message),
            ProgramError::Invalid(message) => PluginError::Invalid(message),
            ProgramError::Failed(message) => PluginError::Failed(message),
        }
    }
}

impl bindings::hestia::plugin::process::Host for PluginState {
    async fn exec(
        &mut self,
        program: String,
        args: Vec<String>,
        timeout_ms: u32,
    ) -> Result<bindings::hestia::plugin::process::ExecOutput, PluginError> {
        let programs = self.programs.clone();
        let output = programs.exec(&program, args, timeout_ms).await?;
        Ok(bindings::hestia::plugin::process::ExecOutput {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    async fn spawn(
        &mut self,
        program: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    ) -> Result<String, PluginError> {
        let programs = self.programs.clone();
        Ok(programs.spawn(&program, args, env).await?)
    }

    async fn stop(&mut self, id: String) -> Result<(), PluginError> {
        Ok(self.programs.stop(&id)?)
    }

    async fn status(
        &mut self,
        id: String,
    ) -> Result<bindings::hestia::plugin::process::ProcessStatus, PluginError> {
        use bindings::hestia::plugin::process::{ProcessStatus, State};
        use proto::process::ProcessState;
        let info = self.programs.status(&id)?;
        Ok(ProcessStatus {
            state: match info.state {
                ProcessState::Running => State::Running,
                ProcessState::Exited => State::Exited,
                ProcessState::Killed => State::Killed,
            },
            exit_code: info.exit_code,
        })
    }

    async fn logs(&mut self, id: String, tail: u32) -> Result<Vec<String>, PluginError> {
        Ok(self.programs.logs(&id, tail)?)
    }

    async fn list(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(self.programs.list()?)
    }
}

impl From<IpcError> for PluginError {
    fn from(error: IpcError) -> Self {
        match error {
            IpcError::Denied(message) => PluginError::PermissionDenied(message),
            IpcError::NotFound(message) => PluginError::NotFound(message),
            IpcError::Invalid(message) => PluginError::Invalid(message),
            IpcError::Failed(message) => PluginError::Failed(message),
        }
    }
}

impl bindings::hestia::plugin::ipc::Host for PluginState {
    async fn open(&mut self, endpoint: String) -> Result<String, PluginError> {
        let ipc = self.ipc.clone();
        Ok(ipc.open(&endpoint).await?)
    }

    async fn send(&mut self, connection: String, bytes: Vec<u8>) -> Result<(), PluginError> {
        let ipc = self.ipc.clone();
        Ok(ipc.send(&connection, &bytes).await?)
    }

    async fn receive(
        &mut self,
        connection: String,
        max: u32,
        timeout_ms: u32,
    ) -> Result<Vec<u8>, PluginError> {
        let ipc = self.ipc.clone();
        let wait = std::time::Duration::from_millis(timeout_ms.into());
        Ok(ipc.receive(&connection, max, wait).await?)
    }

    async fn close(&mut self, connection: String) -> Result<(), PluginError> {
        Ok(self.ipc.close(&connection)?)
    }

    async fn list(&mut self) -> Result<Vec<String>, PluginError> {
        Ok(self.ipc.list()?)
    }
}
