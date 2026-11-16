//! The `.hplugin` package: a zip of exactly `plugin.toml`, `plugin.wasm` and
//! `plugin.minisig`, the last the author's signature over a statement of what
//! the other two are. Nothing of the archive is trusted until it is read under
//! its caps and the signature verifies.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};

use super::error::TrustError;
use super::signature::Signer;
use crate::install::{COMPONENT_MAX, MANIFEST_MAX};

const MANIFEST: &str = "plugin.toml";
const COMPONENT: &str = "plugin.wasm";
const SIGNATURE: &str = "plugin.minisig";
const SIGNATURE_MAX: u64 = 4 << 10;
const SYMLINK: u32 = 0o120000;
const FILE_TYPE: u32 = 0o170000;

/// The most a downloaded archive may be: the component and some headroom for
/// the other entries.
pub(crate) const ARCHIVE_MAX: u64 = COMPONENT_MAX + (1 << 20);

pub(crate) struct Hplugin {
    pub manifest: Vec<u8>,
    pub wasm: Vec<u8>,
    signature: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn refused(reason: impl Into<String>) -> TrustError {
    TrustError::Archive(reason.into())
}

/// What the author signs. The id and version are the manifest's, so a
/// signature cannot be moved to another plugin or release.
fn statement(id: &str, version: &str, manifest: &[u8], wasm: &[u8]) -> Vec<u8> {
    format!(
        "hestia-hplugin-v1\n{id}\n{version}\n{}\n{}\n",
        hex(&Sha256::digest(manifest)),
        hex(&Sha256::digest(wasm))
    )
    .into_bytes()
}

fn read_entry(entry: impl Read, name: &str, max: u64) -> Result<Vec<u8>, TrustError> {
    let mut bytes = Vec::new();
    entry
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| refused(format!("{name} cannot be read: {error}")))?;
    match bytes.len() as u64 > max {
        true => Err(refused(format!("{name} is larger than {max} bytes"))),
        false => Ok(bytes),
    }
}

impl Hplugin {
    pub fn read(archive: &[u8]) -> Result<Hplugin, TrustError> {
        if archive.len() as u64 > ARCHIVE_MAX {
            return Err(refused("it is larger than a package may be"));
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(archive))
            .map_err(|error| refused(format!("it is not a zip: {error}")))?;
        if zip.len() != 3 {
            return Err(refused(format!("it holds {} entries, not 3", zip.len())));
        }
        let (mut manifest, mut wasm, mut signature) = (None, None, None);
        let mut seen = HashSet::new();
        for index in 0..zip.len() {
            let entry = zip
                .by_index(index)
                .map_err(|error| refused(format!("an entry cannot be read: {error}")))?;
            let name = entry
                .name()
                .map_err(|error| refused(format!("an entry has no readable name: {error}")))?
                .into_owned();
            if entry.is_dir() || entry.unix_mode().is_some_and(|m| m & FILE_TYPE == SYMLINK) {
                return Err(refused(format!("{name} is not a regular file")));
            }
            if !seen.insert(name.clone()) {
                return Err(refused(format!("{name} appears twice")));
            }
            match name.as_str() {
                MANIFEST => manifest = Some(read_entry(entry, &name, MANIFEST_MAX)?),
                COMPONENT => wasm = Some(read_entry(entry, &name, COMPONENT_MAX)?),
                SIGNATURE => signature = Some(read_entry(entry, &name, SIGNATURE_MAX)?),
                _ => return Err(refused(format!("{name} is not part of a package"))),
            }
        }
        match (manifest, wasm, signature) {
            (Some(manifest), Some(wasm), Some(signature)) => Ok(Hplugin {
                manifest,
                wasm,
                signature: String::from_utf8(signature)
                    .map_err(|_| refused("plugin.minisig is not text"))?,
            }),
            _ => Err(refused(
                "it lacks plugin.toml, plugin.wasm or plugin.minisig",
            )),
        }
    }

    /// Whether `author` signed this manifest and component as `id` at `version`.
    pub fn verify(&self, author: &Signer, id: &str, version: &str) -> Result<(), TrustError> {
        let statement = statement(id, version, &self.manifest, &self.wasm);
        author
            .verify(&statement, &self.signature)
            .map(|_| ())
            .map_err(|_| TrustError::PackageSignature)
    }
}
