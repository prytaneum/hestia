//! The plugin kernel: the registry every plugin, built-in or not, is listed in,
//! the sandbox WASM plugins run in, and the network policy every request passes.

mod builtin;
mod error;
mod grant;
pub mod id;
mod install;
mod manifest;
mod registry;
mod runtime;
pub mod vetting;

pub use builtin::{Builtin, Busy, Descriptor};
pub use error::{PackageError, PluginError};
pub use grant::{denials, GrantError, Permission, Table};
pub use manifest::ManifestError;
pub use registry::Plugins;
pub use runtime::{
    Answer, Denials, GuestError, Limits, Loaded, NoticeLevel, Opened, Sandbox, SandboxError,
    Services, Shape, ShapeError,
};
