//! The management half: install, list, remove, and update content inside a
//! server or an instance over the daemon.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::content::{
    ContentAddItem, ContentAddSpec, ContentFailure, ContentKind, InstalledContent,
};
use client::proto::error::{EntryKind, EntryTarget};
use client::Client;

use client::proto::content::SearchQuery;
use client::proto::instance::WorldInfo;

use super::format::{kind_plural, source_label, world_label};
use super::{session, PAGE};
use crate::ui::{self, ProvisionReporter, View};

/// The per-entry management grammar shared by every installable kind under a
/// server or instance. The entry is fixed by the enclosing `server <name>` /
/// `instance <name>` context, so these verbs name only the item.
#[derive(Subcommand)]
pub enum ContentCmd {
    /// Install content (a project slug/id, a source URL, or a local --file)
    #[command(visible_alias = "install")]
    Add {
        /// Project slug/id or a source project URL (prompts to search when omitted)
        item: Option<String>,
        #[arg(long, help = "Pin a specific version (id or version number)")]
        version: Option<String>,
        #[arg(
            long,
            help = "Import a local file instead of a project",
            conflicts_with = "version"
        )]
        file: Option<String>,
        #[arg(long, help = "Override the stored filename (url/file installs)")]
        filename: Option<String>,
        #[arg(
            short = 'S',
            long,
            help = "Content source to install from (default: the first configured one)"
        )]
        source: Option<String>,
        #[arg(
            long,
            help = "For a datapack on an instance: a save world to load it in (repeatable; every world when omitted)"
        )]
        world: Vec<String>,
    },
    /// Installed content of this kind
    #[command(visible_alias = "ls")]
    List,
    /// Uninstall content (prompts to pick when omitted)
    #[command(visible_alias = "rm")]
    Remove {
        /// Installed items (project id, slug, filename, or title)
        items: Vec<String>,
        #[arg(
            long,
            help = "For a datapack: stop loading it in this save world only (repeatable)"
        )]
        world: Vec<String>,
    },
    /// Update platform content to its newest compatible version, or a local item from a --file
    Update {
        /// Installed items, or every one of this kind when omitted
        items: Vec<String>,
        #[arg(
            long,
            help = "Only report which items have an update, without applying"
        )]
        check: bool,
        #[arg(
            long,
            help = "Update the one named local item from this file",
            conflicts_with = "check"
        )]
        file: Option<String>,
    },
    /// Enable a disabled item (prompts to pick when omitted)
    Enable {
        /// Installed item (project id, slug, filename, or title)
        item: Option<String>,
        #[arg(long, help = "For a datapack: only in this save world (repeatable)")]
        world: Vec<String>,
    },
    /// Disable an item without uninstalling it (prompts to pick when omitted)
    Disable {
        /// Installed item (project id, slug, filename, or title)
        item: Option<String>,
        #[arg(long, help = "For a datapack: only in this save world (repeatable)")]
        world: Vec<String>,
    },
    /// Re-pin an item to a specific published version
    SetVersion {
        /// Installed item (project id, slug, filename, or title)
        item: String,
        /// Version id or version number to pin
        version: String,
        /// Take the version from this source instead of the item's own
        #[arg(short = 'S', long)]
        source: Option<String>,
    },
}

