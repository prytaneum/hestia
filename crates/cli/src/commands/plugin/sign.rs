//! Signing with the repository's minisign key, shared by `pack` and `index`.
//! The trusted comments come from `proto::repository`, the same functions the
//! daemon compares against.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use minisign::{PublicKey, SecretKey, SecretKeyBox};

pub struct Signer {
    secret: SecretKey,
    public: PublicKey,
}

impl Signer {
    /// Loads the secret key at `path`, asking for its password when it is
    /// encrypted, and refuses one that is not the key the repository declares:
    /// everything it signed would fail verification for every user.
    pub fn load(path: &Path, declared_key: &str) -> Result<Signer> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read the secret key {}", path.display()))?;
        let secret = match SecretKeyBox::from_string(&text)?.into_unencrypted_secret_key() {
            Ok(secret) => secret,
            Err(plain) => SecretKeyBox::from_string(&text)?
                .into_secret_key(None)
                .map_err(|locked| {
                    anyhow::anyhow!(
                        "cannot load the secret key {}: {plain}; as an encrypted key: {locked} (make keys with `hestia plugin keygen`)",
                        path.display()
                    )
                })?,
        };
        let public = PublicKey::from_secret_key(&secret)?;
        if public.to_base64() != declared_key.trim() {
            bail!(
                "{} is not the key declared in [workspace.metadata.hestia]; run `hestia plugin keygen` or pass the matching secret key",
                path.display()
            );
        }
        Ok(Signer { secret, public })
    }

    /// Writes `<target>.minisig` beside `target`, signing `data` with `comment`
    /// as its trusted comment, and returns the path.
    pub fn write(&self, data: &[u8], comment: &str, target: &Path) -> Result<PathBuf> {
        let signature =
            minisign::sign(Some(&self.public), &self.secret, data, Some(comment), None)?;
        let mut name = target.as_os_str().to_owned();
        name.push(".minisig");
        let path = PathBuf::from(name);
        std::fs::write(&path, signature.to_string())
            .with_context(|| format!("cannot write {}", path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use minisign::{KeyPair, SignatureBox};

    use super::*;

    fn key_file(dir: &Path) -> (PathBuf, PublicKey) {
        let pair = KeyPair::generate_unencrypted_keypair().unwrap();
        let path = dir.join("hestia.key");
        std::fs::write(&path, pair.sk.to_box(None).unwrap().to_string()).unwrap();
        (path, pair.pk)
    }

    #[test]
    fn a_signature_verifies_with_the_declared_key_and_carries_the_comment() {
        let dir = tempfile::tempdir().unwrap();
        let (path, public) = key_file(dir.path());
        let signer = Signer::load(&path, &public.to_base64()).unwrap();
        let target = dir.path().join("aa-0.1.0.hplugin");
        let comment = proto::repository::build_comment("aa", "0.1.0", "aa-0.1.0.hplugin");

        let written = signer.write(b"package", &comment, &target).unwrap();

        assert_eq!(written, dir.path().join("aa-0.1.0.hplugin.minisig"));
        let signature = SignatureBox::from_file(&written).unwrap();
        minisign::verify(
            &public,
            &signature,
            std::io::Cursor::new(b"package"),
            true,
            false,
            false,
        )
        .unwrap();
        assert_eq!(signature.trusted_comment().unwrap(), comment);
    }

    #[test]
    fn a_secret_key_that_is_not_the_declared_one_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (path, _) = key_file(dir.path());
        let declared = KeyPair::generate_unencrypted_keypair().unwrap().pk;

        assert!(Signer::load(&path, &declared.to_base64()).is_err());
    }
}
