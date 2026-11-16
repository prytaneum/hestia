//! The plugin registry and built-ins, beside what the plugin system builds on:
//! download vetting, program pinning, and the signed repository index.

mod builtin;
mod error;
mod registry;

// Kept for the plugin system's later tasks; nothing in the engine calls them yet.
#[allow(dead_code)]
pub(crate) mod api;
#[allow(dead_code)]
mod catalogue;
#[allow(dead_code)]
mod index;
#[allow(dead_code)]
mod invocation;
#[allow(dead_code)]
mod official;
#[allow(dead_code)]
mod program;
#[allow(dead_code)]
mod repos;
#[allow(dead_code)]
mod repositories;
#[cfg(test)]
pub(crate) mod testkit;
#[allow(dead_code)]
pub(crate) mod vouch;

pub use builtin::{all, Builtin, Busy, Descriptor};
pub use catalogue::Listings;
pub use error::PluginError;
pub use official::{is_official_signer, refuse_official_key};
pub use program::{
    approval_digest, pin_on_path, resolve_on_path, target_triple, ManagedJava, Resolver,
};
pub use registry::Plugins;
pub use repos::{key_id_of, revocation, verify_build, verify_index, RepoError};
