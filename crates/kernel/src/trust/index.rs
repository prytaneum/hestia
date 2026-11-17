use std::collections::HashSet;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use super::error::TrustError;
use super::hplugin::ARCHIVE_MAX;
use super::signature::Signer;

/// The longest an index may live, counted from when it is fetched. A stolen
/// repository key cannot publish an index that outlasts the key's rotation.
pub const LIFETIME_DAYS: u64 = 30;

/// A repository's catalogue, signed as a whole. The trusted comment of its
/// signature repeats the subject, `hestia-index <repository> <sequence>
/// <expires>`, so a signature cannot be lifted onto another repository, an
/// older sequence or a longer life.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub repository: String,
    /// Rises with every published index; one below what was seen is a replay.
    pub sequence: u64,
    /// Unix seconds; an index past it is refused, so a frozen copy ends.
    pub expires: u64,
    /// Hosts, besides the index's own, that serve the packages it lists.
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub plugins: Vec<Listing>,
    #[serde(default)]
    pub revoked: Vec<Revocation>,
}

/// A plugin of the repository: the key its author signs releases with, and the
/// releases. The key is the author's own; the repository only names it.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    pub id: String,
    pub author: String,
    pub releases: Vec<Release>,
}

/// One `.hplugin` of a plugin and the SHA-256 and size the signed index vouches
/// for.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub reason: String,
}

fn semver(version: &str) -> Option<Version> {
    Version::parse(version).ok()
}

fn well_formed(index: &Index) -> Result<(), String> {
    let mut ids = HashSet::new();
    for listing in &index.plugins {
        if !crate::id::is_valid(&listing.id) || !ids.insert(&listing.id) {
            return Err(format!("'{}' is not a valid plugin", listing.id));
        }
        if Signer::parse(&listing.author).is_err() {
            return Err(format!("the author key of '{}' is not valid", listing.id));
        }
        let mut versions = HashSet::new();
        for release in &listing.releases {
            let hex = release.sha256.len() == 64
                && release.sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
            let fine = semver(&release.version).is_some()
                && versions.insert(&release.version)
                && hex
                && (1..=ARCHIVE_MAX).contains(&release.size);
            if !fine {
                return Err(format!(
                    "'{}' {} is not a valid release",
                    listing.id, release.version
                ));
            }
        }
    }
    Ok(())
}

impl Index {
    /// The index `body` signed by `signer` as `signature`, for `repository`, no
    /// older than `floor`, not expired at `now` and not living past
    /// [`LIFETIME_DAYS`].
    pub fn verify(
        body: &[u8],
        signature: &str,
        signer: &Signer,
        repository: &str,
        floor: u64,
        now: u64,
    ) -> Result<Index, TrustError> {
        let comment = signer.verify(body, signature)?;
        let index: Index = serde_json::from_slice(body)
            .map_err(|error| TrustError::Malformed(error.to_string()))?;
        let subject = format!(
            "hestia-index {} {} {}",
            index.repository, index.sequence, index.expires
        );
        if comment != subject || index.repository != repository {
            return Err(TrustError::Subject);
        }
        if index.sequence < floor {
            return Err(TrustError::Replay {
                got: index.sequence,
                seen: floor,
            });
        }
        if index.expires <= now {
            return Err(TrustError::Expired);
        }
        if index.expires - now > LIFETIME_DAYS * 24 * 3600 {
            return Err(TrustError::Lifetime(LIFETIME_DAYS));
        }
        well_formed(&index).map_err(TrustError::Malformed)?;
        Ok(index)
    }

    pub fn revocation(&self, id: &str, version: &str) -> Option<&Revocation> {
        self.revoked
            .iter()
            .find(|revoked| revoked.id == id && revoked.version == version)
    }

    pub fn listing(&self, id: &str) -> Option<&Listing> {
        self.plugins.iter().find(|listing| listing.id == id)
    }

    /// The release of `id` at `version` unless the index revokes it.
    pub fn release(&self, id: &str, version: &str) -> Option<(&Listing, &Release)> {
        let listing = self.listing(id)?;
        listing
            .releases
            .iter()
            .find(|release| release.version == version)
            .filter(|_| self.revocation(id, version).is_none())
            .map(|release| (listing, release))
    }

    /// The newest release of `id` that is not revoked and `range` accepts.
    pub fn matching(&self, id: &str, range: &VersionReq) -> Option<(&Listing, &Release)> {
        let listing = self.listing(id)?;
        listing
            .releases
            .iter()
            .filter(|release| self.revocation(id, &release.version).is_none())
            .filter(|release| semver(&release.version).is_some_and(|v| range.matches(&v)))
            .max_by_key(|release| semver(&release.version))
            .map(|release| (listing, release))
    }

    /// The newest release of `id` that is not revoked.
    pub fn latest(&self, id: &str) -> Option<(&Listing, &Release)> {
        let listing = self.listing(id)?;
        listing
            .releases
            .iter()
            .filter(|release| self.revocation(id, &release.version).is_none())
            .max_by_key(|release| semver(&release.version))
            .map(|release| (listing, release))
    }
}

#[cfg(feature = "sandbox")]
/// Whether `candidate` is a higher version than `installed`. An installed
/// version that is not semver, as a dev install may have, is always older.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    match (semver(candidate), semver(installed)) {
        (Some(candidate), Some(installed)) => candidate > installed,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(feature = "sandbox")]
/// Whether `version` is one the caret `range` accepts. A version that is not
/// semver, as a dev install may have, is accepted by none.
pub fn accepts(range: &str, version: &str) -> bool {
    match (VersionReq::parse(range), semver(version)) {
        (Ok(range), Some(version)) => range.matches(&version),
        _ => false,
    }
}
