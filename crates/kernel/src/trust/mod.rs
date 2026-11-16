//! Who a package is trusted to come from: repositories whose key the user
//! pinned, the signed index each publishes, and the digests it vouches for.

mod error;
mod fetch;
mod index;
mod repositories;
mod signature;

pub use error::TrustError;
pub(crate) use fetch::Fetched;
pub(crate) use index::{is_newer, Release};
pub(crate) use repositories::{pinned, Repositories, Repository};
pub use signature::Signer;

/// Keys that stand for Hestia. Its own repository is added by the release that
/// ships it; until then no key is official, and the rule below has nothing to
/// refuse.
pub const OFFICIAL_KEYS: &[&str] = &[];
