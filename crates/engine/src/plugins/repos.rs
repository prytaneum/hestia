//! Trust in a plugin repository follows its minisign key, never its URL, host
//! or name ([0097](../../../../docs/decisions/0097-trust-follows-the-repository-key.md)).
//! These checks are pure: the caller fetches the bytes and holds the pinned key.

use proto::error::ErrorInfo;
use proto::repository::{build_comment, index_comment, RepositoryIndex, Revocation};

use crate::signature::{key_id, verify_trusted};

/// Why repository material was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("`{0}` is not a minisign public key")]
    BadKey(String),
    #[error("signed by a key other than the trusted one")]
    WrongKey,
    #[error("key {0} is the official plugins key, which only the official store may pin")]
    OfficialKey(String),
    #[error("the signature does not verify")]
    BadSignature,
    #[error("the index now names key {declared}, but key {pinned} was trusted")]
    KeyChanged { pinned: String, declared: String },
    #[error("the signature was made for `{found}`, not `{expected}`")]
    WrongSubject { expected: String, found: String },
    #[error("index version {offered} is older than the cached {cached}")]
    Rollback { offered: u64, cached: u64 },
    #[error("the index is not valid: {0}")]
    Malformed(String),
}

impl From<RepoError> for ErrorInfo {
    fn from(error: RepoError) -> ErrorInfo {
        match error {
            RepoError::KeyChanged { pinned, declared } => {
                ErrorInfo::RepositoryKeyChanged { pinned, declared }
            }
            RepoError::BadKey(_) | RepoError::Malformed(_) => ErrorInfo::RepositoryInvalid {
                detail: error.to_string(),
            },
            RepoError::WrongKey
            | RepoError::OfficialKey(_)
            | RepoError::BadSignature
            | RepoError::WrongSubject { .. }
            | RepoError::Rollback { .. } => ErrorInfo::RepositoryRefused {
                detail: error.to_string(),
            },
        }
    }
}

/// Checks an index against the key pinned when the repository was added, and
/// returns it. Nothing is parsed before the signature verifies.
pub fn verify_index(
    bytes: &[u8],
    signature: &str,
    pinned_key: &str,
    cached_version: Option<u64>,
) -> Result<RepositoryIndex, RepoError> {
    let comment = verify(bytes, signature, pinned_key)?;
    let index: RepositoryIndex =
        serde_json::from_slice(bytes).map_err(|error| RepoError::Malformed(error.to_string()))?;
    if index.key.trim() != pinned_key.trim() {
        return Err(RepoError::KeyChanged {
            pinned: key_id_of(pinned_key)?,
            declared: key_id(&index.key).unwrap_or_else(|_| index.key.clone()),
        });
    }
    expect_comment(&comment, index_comment(index.version))?;
    if let Some(cached) = cached_version.filter(|cached| index.version < *cached) {
        return Err(RepoError::Rollback {
            offered: index.version,
            cached,
        });
    }
    Ok(index)
}

/// Checks a package against the key of the repository that lists it, and
/// that the signature was made for exactly this id, version and file.
pub fn verify_build(
    bytes: &[u8],
    signature: &str,
    key: &str,
    id: &str,
    version: &str,
    file: &str,
) -> Result<(), RepoError> {
    let comment = verify(bytes, signature, key)?;
    expect_comment(&comment, build_comment(id, version, file))
}

/// The revocation an index declares for a plugin, honoured only when the
/// plugin was signed by the index's own key: a repository cannot revoke what
/// it did not sign. `signer` is the recorded key id.
pub fn revocation<'a>(
    index: &'a RepositoryIndex,
    signer: &str,
    id: &str,
    version: &str,
) -> Option<&'a Revocation> {
    if key_id(&index.key).ok()? != signer {
        return None;
    }
    index
        .revoked
        .iter()
        .find(|entry| entry.id == id && entry.versions.iter().any(|v| v == version))
}

/// The key id to show wherever the user is asked to trust a key.
pub fn key_id_of(key: &str) -> Result<String, RepoError> {
    key_id(key).map_err(|_| RepoError::BadKey(key.to_string()))
}

fn verify(bytes: &[u8], signature: &str, key: &str) -> Result<String, RepoError> {
    key_id_of(key)?;
    verify_trusted(bytes, signature, key).map_err(|error| match error {
        minisign_verify::Error::UnexpectedKeyId => RepoError::WrongKey,
        _ => RepoError::BadSignature,
    })
}

