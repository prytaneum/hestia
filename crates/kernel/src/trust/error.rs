use crate::vetting::HostError;

/// Why a repository, its index or one of its packages was not trusted.
#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error("the repository key is not a minisign public key")]
    BadKey,
    #[error("that key is Hestia's own; only Hestia's repository may use it")]
    OfficialKey,
    #[error("the index signature does not verify under the pinned key")]
    BadSignature,
    #[error("the index is not signed for this repository and sequence")]
    Subject,
    #[error("the index is malformed: {0}")]
    Malformed(String),
    #[error("the index expired")]
    Expired,
    #[error("the index is older than one already seen (sequence {got}, seen {seen})")]
    Replay { got: u64, seen: u64 },
    #[error("{0} does not match the digest the signed index lists")]
    Digest(String),
    #[error("the index lifetime is longer than {0} days")]
    Lifetime(u64),
    #[error("the key of repository '{0}' is not the one that vouched for the installed version")]
    RepositoryKey(String),
    #[error("the author key of '{0}' is not the one that signed the installed version")]
    AuthorKey(String),
    #[error("the package signature does not verify under the author's key the index lists")]
    PackageSignature,
    #[error("the package archive is refused: {0}")]
    Archive(String),
    #[error("version {version} of '{plugin}' is revoked{reason}")]
    Revoked {
        plugin: String,
        version: String,
        reason: String,
    },
    #[error("the index names hosts it cannot reach: {0}")]
    Hosts(#[from] HostError),
    #[error("{0}")]
    Fetch(String),
}
