//! The plugin kernel: the registry every plugin, built-in or not, is listed in,
//! the sandbox WASM plugins run in, and the network policy every request passes.

#[cfg(feature = "sandbox")]
mod api;
#[cfg(feature = "sandbox")]
mod builtin;
mod commands;
#[cfg(feature = "sandbox")]
pub mod entries;
#[cfg(feature = "sandbox")]
mod error;
mod grant;
pub mod id;
#[cfg(feature = "sandbox")]
mod install;
mod manifest;
#[cfg(feature = "sandbox")]
mod registry;
#[cfg(feature = "sandbox")]
mod runtime;
mod shape;
mod trust;
pub mod vetting;

#[cfg(feature = "probe")]
pub use api::probe;
#[cfg(feature = "sandbox")]
pub use api::{Chain, ChainError, HostCalls, Reply, Settled};
#[cfg(feature = "sandbox")]
pub use builtin::{Builtin, Busy, Descriptor};
#[cfg(feature = "sandbox")]
pub use error::{PackageError, PluginError};
pub use grant::versions::check as check_versions;
pub use grant::{
    denials, derive, derive_everywhere, widens, Denials, Gate, GrantError, Permission, Stability,
    Table,
};
#[cfg(feature = "sandbox")]
pub use install::sandbox_root;
pub use manifest::{Manifest, ManifestError, Os, ProgramEntry};
#[cfg(feature = "sandbox")]
pub use registry::Plugins;
#[cfg(feature = "sandbox")]
pub use runtime::{
    Answer, GuestError, Invocation, JobOutcome, Limits, Loaded, NoticeLevel, Opened, Sandbox,
    SandboxError, Services,
};
pub use shape::{Shape, ShapeError};
pub use trust::{
    statement, Hplugin, Index, Listing, Release, Revocation, Signer, TrustError, LIFETIME_DAYS,
};

/// The permissions and interfaces of the kernel's own package, which the
/// registry extends with those of the built-ins that provide a package.
pub fn table() -> Table {
    grant::kernel_table()
}
