use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};

use super::error::TrustError;

/// A repository's pinned minisign key.
#[derive(Debug, Clone)]
pub struct Signer {
    key: PublicKey,
    text: String,
}

impl Signer {
    /// The base64 key line, or the two lines of a `minisign.pub` file.
    pub fn parse(text: &str) -> Result<Self, TrustError> {
        let line = text.trim().lines().last().unwrap_or_default().trim();
        let key = PublicKey::from_base64(line).map_err(|_| TrustError::BadKey)?;
        Ok(Signer {
            key,
            text: line.to_owned(),
        })
    }

    pub fn key(&self) -> &str {
        &self.text
    }

    /// A short stable name for the key, recorded with what it signed.
    pub fn fingerprint(&self) -> String {
        Sha256::digest(self.text.as_bytes())[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
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
