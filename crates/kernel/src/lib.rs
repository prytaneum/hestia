//! The plugin kernel: the registry every plugin, built-in or not, is listed in,
//! the sandbox WASM plugins run in, and the network policy every request passes.

mod api;
mod builtin;
mod commands;
pub mod entries;
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
pub use api::{Chain, ChainError, HostCalls, Reply, Settled};
pub use builtin::{Builtin, Busy, Descriptor};
pub use error::{PackageError, PluginError};
pub use grant::versions::check as check_versions;
pub use grant::{denials, derive, widens, Gate, GrantError, Permission, Stability, Table};
pub use install::sandbox_root;
pub use manifest::{Manifest, ManifestError};
pub use registry::Plugins;
pub use runtime::{
    Answer, Denials, GuestError, Invocation, JobOutcome, Limits, Loaded, NoticeLevel, Opened,
    Sandbox, SandboxError, Services, Shape, ShapeError,
};
pub use trust::{
    statement, Hplugin, Index, Listing, Release, Revocation, Signer, TrustError, LIFETIME_DAYS,
};

/// The permissions and interfaces of the kernel's own package, which the
/// registry extends with those of the built-ins that provide a package.
pub fn table() -> Table {
    api::kernel::table()
}
