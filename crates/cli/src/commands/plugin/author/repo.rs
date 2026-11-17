//! A repository directory: its index, verified before anything is changed, and
//! the signing that replaces it. It is what a web server serves: `index.json`,
//! its signature `index.json.minisig` and the packages the index lists, each
//! under the file name of its URL.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use kernel::{Index, LIFETIME_DAYS};
use minisign::SecretKey;

use super::key;

pub const INDEX: &str = "index.json";
pub const SIGNATURE: &str = "index.json.minisig";
const DAY: u64 = 24 * 3600;

pub struct Repository {
    dir: PathBuf,
    secret: SecretKey,
    pub index: Index,
}

impl Repository {
    /// The repository in `dir` as its own key signed it. An index that is not
    /// signed by `key`, or whose signature does not cover what it says, is
    /// refused, so nothing planted in the directory is signed by the next
    /// change. One that has expired is still read: refreshing it is the
    /// repair. A directory without an index starts one named `name`.
    pub fn open(dir: &Path, key_file: &Path, name: Option<&str>) -> Result<Self> {
        let secret = key::load(key_file)?;
        let path = dir.join(INDEX);
        let index = match std::fs::read(&path) {
            Ok(body) => {
                let signature = std::fs::read_to_string(dir.join(SIGNATURE))
                    .with_context(|| format!("cannot read {SIGNATURE}"))?;
                let held: Index = serde_json::from_slice(&body)
                    .with_context(|| format!("{} is not an index", path.display()))?;
                if name.is_some_and(|name| name != held.repository) {
                    bail!("the repository is named '{}'", held.repository);
                }
                Index::verify(
                    &body,
                    &signature,
                    &key::signer(&secret)?,
                    &held.repository,
                    held.sequence,
                    held.expires.saturating_sub(1),
                )
                .with_context(|| format!("{INDEX} is not signed by this key"))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = name.context("the repository has no index yet; give its --name")?;
                if !kernel::id::is_valid(name) {
                    bail!("'{name}' is not a valid repository name");
                }
                Index {
                    repository: name.to_owned(),
                    sequence: 0,
                    expires: 0,
                    hosts: Vec::new(),
                    plugins: Vec::new(),
                    revoked: Vec::new(),
                }
            }
            Err(error) => {
                return Err(error).with_context(|| format!("cannot read {}", path.display()))
            }
        };
        Ok(Repository {
            dir: dir.to_owned(),
            secret,
            index,
        })
    }

    /// The path of a file in the repository directory.
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Signs the index as it now is under the next sequence and a fresh expiry,
    /// checks it verifies as a client would, and writes `files` (the packages
    /// it lists) before the index and its signature. The sequence it carries.
    pub fn commit(mut self, now: u64, files: &[(&str, &[u8])]) -> Result<u64> {
        self.index.sequence += 1;
        self.index.expires = now + (LIFETIME_DAYS - 1) * DAY;
        let index = &self.index;
        let body = serde_json::to_vec_pretty(index)?;
        let signature = key::sign(
            &self.secret,
            &body,
            &format!(
                "hestia-index {} {} {}",
                index.repository, index.sequence, index.expires
            ),
        )?;
        Index::verify(
            &body,
            &signature,
            &key::signer(&self.secret)?,
            &index.repository,
            index.sequence,
            now,
        )
        .context("the new index would not be accepted")?;
        let write = |file: &str, bytes: &[u8]| {
            common::atomic::write(&self.file(file), bytes)
                .with_context(|| format!("cannot write {file}"))
        };
        for (name, bytes) in files {
            write(name, bytes)?;
        }
        write(INDEX, &body)?;
        write(SIGNATURE, signature.as_bytes())?;
        Ok(index.sequence)
    }
}
