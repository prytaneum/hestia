//! The discovery half: `hestia <kind> search|info|versions` against a content
//! source. On a terminal, search opens the fullscreen browse session; piped
//! it prints one page of results plainly.

use anyhow::Result;
use clap::Subcommand;
use client::proto::content::{
    ContentKind, ContentProject, ContentVersion, SearchQuery, VersionQuery,
};
use client::Client;

use super::format::{channel_label, compact, side_label, truncate};
use super::{session, SortArg, PAGE};
use crate::commands::connect;
use crate::ui::{self, Spinner, View};

/// The browse grammar shared by every content kind (`mod`, `modpack`,
/// resourcepack, shader, datapack).
#[derive(Subcommand)]
pub enum BrowseCmd {
    /// Search a source (browses when the query is omitted)
    Search {
        /// Search terms
        query: Option<String>,
        #[arg(short, long, help = "Filter by loader (e.g. fabric)")]
        loader: Option<String>,
        #[arg(short = 'g', long = "game-version", help = "Filter by game version")]
        game_version: Option<String>,
        #[arg(short, long, help = "Filter by category (repeatable)")]
        category: Vec<String>,
        #[arg(short, long, value_enum, default_value_t = SortArg::Relevance, help = "Result ordering")]
        sort: SortArg,
        #[arg(
            short = 'S',
            long,
            help = "Content source (default: the first configured one)"
        )]
        source: Option<String>,
        #[arg(long, default_value_t = PAGE, help = "Results per page")]
        limit: u32,
        #[arg(long, default_value_t = 0, help = "Skip this many results")]
        offset: u32,
    },
    /// A project's details (accepts a slug or id)
    Info {
        /// Project slug or id
        project: String,
        #[arg(
            short = 'S',
            long,
            help = "Content source (default: the first configured one)"
        )]
        source: Option<String>,
    },
    /// A project's downloadable versions, newest first
    Versions {
        /// Project slug or id
        project: String,
        #[arg(short, long, help = "Filter by loader (e.g. fabric)")]
        loader: Option<String>,
        #[arg(short = 'g', long = "game-version", help = "Filter by game version")]
        game_version: Option<String>,
        #[arg(
            short = 'S',
            long,
            help = "Content source (default: the first configured one)"
        )]
        source: Option<String>,
    },
}

/// `hestia <kind> search|info|versions`.
pub async fn run_browse(kind: ContentKind, cmd: BrowseCmd) -> Result<()> {
    let client = connect().await?;
    match cmd {
        BrowseCmd::Search {
            query,
            loader,
            game_version,
            category,
            sort,
            source,
            limit,
            offset,
        } => {
            let bare_query = query.is_none();
            let base = SearchQuery {
                source: source.unwrap_or_default(),
                kind,
                query: query.unwrap_or_default(),
                loaders: loader.into_iter().collect(),
                game_version,
                categories: category,
                sort: sort.into(),
                limit: limit.clamp(1, 100),
                offset,
            };
            if bare_query && ui::interactive_output() {
                session::run(&client, base, None).await?;
                return Ok(());
            }
            search_page(&client, base).await
        }
        BrowseCmd::Info { project, source } => {
            let detail = {
                let _spinner = Spinner::start("fetching project");
                client
                    .content()
                    .project(&source.unwrap_or_default(), &project, Some(kind))
                    .await?
            };
            show_project(&detail)
        }
        BrowseCmd::Versions {
            project,
            loader,
            game_version,
            source,
        } => {
            let versions = {
                let _spinner = Spinner::start("fetching versions");
                client
                    .content()
                    .versions(&VersionQuery {
                        source: source.unwrap_or_default(),
                        project,
                        loaders: loader.into_iter().collect(),
                        game_version,
                    })
                    .await?
            };
            show_versions(versions)
        }
    }
}

/// Print one page of search results plainly (the piped path).
async fn search_page(client: &Client, query: SearchQuery) -> Result<()> {
    let result = {
        let _spinner = Spinner::start("searching");
        client.content().search(&query).await?
    };
    if result.hits.is_empty() {
        return ui::show(View::empty("no results"));
    }
    let rows = result
        .hits
        .iter()
        .map(|h| {
            vec![
                h.title.clone(),
                h.slug.clone(),
                compact(h.downloads),
                truncate(&h.description, 60),
            ]
        })
        .collect();
    ui::show(View::table(
        "results",
        ["NAME", "SLUG", "DOWNLOADS", "DESCRIPTION"],
        rows,
    ))?;
    let shown_to = query.offset + result.hits.len() as u32;
    ui::show(View::note(format!(
        "showing {}–{shown_to} of {} (--offset pages)",
        query.offset + 1,
        result.total
    )))
}

fn show_project(project: &ContentProject) -> Result<()> {
    ui::show(View::detail([
        ("title", project.title.clone()),
        ("slug", project.slug.clone()),
        ("id", project.id.clone()),
        ("source", project.source.clone()),
        ("downloads", compact(project.downloads)),
        ("follows", compact(project.follows)),
        ("client", side_label(project.client_side)),
        ("server", side_label(project.server_side)),
        ("categories", project.categories.join(", ")),
        ("description", project.description.clone()),
    ]))
}

fn show_versions(versions: Vec<ContentVersion>) -> Result<()> {
    if versions.is_empty() {
        return ui::show(View::empty("no versions"));
    }
    let rows = versions
        .iter()
        .map(|v| {
            vec![
                v.version_number.clone(),
                channel_label(v.channel).to_string(),
                v.game_versions.join(", "),
                v.loaders.join(", "),
            ]
        })
        .collect();
    ui::show(View::table(
        "versions",
        ["VERSION", "CHANNEL", "GAME", "LOADERS"],
        rows,
    ))
}
