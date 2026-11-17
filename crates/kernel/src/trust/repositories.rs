use std::path::{Path, PathBuf};

use common::schema::{self, Document, SchemaError};
use serde::{Deserialize, Serialize};

use super::error::TrustError;
use super::signature::Signer;

/// A repository the user added: where its index is, the key every index must
/// verify under, and the highest sequence seen.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub url: String,
    pub key: String,
    pub sequence: u64,
}

impl Repository {
    pub fn signer(&self, official: &[Signer]) -> Result<Signer, TrustError> {
        pinned(&self.key, official)
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Saved {
    repositories: Vec<Repository>,
}

impl Document for Saved {
    const NAME: &'static str = "repositories.json";
}

/// A key a repository may pin: never one of Hestia's own, whose signatures stand
/// for Hestia and so cannot be vouched by anyone who merely names a URL.
pub fn pinned(key: &str, official: &[Signer]) -> Result<Signer, TrustError> {
    let signer = Signer::parse(key)?;
    match official.iter().any(|own| own.same_key(&signer)) {
        true => Err(TrustError::OfficialKey),
        false => Ok(signer),
    }
}

/// The repositories on disk. The disk is the registry: every call reads it, and
/// the caller serialises changes.
#[derive(Clone)]
pub struct Repositories {
    path: PathBuf,
}

impl Repositories {
    pub fn new(data_home: &Path) -> Self {
        Repositories {
            path: data_home.join(Saved::NAME),
        }
    }

    /// Only a missing file is an empty list. One that cannot be read is an
    /// error, because the next `put` would otherwise replace it and forget the
    /// pinned keys and the highest sequence seen, which replay protection rests on.
    pub fn list(&self) -> Result<Vec<Repository>, SchemaError> {
        Ok(schema::try_load::<Saved>(&self.path)?
            .map(|saved| saved.repositories)
            .unwrap_or_default())
    }

    pub fn get(&self, id: &str) -> Result<Option<Repository>, SchemaError> {
        Ok(self
            .list()?
            .into_iter()
            .find(|repository| repository.id == id))
    }

    pub fn put(&self, repository: Repository) -> Result<(), SchemaError> {
        let mut repositories = self.list()?;
        repositories.retain(|existing| existing.id != repository.id);
        repositories.push(repository);
        schema::save(&self.path, &Saved { repositories })
    }

    pub fn remove(&self, id: &str) -> Result<bool, SchemaError> {
        let mut repositories = self.list()?;
        let before = repositories.len();
        repositories.retain(|existing| existing.id != id);
        let removed = repositories.len() != before;
        schema::save(&self.path, &Saved { repositories })?;
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_list_that_cannot_be_read_is_an_error_and_is_never_replaced() {
        let home = tempfile::tempdir().unwrap();
        let repositories = Repositories::new(home.path());
        std::fs::create_dir(home.path().join(Saved::NAME)).unwrap();

        assert!(repositories.list().is_err());
        assert!(repositories.put(Repository::default()).is_err());
        assert!(repositories.remove("any").is_err());
        assert!(home.path().join(Saved::NAME).is_dir());
    }
}
