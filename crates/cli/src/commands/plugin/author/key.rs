//! Minisign secret keys: the author's, which signs a package, and the
//! repository's, which signs its index.

use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use kernel::Signer;
use minisign::{PublicKey, SecretKey, SecretKeyBox};

/// A secret key from its file. An encrypted one asks for its password on the
/// terminal.
pub fn load(path: &Path) -> Result<SecretKey> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read the secret key {}", path.display()))?;
    let stored = SecretKeyBox::from_string(&text)
        .with_context(|| format!("{} is not a minisign secret key", path.display()))?;
    SecretKey::from_unencrypted_box(stored.clone())
        .or_else(|_| stored.into_secret_key(None))
        .with_context(|| format!("cannot unlock the secret key {}", path.display()))
}

pub fn signer(key: &SecretKey) -> Result<Signer> {
    let public = PublicKey::from_secret_key(key).context("the secret key has no public key")?;
    Ok(Signer::parse(&public.to_base64())?)
}

/// `body` signed by `key`, the trusted comment saying what it is a signature of.
pub fn sign(key: &SecretKey, body: &[u8], comment: &str) -> Result<String> {
    Ok(
        minisign::sign(None, key, Cursor::new(body), Some(comment), None)
            .context("cannot sign")?
            .to_string(),
    )
}
