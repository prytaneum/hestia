//! Minisign verification, shared by everything the launcher fetches and trusts.
//!
//! Each consumer brings its own key set — release artifacts are verified against
//! [`common::app::update_pubkeys`] — so a key that can sign one artifact cannot
//! sign another.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;

/// Accept `data` if *any* key in `keys` verifies it. The signature names one
/// key, so a rotation signs with the successor while builds in the field still
/// trust the predecessor — the reason a key set is a list rather than a
/// constant. An **empty** key set verifies nothing: a caller with no compiled-in
/// key fails closed rather than trusting whatever it was handed.
pub fn verify_bytes(
    data: &[u8],
    signature: &str,
    keys: impl Iterator<Item = &'static str>,
) -> Result<()> {
    // Checked before the signature is even parsed: with no key, no signature
    // could pass, and "malformed signature" would misreport why.
    let keys: Vec<&str> = keys.collect();
    if keys.is_empty() {
        return Err(anyhow!("no signing key is compiled into this build"));
    }
    // Wire contract with tauri's signer: the public key and the signature are
    // both base64-wrapped minisign documents.
    let signature = base64_text(signature).context("bad signature")?;
    let signature = minisign_verify::Signature::decode(&signature).map_err(|e| anyhow!("{e}"))?;
    for key in keys {
        let pubkey = base64_text(key).context("bad public key")?;
        let pubkey = minisign_verify::PublicKey::decode(&pubkey).map_err(|e| anyhow!("{e}"))?;
        if pubkey.verify(data, &signature, true).is_ok() {
            return Ok(());
        }
    }
    Err(anyhow!("no trusted key verifies this artifact"))
}

/// [`verify_bytes`] over a file's contents.
pub fn verify_file(
    path: &Path,
    signature: &str,
    keys: impl Iterator<Item = &'static str>,
) -> Result<()> {
    let data = std::fs::read(path).context("cannot read the file to verify")?;
    verify_bytes(&data, signature, keys)
}

fn base64_text(value: &str) -> Result<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(value.trim())?;
    Ok(String::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::verify_bytes;

    #[test]
    fn an_empty_key_set_verifies_nothing() {
        // Fails closed: a build with no compiled-in key must refuse the
        // artifact, never treat "nothing to check against" as a pass.
        let err = verify_bytes(b"payload", "", std::iter::empty()).unwrap_err();
        assert!(err.to_string().contains("no signing key"), "{err}");
    }
}
