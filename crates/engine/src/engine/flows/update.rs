//! Self-update against the channel the settings name, so the feed is chosen in
//! one place rather than by each caller of the update store.

use std::path::PathBuf;

use anyhow::Result;
use proto::update::{UpdateChannel, UpdateCheckResult};

use super::Engine;
use crate::download::ProgressFn;

impl Engine {
    fn update_channel(&self) -> UpdateChannel {
        self.config.settings().update.channel
    }

    pub async fn check_update(&self) -> Result<UpdateCheckResult> {
        self.update.check(self.update_channel()).await
    }

    /// Stages the official plugins a release that changes the plugin API
    /// needs before downloading it, so an update that would leave one without
    /// a build is refused before anything is fetched.
    pub async fn download_update(&self, on_progress: &ProgressFn<'_>) -> Result<(PathBuf, String)> {
        let release = self.update.release(self.update_channel()).await?;
        if let Some(api) = release.plugin_api.as_deref() {
            self.stage_official_plugins(api).await?;
        }
        let path = self.update.download(&release, on_progress).await?;
        Ok((path, release.version))
    }

    pub fn staged_plugins(&self, api: &str) -> PathBuf {
        self.update.staged_plugins(api)
    }

    pub fn discard_staged_plugins(&self) {
        self.update.discard_staged_plugins()
    }
}
