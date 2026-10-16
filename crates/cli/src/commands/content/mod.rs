//! Third-party content: the kind-first browse commands (`hestia mod search`,
//! `resourcepack info`, …) and the shared per-entry management grammar
//! (`hestia instance mod add|list|remove|update`, `server mod …`). Browsing
//! hits a content source directly; management installs into a server or
//! instance over the daemon. Every argument omitted on a terminal is asked for
//! interactively; piped invocations must pass it.

mod browse;
pub mod format;
mod manage;
mod session;

use anyhow::Result;
use clap::ValueEnum;
use client::proto::content::SearchSort;

use crate::commands::connect;
use crate::ui::{self, View};

pub use browse::{run_browse, BrowseCmd};
pub use manage::{run_entry, ContentCmd};

const PAGE: u32 = 20;

#[derive(Clone, Copy, ValueEnum)]
pub enum SortArg {
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

impl From<SortArg> for SearchSort {
    fn from(sort: SortArg) -> Self {
        match sort {
            SortArg::Relevance => SearchSort::Relevance,
            SortArg::Downloads => SearchSort::Downloads,
            SortArg::Follows => SearchSort::Follows,
            SortArg::Newest => SearchSort::Newest,
            SortArg::Updated => SearchSort::Updated,
        }
    }
}

/// `hestia sources`. Only sources that can serve are listed — a platform
/// needing an API key it has not been given is not one of them.
pub async fn run_sources() -> Result<()> {
    let client = connect().await?;
    let sources = client.content().sources().await?;
    if sources.is_empty() {
        return ui::show(View::note("no content source is configured"));
    }
    let rows = sources
        .into_iter()
        .map(|s| {
            vec![
                s.id,
                s.name,
                s.kinds
                    .iter()
                    .map(|k| format::kind_plural(*k))
                    .collect::<Vec<_>>()
                    .join(", "),
            ]
        })
        .collect::<Vec<_>>();
    ui::show(View::table(
        "content sources",
        ["ID", "NAME", "SERVES"],
        rows,
    ))
}
