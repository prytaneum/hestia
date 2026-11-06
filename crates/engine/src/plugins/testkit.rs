//! Shared by the repository tests: a minisign key pair made in-test and the
//! index of a real repository in the 8.04 shape, captured from `dist/`.

use minisign::KeyPair;
use proto::repository::RepositoryIndex;

pub(super) struct Signer {
    pair: KeyPair,
}

impl Signer {
    pub(super) fn new() -> Signer {
        Signer {
            pair: KeyPair::generate_unencrypted_keypair().unwrap(),
        }
    }

    pub(super) fn key(&self) -> String {
        self.pair.pk.to_base64()
    }

    pub(super) fn sign(&self, data: &[u8], comment: &str) -> String {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            data,
            Some(comment),
            None,
        )
        .unwrap()
        .to_string()
    }
}

/// The captured index with `key` declared and `change` applied: plugins `aa`
/// (0.1.0 and 0.2.0 built) and `bb` (0.1.0 built), index version 3.
pub(super) fn sample_index(key: &str, change: impl FnOnce(&mut RepositoryIndex)) -> Vec<u8> {
    let mut index: RepositoryIndex =
        serde_json::from_str(include_str!("fixtures/hestia-index.json")).unwrap();
    index.key = key.to_string();
    change(&mut index);
    serde_json::to_vec(&index).unwrap()
}
