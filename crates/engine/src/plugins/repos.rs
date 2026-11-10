//! Trust in a plugin repository follows its minisign key, never its URL, host
//! or name ([0097](../../../../docs/decisions/0097-trust-follows-the-repository-key.md),
//! [0111](../../../../docs/decisions/0111-a-repository-publishes-to-its-own-branch.md)).
//! These checks are pure: the caller fetches the bytes and holds the pinned keys.

use proto::error::ErrorInfo;
use proto::repository::{build_comment, index_comment, RepositoryIndex, Revocation};

use crate::signature::{key_id, verify_trusted};

/// Why repository material was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RepoError {
    #[error("`{0}` is not a minisign public key")]
    BadKey(String),
    #[error("there is no pinned key to verify with")]
    NoKey,
    #[error("signed by a key other than the trusted one")]
    WrongKey,
    #[error("key {0} is the official plugins key, which only the official repository may pin")]
    OfficialKey(String),
    #[error("the signature does not verify")]
    BadSignature,
    #[error("the index now names key {declared}, but key {pinned} was trusted")]
    KeyChanged { pinned: String, declared: String },
    #[error("the signature was made for `{found}`, not `{expected}`")]
    WrongSubject { expected: String, found: String },
    #[error("index version {offered} is older than the cached {cached}")]
    Rollback { offered: u64, cached: u64 },
    #[error("{repository} is the official repository and cannot be removed")]
    Official { repository: String },
    #[error("the index of {repository} has expired")]
    Expired { repository: String },
    #[error("the index is not valid: {0}")]
    Malformed(String),
}

impl From<RepoError> for ErrorInfo {
    fn from(error: RepoError) -> ErrorInfo {
        match error {
            RepoError::KeyChanged { pinned, declared } => {
                ErrorInfo::RepositoryKeyChanged { pinned, declared }
            }
            RepoError::Expired { repository } => ErrorInfo::RepositoryExpired { repository },
            RepoError::Official { repository } => ErrorInfo::RepositoryOfficial { repository },
            RepoError::BadKey(_) | RepoError::Malformed(_) => ErrorInfo::RepositoryInvalid {
                detail: error.to_string(),
            },
            RepoError::NoKey
            | RepoError::WrongKey
            | RepoError::OfficialKey(_)
            | RepoError::BadSignature
            | RepoError::WrongSubject { .. }
            | RepoError::Rollback { .. } => ErrorInfo::RepositoryRefused {
                detail: error.to_string(),
            },
        }
    }
}

/// Checks an index against the keys its repository trusts, and returns it.
/// The order is the point: the signature first and nothing parsed before it
/// verifies, then the declared key, the version against the cached one and the
/// expiry. The caller caches the index only after this returns.
pub fn verify_index(
    bytes: &[u8],
    signature: &str,
    keys: &[String],
    repository: &str,
    cached_version: Option<u64>,
    now_unix: i64,
) -> Result<RepositoryIndex, RepoError> {
    let (key, comment) = verify_any(bytes, signature, keys)?;
    let index: RepositoryIndex =
        serde_json::from_slice(bytes).map_err(|error| RepoError::Malformed(error.to_string()))?;
    if index.key.trim() != key.trim() {
        return Err(RepoError::KeyChanged {
            pinned: key_id_of(key)?,
            declared: key_id(&index.key).unwrap_or_else(|_| index.key.clone()),
        });
    }
    expect_comment(&comment, index_comment(index.version, index.expires))?;
    if let Some(cached) = cached_version.filter(|cached| index.version < *cached) {
        return Err(RepoError::Rollback {
            offered: index.version,
            cached,
        });
    }
    ensure_fresh(&index, repository, now_unix)?;
    Ok(index)
}

/// An index past its expiry is stale: nothing new is installed from it.
pub fn ensure_fresh(
    index: &RepositoryIndex,
    repository: &str,
    now_unix: i64,
) -> Result<(), RepoError> {
    if index.expires <= now_unix {
        return Err(RepoError::Expired {
            repository: repository.to_string(),
        });
    }
    Ok(())
}