/// `hestia <server|instance> <name> <kind> add|list|remove|update`.
pub async fn run_entry(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
    cmd: ContentCmd,
) -> Result<()> {
    match cmd {
        ContentCmd::Add {
            item,
            version,
            file,
            filename,
            source,
            world,
        } => {
            add(
                client, entry, kind, reference, item, version, file, filename, source, world,
            )
            .await
        }
        ContentCmd::List => list(client, entry, kind, reference).await,
        ContentCmd::Remove { items, world } => {
            remove(client, entry, kind, reference, items, world).await
        }
        ContentCmd::Update { items, check, file } => {
            if check {
                check_updates(client, entry, kind, reference).await
            } else {
                update(client, entry, kind, reference, items, file).await
            }
        }
        ContentCmd::Enable { item, world } => {
            set_enabled(client, entry, kind, reference, item, world, true).await
        }
        ContentCmd::Disable { item, world } => {
            set_enabled(client, entry, kind, reference, item, world, false).await
        }
        ContentCmd::SetVersion {
            item,
            version,
            source,
        } => {
            let source = source.unwrap_or_default();
            set_version(client, entry, kind, reference, item, version, source).await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn add(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
    item: Option<String>,
    version: Option<String>,
    file: Option<String>,
    filename: Option<String>,
    source: Option<String>,
    world: Vec<String>,
) -> Result<()> {
    let info = resolve_entry(client, entry, reference).await?;
    let source = source.unwrap_or_default();

    if item.is_none() && file.is_none() {
        if !ui::interactive_output() {
            bail!("name an item to install (a project slug/id, a source URL, or --file)");
        }
        return add_session(client, entry, kind, info, source).await;
    }

    let mut add_item = ContentAddItem {
        filename: filename.unwrap_or_default(),
        version: version.unwrap_or_default(),
        ..ContentAddItem::default()
    };
    if let Some(path) = file {
        add_item.path = std::fs::canonicalize(&path)
            .with_context(|| format!("cannot read {path}"))?
            .to_string_lossy()
            .into_owned();
    } else {
        let reference = item.unwrap_or_default();
        if is_url(&reference) {
            add_item.url = reference;
        } else {
            add_item.project = reference;
        }
    }

    let worlds = if kind == ContentKind::DataPack && matches!(entry, EntryKind::Instance) {
        pick_worlds(client, &info.id, world).await?
    } else {
        Vec::new()
    };

    let spec = ContentAddSpec {
        kind,
        source,
        items: vec![add_item],
        worlds,
    };
    let target = EntryTarget::new(entry, info.id.clone());
    let (installed, failures) = install_spec(client, &target, spec).await?;
    show_install_report(&info.name, &installed, &[], &failures)
}

/// The interactive install path: the fullscreen search → review → install
/// session, seeded with the entry's own loader and game version as fixed
/// search filters.
async fn add_session(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    info: EntryInfo,
    source: String,
) -> Result<()> {
    let worlds = if kind == ContentKind::DataPack && matches!(entry, EntryKind::Instance) {
        let worlds = client.instance().worlds(&info.id).await?;
        if worlds.is_empty() {
            bail!("no save worlds yet — launch the instance and create a world first");
        }
        // The session installs by folder; its picker only needs the identity.
        worlds.into_iter().map(|w| w.folder).collect()
    } else {
        Vec::new()
    };
    let target = EntryTarget::new(entry, info.id.clone());
    let (installed, _) = client.content().list(&target, kind).await?;
    let base = SearchQuery {
        kind,
        source,
        loader: matches!(kind, ContentKind::Mod | ContentKind::Plugin).then(|| info.flavor.clone()),
        game_version: Some(info.game_version.clone()),
        limit: PAGE,
        ..SearchQuery::default()
    };
    let target = session::Target {
        entry: target,
        name: info.name.clone(),
        worlds,
        installed,
    };
    match session::run(client, base, Some(target)).await? {
        None => ui::show(View::note("cancelled")),
        Some(report) => {
            if let Some(error) = report.error {
                bail!(error);
            }
            show_install_report(&info.name, &report.items, &report.removed, &report.failures)
        }
    }
}

/// Which worlds an installed datapack loads in, as a trailing clause; empty for
/// every other kind.
fn worlds_suffix(item: &InstalledContent) -> String {
    match item.kind == ContentKind::DataPack {
        true => format!(" ({})", world_label(item)),
        false => String::new(),
    }
}

/// Print what a batch installed, removed, and failed; errors (a non-zero
/// exit) when nothing landed at all.
pub(super) fn show_install_report(
    name: &str,
    installed: &[InstalledContent],
    removed: &[InstalledContent],
    failures: &[ContentFailure],
) -> Result<()> {
    for content in installed {
        ui::show(View::line(format!(
            "installed {} ({}) into '{name}'{}",
            content.title,
            content.filename,
            worlds_suffix(content)
        )))?;
    }
    for content in removed {
        ui::show(View::line(format!(
            "removed {} ({}) from '{name}'",
            content.title, content.filename
        )))?;
    }
    show_failures(failures)?;
    if installed.is_empty() && removed.is_empty() {
        match failures.is_empty() {
            true => ui::show(View::note("no changes applied")),
            false => bail!("no changes applied"),
        }
    } else {
        Ok(())
    }
}

fn show_failures(failures: &[ContentFailure]) -> Result<()> {
    for failure in failures {
        let label = if failure.title.is_empty() {
            failure.item.clone()
        } else {
            failure.title.clone()
        };
        ui::show(View::note(format!("failed {label}: {}", failure.error)))?;
    }
    Ok(())
}

pub(super) async fn install_spec(
    client: &Client,
    target: &EntryTarget,
    spec: ContentAddSpec,
) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>)> {
    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let result = client
        .content()
        .add(target, spec, move |p| progress.update(p))
        .await;
    reporter.finish();
    Ok(result?)
}

async fn list(client: &Client, entry: EntryKind, kind: ContentKind, reference: &str) -> Result<()> {
    let EntryInfo { id, name, .. } = resolve_entry(client, entry, reference).await?;
    let (items, untracked) = client
        .content()
        .list(&EntryTarget::new(entry, id), kind)
        .await?;
    if items.is_empty() && untracked.is_empty() {
        return ui::show(View::note(format!("no {} installed", kind_plural(kind))));
    }
    if !items.is_empty() {
        if kind == ContentKind::DataPack {
            let rows = items
                .iter()
                .map(|i| {
                    vec![
                        i.title.clone(),
                        i.version_number.clone(),
                        world_label(i),
                        source_label(i),
                    ]
                })
                .collect();
            ui::show(View::table(
                format!("{name} {}", kind_plural(kind)),
                ["NAME", "VERSION", "WORLD", "SOURCE"],
                rows,
            ))?;
        } else {
            let rows = items
                .iter()
                .map(|i| vec![i.title.clone(), i.version_number.clone(), source_label(i)])
                .collect();
            ui::show(View::table(
                format!("{name} {}", kind_plural(kind)),
                ["NAME", "VERSION", "SOURCE"],
                rows,
            ))?;
        }
    }
    if !untracked.is_empty() {
        ui::show(View::note(format!(
            "{} untracked file(s) in the game dir: {}",
            untracked.len(),
            untracked
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )))?;
    }
    Ok(())
}

async fn remove(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
    items: Vec<String>,
    world: Vec<String>,
) -> Result<()> {
    let EntryInfo { id, name, .. } = resolve_entry(client, entry, reference).await?;
    let target = EntryTarget::new(entry, id);
    let worlds: Vec<String> = world.into_iter().filter(|w| !w.is_empty()).collect();
    let items = match items.is_empty() {
        false => items,
        true => {
            let (installed, _) = client.content().list(&target, kind).await?;
            vec![pick_installed(installed)?]
        }
    };
    client
        .content()
        .remove(&target, kind, &items, &worlds)
        .await?;
    let where_ = match worlds.is_empty() {
        true => format!("'{name}'"),
        false => format!("'{name}' ({})", worlds.join(", ")),
    };
    for item in &items {
        ui::show(View::line(format!("removed '{item}' from {where_}")))?;
    }
    Ok(())
}

async fn update(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
    items: Vec<String>,
    file: Option<String>,
) -> Result<()> {
    let path = match file {
        Some(file) if items.len() != 1 => {
            bail!("--file updates exactly one item; name the one to update from {file}")
        }
        Some(file) => std::fs::canonicalize(&file)
            .with_context(|| format!("cannot read {file}"))?
            .to_string_lossy()
            .into_owned(),
        None => String::new(),
    };
    let EntryInfo { id, name, .. } = resolve_entry(client, entry, reference).await?;
    let target = EntryTarget::new(entry, id);

    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let (updated, failures) = {
        let result = client
            .content()
            .update(&target, kind, &items, &path, move |p| progress.update(p))
            .await;
        reporter.finish();
        result?
    };

    for content in &updated {
        let line = match path.is_empty() {
            true => format!("updated {} to {}", content.title, content.version_number),
            false => format!("updated {} to {}", items[0], content.filename),
        };
        ui::show(View::line(line))?;
    }
    show_failures(&failures)?;
    match (updated.is_empty(), failures.is_empty()) {
        (true, true) => ui::show(View::note(format!("'{name}' is already up to date"))),
        (true, false) => bail!("no changes applied"),
        (false, _) => Ok(()),
    }
}

async fn check_updates(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
) -> Result<()> {
    let EntryInfo { id, name, .. } = resolve_entry(client, entry, reference).await?;
    let updates = client
        .content()
        .check_updates(&EntryTarget::new(entry, id), kind)
        .await?;
    let outdated: Vec<_> = updates.iter().filter(|u| u.updatable).collect();
    if outdated.is_empty() {
        return ui::show(View::note(format!("'{name}' is up to date")));
    }
    let rows = outdated
        .iter()
        .map(|u| {
            vec![
                u.filename.clone(),
                u.current_version_number.clone(),
                u.latest_version_number.clone(),
            ]
        })
        .collect();
    ui::show(View::table(
        format!("{name} {} with updates", kind_plural(kind)),
        ["FILE", "CURRENT", "LATEST"],
        rows,
    ))
}

async fn set_enabled(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
    item: Option<String>,
    world: Vec<String>,
    enabled: bool,
) -> Result<()> {
    let EntryInfo { id, name, .. } = resolve_entry(client, entry, reference).await?;
    let target = EntryTarget::new(entry, id);
    let worlds: Vec<String> = world.into_iter().filter(|w| !w.is_empty()).collect();
    let item = match item {
        Some(item) => item,
        None => {
            let (items, _) = client.content().list(&target, kind).await?;
            pick_installed(items)?
        }
    };
    client
        .content()
        .enable(&target, kind, &item, enabled, &worlds)
        .await?;
    let verb = if enabled { "enabled" } else { "disabled" };
    let where_ = match worlds.is_empty() {
        true => format!("'{name}'"),
        false => format!("'{name}' ({})", worlds.join(", ")),
    };
    ui::show(View::line(format!("{verb} '{item}' in {where_}")))
}

async fn set_version(
    client: &Client,
    entry: EntryKind,
    kind: ContentKind,
    reference: &str,
    item: String,
    version: String,
    source: String,
) -> Result<()> {
    let EntryInfo { id, name, .. } = resolve_entry(client, entry, reference).await?;
    let target = EntryTarget::new(entry, id);

    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let updated = {
        let result = client
            .content()
            .set_version(&target, kind, &item, &version, &source, move |p| {
                progress.update(p)
            })
            .await;
        reporter.finish();
        result?
    };

    if updated.is_empty() {
        return ui::show(View::note(format!("'{item}' is already at that version")));
    }
    for content in &updated {
        ui::show(View::line(format!(
            "pinned {} to {} in '{name}'",
            content.title, content.version_number
        )))?;
    }
    Ok(())
}

fn pick_installed(items: Vec<InstalledContent>) -> Result<String> {
    if items.is_empty() {
        bail!("nothing is installed");
    }
    let labels: Vec<String> = items
        .iter()
        .map(|i| format!("{} ({})", i.title, i.version_number))
        .collect();
    let index = ui::select("select an item", &labels)?;
    let chosen = &items[index];
    Ok(if chosen.project_id.is_empty() {
        chosen.filename.clone()
    } else {
        chosen.project_id.clone()
    })
}

/// Choose the save world(s) for an instance datapack: the given `--world`
/// flags when present, otherwise an interactive multi-select over the
/// instance's worlds (which errors when stdin is not a terminal, so scripts
/// must pass `--world`).
async fn pick_worlds(client: &Client, id: &str, world: Vec<String>) -> Result<Vec<String>> {
    let flags: Vec<String> = world.into_iter().filter(|w| !w.is_empty()).collect();
    if !flags.is_empty() {
        return Ok(flags);
    }
    let worlds = client.instance().worlds(id).await?;
    if worlds.is_empty() {
        bail!("no save worlds yet — launch the instance and create a world first");
    }
    // Labelled by what the player named the world, but installed by folder —
    // the identity the game and the index both key on.
    let labels: Vec<String> = worlds.iter().map(save_label).collect();
    let picks = ui::multi_select("select world(s)", &labels)
        .context("pass --world to name the world(s)")?;
    Ok(picks
        .into_iter()
        .map(|i| worlds[i].folder.clone())
        .collect())
}

/// A save world for a picker line: its in-game name, plus the folder when they
/// differ (the folder is what gets installed into).
fn save_label(world: &WorldInfo) -> String {
    if world.name == world.folder {
        world.folder.clone()
    } else {
        format!("{} ({})", world.name, world.folder)
    }
}

/// What a content command needs to know about its entry.
pub(super) struct EntryInfo {
    pub id: String,
    pub name: String,
    pub flavor: String,
    pub game_version: String,
}

/// Resolve a known server/instance reference to its record essentials.
async fn resolve_entry(client: &Client, entry: EntryKind, reference: &str) -> Result<EntryInfo> {
    let entries: Vec<EntryInfo> = match entry {
        EntryKind::Server => client
            .server()
            .list()
            .await?
            .into_iter()
            .map(|s| EntryInfo {
                id: s.id,
                name: s.name,
                flavor: s.flavor,
                game_version: s.game_version,
            })
            .collect(),
        EntryKind::Instance => client
            .instance()
            .list()
            .await?
            .into_iter()
            .map(|i| EntryInfo {
                id: i.id,
                name: i.name,
                flavor: i.flavor,
                game_version: i.game_version,
            })
            .collect(),
    };
    entries
        .into_iter()
        .find(|e| client::proto::naming::reference_matches(reference, &e.id, &e.name))
        .with_context(|| format!("no {entry} matches '{reference}'"))
}

fn is_url(reference: &str) -> bool {
    reference.starts_with("http://") || reference.starts_with("https://")
}
