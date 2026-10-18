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

    pub async fn download_update(&self, on_progress: &ProgressFn<'_>) -> Result<(PathBuf, String)> {
        self.update
            .download(self.update_channel(), on_progress)
            .await
    }
}
