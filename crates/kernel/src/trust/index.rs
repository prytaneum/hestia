use std::collections::HashSet;

use semver::Version;
use serde::Deserialize;

use super::error::TrustError;
use super::signature::Signer;

/// A repository's catalogue, signed as a whole. The trusted comment of its
/// signature repeats the subject, `hestia-index <repository> <sequence>
/// <expires>`, so a signature cannot be lifted onto another repository, an
/// older sequence or a longer life.
#[derive(Deserialize, Debug, Clone)]
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
    pub plugins: Vec<Release>,
    #[serde(default)]
    pub revoked: Vec<Revocation>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub id: String,
    pub version: String,
    pub manifest: Artifact,
    pub component: Artifact,
}

/// A file of a package and the SHA-256 the signed index vouches for.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
}

#[derive(Deserialize, Debug, Clone)]
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

impl Index {
    /// The index `body` signed by `signer` as `signature`, for `repository`, no
    /// older than `floor` and not expired at `now`.
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
        let mut listed = HashSet::new();
        for release in &index.plugins {
            let fine = crate::id::is_valid(&release.id)
                && semver(&release.version).is_some()
                && listed.insert((&release.id, &release.version));
            if !fine {
                return Err(TrustError::Malformed(format!(
                    "'{}' {} is not a valid release",
                    release.id, release.version
                )));
            }
        }
        Ok(index)
    }

    pub fn revocation(&self, id: &str, version: &str) -> Option<&Revocation> {
        self.revoked
            .iter()
            .find(|revoked| revoked.id == id && revoked.version == version)
    }

    pub fn release(&self, id: &str, version: &str) -> Option<&Release> {
        self.plugins
            .iter()
            .find(|release| release.id == id && release.version == version)
    }

    /// The newest release of `id` that is not revoked.
    pub fn latest(&self, id: &str) -> Option<&Release> {
        self.plugins
            .iter()
            .filter(|release| release.id == id && self.revocation(id, &release.version).is_none())
            .max_by_key(|release| semver(&release.version))
    }
}

/// Whether `candidate` is a higher version than `installed`. An installed
/// version that is not semver, as a dev install may have, is always older.
pub fn is_newer(candidate: &str, installed: &str) -> bool {
    match (semver(candidate), semver(installed)) {
        (Some(candidate), Some(installed)) => candidate > installed,
        (Some(_), None) => true,
        _ => false,
    }
}
