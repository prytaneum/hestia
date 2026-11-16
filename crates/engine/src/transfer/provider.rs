//! A pack format other than Hestia's own: it declares the members it reads and
//! answers what an archive holds and how to write one. The host opens the
//! archive and vets every answer; a provider never touches the filesystem.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use kernel::vetting::Policy;
use proto::provider::pack::{PackInfo, PackRead, PackWritten, WriteQuery};
use proto::warning::WarningInfo;

use super::archive::Reader;
use crate::minecraft::Loaders;

/// A provider's answer after vetting, with what the host dropped from it.
pub struct Vetted {
    pub read: PackRead,
    pub warnings: Vec<WarningInfo>,
}

#[async_trait]
pub trait PackProvider: Send + Sync {
    fn id(&self) -> &str;

    fn info(&self) -> &PackInfo;

    /// The policy the files this format names are fetched under.
    fn policy(&self) -> Arc<Policy>;

    /// The directory the archive's pack starts at, when it holds every member
    /// the format declared.
    fn root_of(&self, names: &[String]) -> Option<String>;

    /// Whether the format accepts an archive that holds its declared members.
    async fn claims(&self, reader: &mut Reader, root: &str) -> bool;

    async fn read(&self, reader: &mut Reader, root: &str, loaders: &Loaders) -> Result<Vetted>;

    async fn write(&self, query: &WriteQuery) -> Result<PackWritten>;
}