/// Checks a package against the keys that may have signed it, and that the
/// signature was made for exactly this id, version and file. Returns the id of
/// the key that verified, which is what the install records as its signer.
pub fn verify_build(
    bytes: &[u8],
    signature: &str,
    keys: &[String],
    id: &str,
    version: &str,
    file: &str,
) -> Result<String, RepoError> {
    let (key, comment) = verify_any(bytes, signature, keys)?;
    expect_comment(&comment, build_comment(id, version, file))?;
    key_id_of(key)
}

/// The revocation an index declares for a plugin, honoured only when the
/// plugin was signed by one of the index's own keys (`key` or its spare
/// `next`): a repository cannot revoke what it did not sign. `signer` is the
/// recorded key id.
pub fn revocation<'a>(
    index: &'a RepositoryIndex,
    signer: &str,
    id: &str,
    version: &str,
) -> Option<&'a Revocation> {
    let own_key = [&index.key, &index.next]
        .into_iter()
        .filter(|key| !key.is_empty())
        .any(|key| key_id(key).is_ok_and(|own| own == signer));
    if !own_key {
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

/// The first of `keys` that verifies the signature, with the trusted comment.
/// When none does, the first failure is the one reported.
fn verify_any<'a>(
    bytes: &[u8],
    signature: &str,
    keys: &'a [String],
) -> Result<(&'a String, String), RepoError> {
    let mut first_failure = None;
    for key in keys {
        match verify(bytes, signature, key) {
            Ok(comment) => return Ok((key, comment)),
            Err(error) => {
                first_failure.get_or_insert(error);
            }
        }
    }
    Err(first_failure.unwrap_or(RepoError::NoKey))
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
    use crate::plugins::testkit::{sample_index, sign_index, Signer, SAMPLE_NOW, SAMPLE_VERSION};

    const URL: &str = "https://repository.example/index.json";

    fn keys(signer: &Signer) -> Vec<String> {
        vec![signer.key()]
    }

    fn verify_sample(
        bytes: &[u8],
        signature: &str,
        keys: &[String],
        cached: Option<u64>,
        now: i64,
    ) -> Result<RepositoryIndex, RepoError> {
        verify_index(bytes, signature, keys, URL, cached, now)
    }

    #[test]
    fn a_good_index_passes() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        let signature = sign_index(&signer, &bytes);

        let index = verify_sample(
            &bytes,
            &signature,
            &keys(&signer),
            Some(SAMPLE_VERSION),
            SAMPLE_NOW,
        )
        .unwrap();

        assert_eq!(index.version, SAMPLE_VERSION);
    }

    #[test]
    fn an_index_signed_by_another_key_is_refused() {
        let (trusted, other) = (Signer::new(), Signer::new());
        let bytes = sample_index(&trusted.key(), |_| {});
        let signature = sign_index(&other, &bytes);

        let error =
            verify_sample(&bytes, &signature, &keys(&trusted), None, SAMPLE_NOW).unwrap_err();

        assert_eq!(error, RepoError::WrongKey);
    }

    #[test]
    fn an_index_declaring_another_key_is_refused() {
        let (trusted, other) = (Signer::new(), Signer::new());
        let bytes = sample_index(&other.key(), |_| {});
        let signature = sign_index(&trusted, &bytes);

        let error =
            verify_sample(&bytes, &signature, &keys(&trusted), None, SAMPLE_NOW).unwrap_err();

        assert!(matches!(error, RepoError::KeyChanged { .. }), "{error:?}");
    }

    #[test]
    fn tampered_bytes_are_refused() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        let signature = sign_index(&signer, &bytes);

        let tampered = sample_index(&signer.key(), |index| index.name = "other".to_string());
        let error =
            verify_sample(&tampered, &signature, &keys(&signer), None, SAMPLE_NOW).unwrap_err();

        assert_eq!(error, RepoError::BadSignature);
    }

    #[test]
    fn an_older_index_is_refused_even_with_a_valid_signature() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        let signature = sign_index(&signer, &bytes);

        let error = verify_sample(
            &bytes,
            &signature,
            &keys(&signer),
            Some(SAMPLE_VERSION + 5),
            SAMPLE_NOW,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RepoError::Rollback {
                offered: SAMPLE_VERSION,
                cached: SAMPLE_VERSION + 5
            }
        );
    }

    #[test]
    fn an_expired_index_is_refused_even_with_a_valid_signature() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        let signature = sign_index(&signer, &bytes);
        let expires = SAMPLE_VERSION as i64 + proto::repository::INDEX_LIFETIME_SECONDS;

        verify_sample(&bytes, &signature, &keys(&signer), None, expires - 1).unwrap();
        let error = verify_sample(&bytes, &signature, &keys(&signer), None, expires).unwrap_err();

        assert_eq!(
            error,
            RepoError::Expired {
                repository: URL.to_string()
            }
        );
    }

    #[test]
    fn a_signature_that_carries_another_version_or_expiry_is_refused() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        for comment in [
            index_comment(SAMPLE_VERSION + 1, 0),
            format!("hestia-index version={SAMPLE_VERSION}"),
        ] {
            let signature = signer.sign(&bytes, &comment);

            let error =
                verify_sample(&bytes, &signature, &keys(&signer), None, SAMPLE_NOW).unwrap_err();

            assert!(matches!(error, RepoError::WrongSubject { .. }), "{error:?}");
        }
    }

    #[test]
    fn an_index_signed_by_the_spare_key_verifies() {
        let (current, spare) = (Signer::new(), Signer::new());
        let both = vec![current.key(), spare.key()];
        let bytes = sample_index(&spare.key(), |_| {});
        let signature = sign_index(&spare, &bytes);

        let index = verify_sample(&bytes, &signature, &both, None, SAMPLE_NOW).unwrap();

        assert_eq!(index.key, spare.key());
        assert!(verify_sample(&bytes, &signature, &keys(&current), None, SAMPLE_NOW).is_err());
    }

    #[test]
    fn with_no_key_nothing_verifies() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        let signature = sign_index(&signer, &bytes);

        let error = verify_sample(&bytes, &signature, &[], None, SAMPLE_NOW).unwrap_err();

        assert_eq!(error, RepoError::NoKey);
    }

    #[test]
    fn a_build_is_bound_to_its_id_version_and_file() {
        let (signer, other) = (Signer::new(), Signer::new());
        let bytes = b"package";
        let signature = signer.sign(bytes, &build_comment("aa", "0.1.0", "aa-0.1.0.hplugin"));
        let verify = |key: &Signer, id: &str, version: &str, file: &str| {
            verify_build(bytes, &signature, &keys(key), id, version, file)
        };

        let signer_id = verify(&signer, "aa", "0.1.0", "aa-0.1.0.hplugin").unwrap();
        assert_eq!(signer_id, key_id_of(&signer.key()).unwrap());
        for (id, version, file) in [
            ("bb", "0.1.0", "aa-0.1.0.hplugin"),
            ("aa", "0.2.0", "aa-0.1.0.hplugin"),
            ("aa", "0.1.0", "aa-0.2.0.hplugin"),
        ] {
            let error = verify(&signer, id, version, file).unwrap_err();
            assert!(matches!(error, RepoError::WrongSubject { .. }), "{error:?}");
        }
        assert_eq!(
            verify(&other, "aa", "0.1.0", "aa-0.1.0.hplugin").unwrap_err(),
            RepoError::WrongKey
        );
    }

    #[test]
    fn a_malformed_key_is_refused() {
        let error =
            verify_build(b"", "", &["not a key".to_string()], "aa", "0.1.0", "f").unwrap_err();

        assert!(matches!(error, RepoError::BadKey(_)), "{error:?}");
    }

    #[test]
    fn a_revocation_counts_only_for_plugins_signed_by_the_index_keys() {
        let (signer, spare, other) = (Signer::new(), Signer::new(), Signer::new());
        let index = RepositoryIndex {
            key: signer.key(),
            next: spare.key(),
            revoked: vec![Revocation {
                id: "aa".to_string(),
                versions: vec!["0.1.0".to_string()],
                reason: "bad".to_string(),
            }],
            ..RepositoryIndex::default()
        };
        let id = |signer: &Signer| key_id_of(&signer.key()).unwrap();

        assert!(revocation(&index, &id(&signer), "aa", "0.1.0").is_some());
        assert!(revocation(&index, &id(&spare), "aa", "0.1.0").is_some());
        assert!(revocation(&index, &id(&signer), "aa", "0.2.0").is_none());
        assert!(revocation(&index, &id(&other), "aa", "0.1.0").is_none());
    }
}