fn expect_comment(found: &str, expected: String) -> Result<(), RepoError> {
    if found == expected {
        Ok(())
    } else {
        Err(RepoError::WrongSubject {
            expected,
            found: found.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::testkit::Signer;

    fn index_bytes(key: &str, version: u64) -> Vec<u8> {
        serde_json::to_vec(&RepositoryIndex {
            version,
            key: key.to_string(),
            revoked: vec![Revocation {
                id: "aa".to_string(),
                versions: vec!["0.1.0".to_string()],
                reason: "bad".to_string(),
            }],
            ..RepositoryIndex::default()
        })
        .unwrap()
    }

    #[test]
    fn a_good_index_passes() {
        let signer = Signer::new();
        let bytes = index_bytes(&signer.key(), 3);
        let signature = signer.sign(&bytes, &index_comment(3));

        let index = verify_index(&bytes, &signature, &signer.key(), Some(3)).unwrap();

        assert_eq!(index.version, 3);
    }

    #[test]
    fn an_index_signed_by_another_key_is_refused() {
        let (trusted, other) = (Signer::new(), Signer::new());
        let bytes = index_bytes(&trusted.key(), 1);
        let signature = other.sign(&bytes, &index_comment(1));

        let error = verify_index(&bytes, &signature, &trusted.key(), None).unwrap_err();

        assert_eq!(error, RepoError::WrongKey);
    }

    #[test]
    fn an_index_declaring_another_key_is_refused() {
        let (trusted, other) = (Signer::new(), Signer::new());
        let bytes = index_bytes(&other.key(), 1);
        let signature = trusted.sign(&bytes, &index_comment(1));

        let error = verify_index(&bytes, &signature, &trusted.key(), None).unwrap_err();

        assert!(matches!(error, RepoError::KeyChanged { .. }), "{error:?}");
    }

    #[test]
    fn tampered_bytes_are_refused() {
        let signer = Signer::new();
        let bytes = index_bytes(&signer.key(), 1);
        let signature = signer.sign(&bytes, &index_comment(1));

        let tampered = index_bytes(&signer.key(), 2);
        let error = verify_index(&tampered, &signature, &signer.key(), None).unwrap_err();

        assert_eq!(error, RepoError::BadSignature);
    }

    #[test]
    fn an_older_index_is_refused_even_with_a_valid_signature() {
        let signer = Signer::new();
        let bytes = index_bytes(&signer.key(), 2);
        let signature = signer.sign(&bytes, &index_comment(2));

        let error = verify_index(&bytes, &signature, &signer.key(), Some(5)).unwrap_err();

        assert_eq!(
            error,
            RepoError::Rollback {
                offered: 2,
                cached: 5
            }
        );
    }

    #[test]
    fn an_index_signature_carrying_another_version_is_refused() {
        let signer = Signer::new();
        let bytes = index_bytes(&signer.key(), 2);
        let signature = signer.sign(&bytes, &index_comment(9));

        let error = verify_index(&bytes, &signature, &signer.key(), None).unwrap_err();

        assert!(matches!(error, RepoError::WrongSubject { .. }), "{error:?}");
    }

    #[test]
    fn a_build_is_bound_to_its_id_version_and_file() {
        let (signer, other) = (Signer::new(), Signer::new());
        let bytes = b"package";
        let signature = signer.sign(bytes, &build_comment("aa", "0.1.0", "aa-0.1.0.hplugin"));
        let verify = |key: &str, id: &str, version: &str, file: &str| {
            verify_build(bytes, &signature, key, id, version, file)
        };

        verify(&signer.key(), "aa", "0.1.0", "aa-0.1.0.hplugin").unwrap();
        for (id, version, file) in [
            ("bb", "0.1.0", "aa-0.1.0.hplugin"),
            ("aa", "0.2.0", "aa-0.1.0.hplugin"),
            ("aa", "0.1.0", "aa-0.2.0.hplugin"),
        ] {
            let error = verify(&signer.key(), id, version, file).unwrap_err();
            assert!(matches!(error, RepoError::WrongSubject { .. }), "{error:?}");
        }
        assert_eq!(
            verify(&other.key(), "aa", "0.1.0", "aa-0.1.0.hplugin").unwrap_err(),
            RepoError::WrongKey
        );
    }

    #[test]
    fn a_malformed_key_is_refused() {
        let error = verify_build(b"", "", "not a key", "aa", "0.1.0", "f").unwrap_err();

        assert!(matches!(error, RepoError::BadKey(_)), "{error:?}");
    }

    #[test]
    fn a_revocation_counts_only_for_plugins_signed_by_the_index_key() {
        let (signer, other) = (Signer::new(), Signer::new());
        let index: RepositoryIndex =
            serde_json::from_slice(&index_bytes(&signer.key(), 1)).unwrap();
        let own = key_id_of(&signer.key()).unwrap();

        assert!(revocation(&index, &own, "aa", "0.1.0").is_some());
        assert!(revocation(&index, &own, "aa", "0.2.0").is_none());
        assert!(revocation(&index, &key_id_of(&other.key()).unwrap(), "aa", "0.1.0").is_none());
    }
}
