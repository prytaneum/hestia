//! The one way a flow downloads a file into an entry: a planned file in, its
//! SHA-1 out, singly or as a batch under one progress extent.

use std::path::PathBuf;

use anyhow::{bail, Result};
use proto::error::ErrorInfo;
use proto::minecraft::{Artifact, ProvisionPhase, ProvisionProgress};

use super::batch::{gather_ordered, Batch};
use crate::cancel::Job;
use crate::content::install;
use crate::engine::Engine;
use crate::minecraft::materialize;

/// A file to fetch to `target`; `label` and `source` name it if its author has
/// opted out of distribution.
pub(super) struct Download {
    pub artifact: Artifact,
    pub target: PathBuf,
    pub label: String,
    pub source: String,
}

impl Engine {
    pub(super) async fn download(
        &self,
        download: &Download,
        phase: ProvisionPhase,
        job: &Job<'_>,
    ) -> Result<String> {
        // Routine on CurseForge: one file fails and the rest still install.
        if download.artifact.url.is_empty() {
            bail!(ErrorInfo::ContentDownloadBlocked {
                title: download.label.clone(),
                source: download.source.clone(),
            });
        }
        materialize::ensure_artifact(
            Some(&self.cache),
            &download.artifact,
            &download.target,
            phase,
            job,
        )
        .await?;
        match &download.artifact.checksum {
            Some(checksum) => Ok(checksum.hex.clone()),
            None => install::sha1_file(&download.target),
        }
    }

    /// Each item's outcome, in the order given. Items sharing a target are
    /// fetched one after the other.
    pub(super) async fn download_batch<T>(
        &self,
        items: Vec<(T, Download)>,
        phase: ProvisionPhase,
        job: &Job<'_>,
    ) -> Vec<(T, Result<String>)> {
        let batch = &Batch::new(job, phase, items.iter().map(|(_, d)| d.artifact.size));
        gather_ordered(
            items,
            |(_, download)| Some(download.target.to_string_lossy().into_owned()),
            |(tag, download)| async move {
                let share = batch.share(download.artifact.size);
                let relay = |progress: &ProvisionProgress| share.relay(progress);
                let fetched = self
                    .download(&download, phase, &Job::new(&relay, job.cancel()))
                    .await;
                (tag, fetched)
            },
        )
        .await
    }
}
