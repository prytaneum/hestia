//! The plugin kernel: the registry every plugin, built-in or not, is listed in,
//! the sandbox WASM plugins run in, and the network policy every request passes.

mod builtin;
mod error;
mod registry;
mod runtime;
pub mod vetting;

pub use builtin::{Builtin, Busy, Descriptor};
pub use error::PluginError;
pub use registry::Plugins;
pub use runtime::{GuestError, Limits, Loaded, Sandbox, SandboxError};
