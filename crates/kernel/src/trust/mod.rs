//! Who a package is trusted to come from: repositories whose key the user
//! pinned, the signed index each publishes, and the authors whose keys it lists.
//! A package is a zip the author signed; the index only says which key that is
//! and what the file's digest must be.

mod error;
mod fetch;
mod hplugin;
mod index;
mod repositories;
mod signature;

pub use error::TrustError;
#[cfg(feature = "loopback")]
pub(crate) use fetch::loopback;
pub(crate) use fetch::{policy, Fetched, PolicyFor};
pub(crate) use hplugin::Hplugin;
pub(crate) use index::{accepts, is_newer, Listing, Release};
pub(crate) use repositories::{pinned, Repositories, Repository};
pub(crate) use signature::same_key;
pub use signature::Signer;

/// Keys that stand for Hestia. Its own repository is added by the release that
/// ships it; until then no key is official, and the rules below have nothing to
/// refuse. An author key is official only by being listed here: an index never
/// makes a key official, and no repository may pin one of these.
pub const OFFICIAL_KEYS: &[&str] = &[];
