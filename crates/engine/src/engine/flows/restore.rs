//! Making an imported entry's content list true: every tracked item either has
//! its file, is fetched from the source that published it, or leaves the list.

use std::path::{Path, PathBuf};

use anyhow::Result;
use proto::content::InstalledContent;
use proto::download::{Checksum, HashAlgorithm};
use proto::minecraft::{Artifact, ProvisionPhase, ProvisionProgress};
use proto::warning::WarningInfo;

use super::batch::{gather, Batch};
use crate::cancel::Job;
use crate::content::install;
use crate::engine::Engine;
use crate::minecraft::materialize;

impl Engine {
    /// A fetch that fails, or whose bytes do not match the recorded SHA-1, fails
    /// the whole restore: the caller unwinds the import and it can be retried.
    pub(super) async fn restore_tracked_content(
        &self,
        entry_dir: &Path,
        job: &Job<'_>,
    ) -> Result<Vec<WarningInfo>> {
        let mut kept = Vec::new();
        let mut missing: Vec<(InstalledContent, PathBuf)> = Vec::new();
        let mut dropped: Vec<String> = Vec::new();
        for item in install::load(entry_dir) {
            match install::managed_path(entry_dir, &item) {
                Ok(managed) if managed.is_file() => kept.push(item),
                Ok(managed) if install::referenceable(&item) => {
                    missing.push((item.clone(), managed));
                    kept.push(item);
                }
                _ => dropped.push(item.filename),
            }
        }
        if missing.is_empty() && dropped.is_empty() {
            return Ok(Vec::new());
        }
        tracing::info!(
            fetching = missing.len(),
            dropping = dropped.len(),
            "restoring tracked content"
        );

        let batch = &Batch::new(job, ProvisionPhase::Content, missing.iter().map(|_| 0));
        // A loop, not `map`: a closure taking a reference and returning an
        // async block trips rustc's higher-ranked lifetime inference
        // (rust-lang/rust#89976).
        let mut fetches = Vec::new();
        for (item, managed) in &missing {
            fetches.push(async move {
                let share = batch.share(0);
                let relay = |progress: &ProvisionProgress| share.relay(progress);
                self.fetch_tracked(item, managed, &Job::new(&relay, job.cancel()))
                    .await
            });
        }
        gather(fetches).await.into_iter().collect::<Result<()>>()?;

        if dropped.is_empty() {
            return Ok(Vec::new());
        }
        install::save(entry_dir, kept)?;
        tracing::warn!(
            count = dropped.len(),
            "dropped tracked content with no bytes and no download"
        );
        Ok(vec![WarningInfo::ImportContentMissing {
            count: dropped.len() as u32,
            files: dropped,
        }])
    }

    async fn fetch_tracked(
        &self,
        item: &InstalledContent,
        managed: &Path,
        job: &Job<'_>,
    ) -> Result<()> {
        materialize::validate_filename(&item.filename)?;
        let artifact = Artifact {
            url: item.url.clone(),
            filename: item.filename.clone(),
            size: 0,
            checksum: Some(Checksum {
                algorithm: HashAlgorithm::Sha1,
                hex: item.sha1.clone(),
            }),
        };
        materialize::ensure_artifact(
            Some(&self.cache),
            &artifact,
            managed,
            ProvisionPhase::Content,
            job,
        )
        .await
    }
}
