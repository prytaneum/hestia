//! `hestia updates` — every entry's content updates, as the daemon's own checks
//! found them. The daemon checks at startup, when the network returns, as an
//! instance launches and when an entry's content is listed, so this answers at
//! once without asking upstream.

use anyhow::{bail, Result};
use clap::Args;
use client::proto::error::EntryKind;

use crate::commands::content::{self, format::kind_plural};
use crate::ui::{self, View};

#[derive(Args)]
pub struct Updates {
    /// Apply them: every entry with an update moves to the newest versions,
    /// passing held items by
    #[arg(long)]
    apply: bool,
}

pub async fn run(args: Updates) -> Result<()> {
    let client = super::connect().await?;
    let entries: Vec<_> = client
        .content()
        .available_updates()
        .await?
        .into_iter()
        .filter(|e| e.updates.iter().any(|u| u.updatable))
        .collect();
    if entries.is_empty() {
        return ui::show(View::note("no content updates known"));
    }
    let rows = entries
        .iter()
        .flat_map(|e| {
            e.updates.iter().filter(|u| u.updatable).map(|u| {
                vec![
                    e.name.clone(),
                    entry_label(e.target.kind).to_string(),
                    u.title.clone(),
                    kind_plural(u.kind).to_string(),
                    u.current_version_number.clone(),
                    u.latest_version_number.clone(),
                    if u.held { "held" } else { "-" }.to_string(),
                ]
            })
        })
        .collect();
    ui::show(View::table(
        "content updates",
        ["ENTRY", "TYPE", "ITEM", "KIND", "CURRENT", "LATEST", "HELD"],
        rows,
    ))?;
    if !args.apply {
        return ui::show(View::note("apply them with `hestia updates --apply`"));
    }

    let mut failed = 0usize;
    for entry in &entries {
        let mut kinds = Vec::new();
        for update in entry.updates.iter().filter(|u| u.updatable && !u.held) {
            if !kinds.contains(&update.kind) {
                kinds.push(update.kind);
            }
        }
        for kind in kinds {
            let outcome = content::update(
                &client,
                entry.target.kind,
                kind,
                &entry.target.entry,
                Vec::new(),
                None,
            )
            .await;
            if let Err(e) = outcome {
                failed += 1;
                ui::show(View::warning(format!(
                    "'{}' {}: {e:#}",
                    entry.name,
                    kind_plural(kind)
                )))?;
            }
        }
    }
    if failed > 0 {
        bail!("{failed} update batch(es) did not apply");
    }
    Ok(())
}

fn entry_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Server => "server",
        EntryKind::Instance => "instance",
    }
}
