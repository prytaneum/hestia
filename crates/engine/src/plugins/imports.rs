use std::sync::Arc;

use async_trait::async_trait;
use proto::plugin::{Grant, Permission};
use serde_json::Value;
use tracing::Level;
use wasmtime::component::ResourceTable;
use wasmtime::StoreLimits;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use super::chain::CallChain;
use super::output::PluginLog;

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
}

/// Per-call store data; dropped with the call.
pub(crate) struct PluginState {
    pub(crate) wasi: WasiCtx,
    pub(crate) table: ResourceTable,
    pub(crate) limits: StoreLimits,
    pub(crate) log: Arc<PluginLog>,
    pub(crate) gate: Arc<Gate>,
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
