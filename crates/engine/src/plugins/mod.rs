//! What a plugin system builds on, kept apart from any runtime: download
//! vetting, program pinning, and the signed repository index.
#![allow(dead_code)] // held for the plugin system that returns; nothing in the engine calls them until then

pub(crate) mod api;
mod catalogue;
mod index;
mod invocation;
mod official;
mod program;
mod repos;
mod repositories;
#[cfg(test)]
pub(crate) mod testkit;
pub(crate) mod vouch;

pub use catalogue::Listings;
pub use official::{is_official_signer, refuse_official_key};
pub use program::{
    approval_digest, pin_on_path, resolve_on_path, target_triple, ManagedJava, Resolver,
};
pub use repos::{key_id_of, revocation, verify_build, verify_index, RepoError};
