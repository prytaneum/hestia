//! Giving an entry its icon: a file someone picked, an image a front-end
//! composed, or the icon of the pack the entry was made from.

use std::path::PathBuf;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use proto::error::ErrorInfo;
use proto::icon::{EntryIcon, IconSource};

use super::content::entry::EntryRef;
use super::transfer::daemon_local_path;
use crate::engine::Engine;
use crate::icon;

impl Engine {
    pub fn set_entry_icon(&self, entry: EntryRef<'_>, source: &IconSource) -> Result<EntryIcon> {
        let entry_dir = self.entry_dir(entry)?;
        match source {
            IconSource::File { path } => {
                let image = icon::read_file(&daemon_local_path(path)?)?;
                icon::write(&entry_dir, &image, None)
            }
            IconSource::Generated { image, generator } => {
                let image = STANDARD
                    .decode(image)
                    .map_err(|_| ErrorInfo::MalformedRequest {
                        detail: "the icon image is not base64".to_string(),
                    })?;
                icon::write(&entry_dir, &image, Some(generator))
            }
        }
    }

    /// An icon only dresses the entry, so failing to inherit one is not worth
    /// failing the pack install over.
    pub(super) async fn inherit_pack_icon(&self, entry: EntryRef<'_>, url: &str) {
        if url.is_empty() {
            return;
        }
        let inherited = async {
            let image = icon::fetch(url).await?;
            icon::write(&self.entry_dir(entry)?, &image, None)
        };
        if let Err(e) = inherited.await {
            tracing::warn!(url, error = %format!("{e:#}"), "the pack's icon was not inherited");
        }
    }

    fn entry_dir(&self, entry: EntryRef<'_>) -> Result<PathBuf> {
        Ok(match entry {
            EntryRef::Server(reference) => {
                self.servers.server_dir(&self.servers.require(reference)?)
            }
            EntryRef::Instance(reference) => self
                .instances
                .instance_dir(&self.instances.require(reference)?),
        })
    }
}
