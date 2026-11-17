//! The plugin kernel: the registry every plugin, built-in or not, is listed in,
//! the sandbox WASM plugins run in, and the network policy every request passes.

mod api;
mod builtin;
mod commands;
mod error;
mod grant;
pub mod id;
mod install;
mod manifest;
mod registry;
mod runtime;
mod trust;
pub mod vetting;

#[cfg(feature = "probe")]
pub use api::probe;
pub use api::{Chain, ChainError, HostCalls, Reply};
pub use builtin::{Builtin, Busy, Descriptor};
pub use error::{PackageError, PluginError};
pub use grant::{denials, GrantError, Permission, Stability, Table};
pub use install::sandbox_root;
pub use manifest::ManifestError;
pub use registry::Plugins;
pub use runtime::{
    Answer, Denials, GuestError, Invocation, JobOutcome, Limits, Loaded, NoticeLevel, Opened,
    Sandbox, SandboxError, Services, Shape, ShapeError,
};
pub use trust::TrustError;
