//! What makes a plugin official: it was signed by the official-plugins key,
//! and nothing else — not its id, its URL or the name of the store that lists
//! it ([0099](../../../../docs/decisions/0099-official-plugins-are-signed.md)).
//! The key set is `common::app::official_plugin_pubkeys`, empty until the
//! real key exists, so nothing is official by default.

use super::repos::{key_id_of, RepoError};

/// The ids of the official keys; a malformed entry is skipped, never trusted.
fn key_ids(keys: &[&str]) -> Vec<String> {
    keys.iter().filter_map(|key| key_id_of(key).ok()).collect()
}

/// Whether a recorded signer (a key id) is one of `keys`.
pub fn is_official_signer_in(signer: Option<&str>, keys: &[&str]) -> bool {
    signer.is_some_and(|signer| key_ids(keys).iter().any(|id| id == signer))
}

/// Whether a recorded signer is the official-plugins key of this build.
pub fn is_official_signer(signer: Option<&str>) -> bool {
    is_official_signer_in(signer, &common::app::official_plugin_pubkeys())
}

/// Refuses a repository whose index names one of `keys`: only the official
/// store may pin the official key, and it is added by Hestia, not by `repo.add`.
pub fn refuse_official_key_in(declared: &str, keys: &[&str]) -> Result<(), RepoError> {
    let id = key_id_of(declared)?;
    if key_ids(keys).contains(&id) {
        return Err(RepoError::OfficialKey(id));
    }
    Ok(())
}

/// [`refuse_official_key_in`] against this build's official key.
pub fn refuse_official_key(declared: &str) -> Result<(), RepoError> {
    refuse_official_key_in(declared, &common::app::official_plugin_pubkeys())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::repos::{verify_build, verify_index};
    use crate::plugins::testkit::{sample_index, Signer};
    use crate::signature::verify_bytes;
    use base64::Engine as _;
    use proto::repository::{build_comment, index_comment};

    #[test]
    fn a_plugin_signed_by_the_official_key_is_official() {
        let (official, other) = (Signer::new(), Signer::new());
        let keys = [official.key()];
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();

        let signer = key_id_of(&official.key()).unwrap();
        assert!(is_official_signer_in(Some(&signer), &keys));
        assert!(!is_official_signer_in(
            Some(&key_id_of(&other.key()).unwrap()),
            &keys
        ));
        assert!(!is_official_signer_in(None, &keys));
    }

    #[test]
    fn with_no_official_key_nothing_is_official() {
        let signer = Signer::new();

        assert!(!is_official_signer_in(
            Some(&key_id_of(&signer.key()).unwrap()),
            &[]
        ));
    }

    #[test]
    fn a_third_party_index_naming_the_official_key_is_refused() {
        let (official, other) = (Signer::new(), Signer::new());
        let key = official.key();

        let error = refuse_official_key_in(&key, &[&key]).unwrap_err();

        assert_eq!(error, RepoError::OfficialKey(key_id_of(&key).unwrap()));
        refuse_official_key_in(&other.key(), &[&key]).unwrap();
        refuse_official_key_in(&key, &[]).unwrap();
    }

    #[test]
    fn a_tampered_official_package_or_index_fails_verification() {
        let official = Signer::new();
        let index = sample_index(&official.key(), |_| {});
        let signature = official.sign(&index, &index_comment(3));
        let package_signature = official.sign(b"package", &build_comment("aa", "0.1.0", "f"));

        let mut tampered = index.clone();
        tampered.push(b' ');
        assert_eq!(
            verify_index(&tampered, &signature, &official.key(), None).unwrap_err(),
            RepoError::BadSignature
        );
        assert_eq!(
            verify_build(
                b"packagf",
                &package_signature,
                &official.key(),
                "aa",
                "0.1.0",
                "f"
            )
            .unwrap_err(),
            RepoError::BadSignature
        );
    }

    #[test]
    fn the_update_path_rejects_the_plugins_key_and_the_plugin_path_rejects_the_app_key() {
        let (plugins, app) = (Signer::new(), Signer::new());
        let data = b"artifact";
        let wrapped_app = base64::engine::general_purpose::STANDARD
            .encode(format!("untrusted comment: key\n{}\n", app.key()));
        let wrapped_signature =
            base64::engine::general_purpose::STANDARD.encode(app.sign(data, "trusted"));
        let plugin_key: &'static str = Box::leak(plugins.key().into_boxed_str());

        assert!(verify_bytes(data, &wrapped_signature, std::iter::once(plugin_key)).is_err());
        let app_signature = app.sign(data, "trusted");
        assert!(crate::signature::verify_trusted(data, &app_signature, &wrapped_app).is_err());
        assert!(!common::app::update_pubkeys().any(|key| key == plugin_key));
    }
}
