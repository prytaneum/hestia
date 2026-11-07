//! Where a Java runtime build comes from. A provider only resolves: it names
//! the archive, and `Java` downloads, verifies and extracts it.

use anyhow::Result;
use async_trait::async_trait;
use proto::download::Checksum;
use proto::java::JavaRelease;

use super::platform::JavaTarget;

/// One resolved build of a runtime, ready to download.
pub struct JavaPackage {
    pub vendor: String,
    pub major: i32,
    pub release_name: String,
    pub url: String,
    pub archive_name: String,
    pub checksum: Checksum,
    /// `0` when the provider does not know it before the download starts.
    pub size: u64,
}

#[async_trait]
pub trait JavaProvider: Send + Sync {
    /// The id the provider is selected by: bare for a built-in, `<plugin>.<id>` otherwise.
    fn id(&self) -> &str;

    /// The feature versions on offer, ascending.
    async fn releases(&self) -> Result<Vec<JavaRelease>>;

    /// The build of `major` for `target`; an error when there is none.
    async fn resolve(&self, major: i32, target: &JavaTarget) -> Result<JavaPackage>;
}
