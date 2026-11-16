use std::path::{Path, PathBuf};

use common::schema::{self, Document};
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

    pub fn list(&self) -> Vec<Repository> {
        schema::load::<Saved>(&self.path)
            .map(|saved| saved.repositories)
            .unwrap_or_default()
    }

    pub fn get(&self, id: &str) -> Option<Repository> {
        self.list()
            .into_iter()
            .find(|repository| repository.id == id)
    }

    pub fn put(&self, repository: Repository) -> Result<(), common::schema::SchemaError> {
        let mut repositories = self.list();
        repositories.retain(|existing| existing.id != repository.id);
        repositories.push(repository);
        schema::save(&self.path, &Saved { repositories })
    }

    pub fn remove(&self, id: &str) -> Result<bool, common::schema::SchemaError> {
        let mut repositories = self.list();
        let before = repositories.len();
        repositories.retain(|existing| existing.id != id);
        let removed = repositories.len() != before;
        schema::save(&self.path, &Saved { repositories })?;
        Ok(removed)
    }
}
