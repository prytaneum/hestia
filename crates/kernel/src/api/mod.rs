//! What a built-in adds to expose a WIT package: a `<package>/` directory with
//! its table, its adapter (the generated `Host` traits over [`HostCalls`], with
//! conversions in `convert`) and the calls into the extension points it
//! defines. Nothing else in the kernel changes. The kernel's own package keeps
//! its table in `grant/kernel.rs` and its adapters in `runtime`.
//!
//! The adapter plumbing (`Link`, the error mapping, extension calls) compiles
//! with the packages that use it. Until the first real built-in package lands,
//! that is only the test built-in, behind the `probe` feature.

mod calls;
mod chain;
#[cfg(feature = "probe")]
mod error;
#[cfg(feature = "probe")]
mod link;
#[cfg(feature = "probe")]
pub mod probe;

use wasmtime::component::{Component, InstancePre, Linker};

pub use calls::{HostCalls, Reply, Settled};
pub use chain::{Chain, ChainError};
#[cfg(feature = "probe")]
pub(crate) use link::Link;

use crate::runtime::{SandboxError, State};

/// Links the adapter of every built-in package this kernel carries.
pub(crate) fn link(linker: &mut Linker<State>) -> Result<(), SandboxError> {
    #[cfg(feature = "probe")]
    probe::link(linker)?;
    #[cfg(not(feature = "probe"))]
    let _ = linker;
    Ok(())
}

/// Type-checks each extension point of a built-in package that `component`
/// exports, so a malformed one fails the load instead of trapping per call.
pub(crate) fn check_exports(
    component: &Component,
    pre: &InstancePre<State>,
) -> Result<(), SandboxError> {
    #[cfg(feature = "probe")]
    probe::check_exports(component, pre)?;
    #[cfg(not(feature = "probe"))]
    let _ = (component, pre);
    Ok(())
}
