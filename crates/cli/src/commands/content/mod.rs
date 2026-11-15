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
use client::proto::content::{RequiredSetting, SearchSort};

use crate::commands::connect;
use crate::commands::names::Names;
use crate::ui::{self, Cell, View};

pub use browse::{run_browse, BrowseCmd};
pub(crate) use manage::update;
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

/// `hestia sources`. A source waiting on a setting is listed with the setting.
pub async fn run_sources() -> Result<()> {
    let client = connect().await?;
    let sources = client.content().sources().await?;
    if sources.is_empty() {
        return ui::show(View::empty(
            "no content source is installed — install a content source plugin",
        ));
    }
    let names = Names::of(sources.iter().map(|s| s.id.clone()));
    let rows = sources
        .into_iter()
        .map(|s| {
            vec![
                names.show(&s.id).into(),
                s.name.into(),
                Cell::list(
                    s.kinds
                        .iter()
                        .map(|k| format::kind_plural(*k))
                        .collect::<Vec<_>>()
                        .join(", "),
                    &s.kinds,
                ),
                match s.requires_setting {
                    None => Cell::token("ready", &"ready"),
                    Some(RequiredSetting { plugin, key }) => Cell::token(
                        format!("needs plugin setting {key} of {plugin}"),
                        &"needs_setting",
                    ),
                },
            ]
        })
        .collect::<Vec<_>>();
    ui::show(View::table(
        "content sources",
        ["ID", "NAME", "SERVES", "STATUS"],
        rows,
    ))
}
