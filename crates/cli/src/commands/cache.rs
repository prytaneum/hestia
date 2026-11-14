//! `hestia cache …` — the checksummed download cache.

use anyhow::Result;
use clap::Subcommand;

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum CacheCmd {
    /// Location, entry count, and size
    Info,
    /// Cached blobs by checksum
    List,
    /// Remove everything, reporting what was freed
    Clear,
}

pub async fn run(cmd: CacheCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        CacheCmd::Info => {
            let info = client.cache().info().await?;
            ui::show(View::detail([
                ("location", info.path.display().to_string()),
                ("entries", info.usage.entries.to_string()),
                ("size", ui::human_bytes(info.usage.bytes)),
            ]))?;
        }
        CacheCmd::List => {
            let entries = client.cache().list().await?;
            if entries.is_empty() {
                return ui::show(View::empty("cache is empty"));
            }
            let rows = entries
                .iter()
                .map(|e| {
                    vec![
                        format!("{}:{}", e.checksum.algorithm.as_str(), e.checksum.hex),
                        ui::human_bytes(e.size),
                    ]
                })
                .collect();
            ui::show(View::table("cache", ["CHECKSUM", "SIZE"], rows))?;
        }
        CacheCmd::Clear => {
            let freed = client.cache().clear().await?;
            ui::show(View::line(format!(
                "freed {} across {} entries",
                ui::human_bytes(freed.bytes),
                freed.entries
            )))?;
        }
    }
    Ok(())
}
