//! The plugin kernel: the registry every plugin, built-in or not, is listed in,
//! the sandbox WASM plugins run in, and the network policy every request passes.

mod builtin;
mod error;
pub mod id;
mod registry;
mod runtime;
pub mod vetting;

pub use builtin::{Builtin, Busy, Descriptor};
pub use error::PluginError;
pub use registry::Plugins;
pub use runtime::{
    Answer, GuestError, Limits, Loaded, NoticeLevel, Sandbox, SandboxError, Services,
};
