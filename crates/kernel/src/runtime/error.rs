pub use super::bindings::hestia::kernel::types::PluginError as GuestError;

/// Why the sandbox could not load or run a plugin.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("cannot start the plugin engine: {0:#}")]
    Engine(wasmtime::Error),
    #[error("cannot load component: {0:#}")]
    Load(wasmtime::Error),
    #[error("plugin '{0}' found no free call slot within its budget")]
    Saturated(String),
    #[error("call exceeded its time budget")]
    Timeout,
    #[error("call exceeded its memory limit")]
    Memory,
    #[error("call trapped: {0}")]
    Trapped(String),
    /// The call failed and was the plugin's last allowed trap in the window;
    /// the owner of the plugin's state disables it.
    #[error("{0}; the plugin reached its trap limit")]
    TrapLimit(Box<SandboxError>),
    #[error("cannot store the setting: {0:?}")]
    Setting(GuestError),
    #[error("plugin returned an error: {0:?}")]
    Guest(GuestError),
}
