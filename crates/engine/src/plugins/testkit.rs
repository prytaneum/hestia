//! Shared by the repository tests: a minisign key pair made in-test and the
//! index of a real repository in the 8.04 shape, captured from `dist/`.

use minisign::KeyPair;
use proto::repository::RepositoryIndex;

pub(crate) struct Signer {
    pair: KeyPair,
}

impl Signer {
    pub(crate) fn new() -> Signer {
        Signer {
            pair: KeyPair::generate_unencrypted_keypair().unwrap(),
        }
    }

    pub(crate) fn key(&self) -> String {
        self.pair.pk.to_base64()
    }

    pub(crate) fn sign(&self, data: &[u8], comment: &str) -> String {
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

/// The signing time of [`sample_index`]; its `expires` is a lifetime later.
pub(crate) const SAMPLE_VERSION: u64 = 1_700_000_000;

/// A moment while [`sample_index`] is fresh.
pub(crate) const SAMPLE_NOW: i64 = SAMPLE_VERSION as i64 + 1;

/// The sample index with `key` declared and `change` applied: plugins `aa`
/// (0.1.0 and 0.2.0 built) and `bb` (0.1.0 built), signed at [`SAMPLE_VERSION`].
pub(crate) fn sample_index(key: &str, change: impl FnOnce(&mut RepositoryIndex)) -> Vec<u8> {
    let mut index: RepositoryIndex =
        serde_json::from_str(include_str!("fixtures/plugins.json")).unwrap();
    index.key = key.to_string();
    change(&mut index);
    serde_json::to_vec(&index).unwrap()
}

/// The signature `signer` makes over the bytes of an index.
pub(crate) fn sign_index(signer: &Signer, bytes: &[u8]) -> String {
    let index: RepositoryIndex = serde_json::from_slice(bytes).unwrap();
    signer.sign(
        bytes,
        &proto::repository::index_comment(index.version, index.expires),
    )
}
