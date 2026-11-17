use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use super::error::TrustError;

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const ED25519_AT: std::ops::Range<usize> = 10..42;

/// A minisign public key. Two keys are the same when their ed25519 bytes are:
/// minisign's key id sits beside the key and nothing signs it, so it never
/// decides who someone is.
#[derive(Debug, Clone)]
pub struct Signer {
    key: PublicKey,
    text: String,
    bytes: [u8; 32],
}

fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut bits, mut have) = (0u32, 0u32);
    for byte in text.trim_end_matches('=').bytes() {
        bits = bits << 6 | BASE64.iter().position(|&c| c == byte)? as u32;
        have += 6;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
            bits &= (1 << have) - 1;
        }
    }
    Some(out)
}

impl Signer {
    /// The base64 key line, or the two lines of a `minisign.pub` file.
    pub fn parse(text: &str) -> Result<Self, TrustError> {
        let line = text.trim().lines().last().unwrap_or_default().trim();
        let key = PublicKey::from_base64(line).map_err(|_| TrustError::BadKey)?;
        let bytes = decode(line)
            .and_then(|raw| raw.get(ED25519_AT)?.try_into().ok())
            .ok_or(TrustError::BadKey)?;
        Ok(Signer {
            key,
            text: line.to_owned(),
            bytes,
        })
    }

    /// The base64 key line, whole.
    pub fn key(&self) -> &str {
        &self.text
    }

    pub fn same_key(&self, other: &Signer) -> bool {
        self.bytes == other.bytes
    }

    /// SHA-256 of the ed25519 key, in hex: what a person compares.
    pub fn fingerprint(&self) -> String {
        hex::encode(Sha256::digest(self.bytes))
    }

    /// The trusted comment of `signature` once it verifies over `body`.
    pub fn verify(&self, body: &[u8], signature: &str) -> Result<String, TrustError> {
        let signature = Signature::decode(signature).map_err(|_| TrustError::BadSignature)?;
        self.key
            .verify(body, &signature, false)
            .map_err(|_| TrustError::BadSignature)?;
        Ok(signature.trusted_comment().to_owned())
    }
}

#[cfg(feature = "sandbox")]
/// Whether two key lines name the same key.
pub(crate) fn same_key(a: &str, b: &str) -> bool {
    matches!((Signer::parse(a), Signer::parse(b)), (Ok(a), Ok(b)) if a.same_key(&b))
}
