//! `hestia modpack …` — browse a source's modpacks, install one into a new or
//! existing entry, and drive the pack an entry already runs.
//!
//! Installing is the hot path and reads as one line: `hestia modpack install
//! <pack>` builds a whole instance from it, because a pack pins its own loader
//! and game version and so knows what entry it wants. Everything it needs but
//! was not given is asked for on a terminal — the pack itself through a
//! searchable picker over live search results — and piped invocations error
//! naming the flag, so scripts stay explicit.
//!
//! What an entry does with the pack it has (`status`, `update`, `remove`) is
//! entry-first like every other per-entry verb: `hestia instance <name> modpack
//! update`.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::content::{ContentKind, SearchQuery, SearchSort};
use client::proto::modpack::{
    InstalledModpack, ModpackDoneEvent, ModpackMove, ModpackRef, ModpackTarget,
};
use client::Client;

use super::content::{BrowseCmd, EntryKind};
use super::{connect, content};
use crate::ui::components::PickerItem;
use crate::ui::{self, ProvisionReporter, Spinner, View};

/// `hestia modpack <search|info|versions|install>`. The first three are the
/// shared browse grammar every content kind has; `install` is the modpack-only
/// verb, since a pack is the one kind that produces an entry rather than going
/// into one.
#[derive(Subcommand)]
pub enum ModpackCmd {
    /// Install a modpack into a new (or, with --into, an existing) entry
    Install(InstallArgs),
    #[command(flatten)]
    Browse(BrowseCmd),
}

#[derive(clap::Args, Default)]
pub struct InstallArgs {
    /// Project slug or id, a modrinth.com URL, or a path to a .mrpack file
    /// (searches interactively when omitted)
    pub pack: Option<String>,
    #[arg(short = 'V', long, help = "Pin a pack version (id or number)")]
    pub version: Option<String>,
    #[arg(
        long,
        conflicts_with = "into",
        help = "Build a server from the pack instead of an instance"
    )]
    pub server: bool,
    #[arg(
        long,
        help = "Install into an existing server or instance instead of creating one"
    )]
    pub into: Option<String>,
    #[arg(long, help = "Name the new entry (defaults to the pack's own name)")]
    pub name: Option<String>,
    #[arg(
        short = 'S',
        long,
        help = "Content source to resolve the pack on (default: the first configured one)"
    )]
    pub source: Option<String>,
    #[arg(long, help = "Accept the Minecraft EULA (creating a server)")]
    pub eula: bool,
    #[arg(long, help = "Pin the new server's game port")]
    pub port: Option<u16>,
}

/// The per-entry actions, reached as `hestia <server|instance> <name> modpack …`.
#[derive(Subcommand)]
pub enum EntryModpackCmd {
    /// The modpack this entry runs
    Status,
    /// Move the pack to another published version, or to a pack file
    Update {
        /// Pack version to move to (id or number; newest when omitted), or a
        /// path to a pack file — the only way a pack no source published updates
        version: Option<String>,
        #[arg(
            long,
            help = "Allow moving to a pack built for an older game version (saves do not downgrade)"
        )]
        downgrade: bool,
        #[arg(
            long,
            help = "Accept a pack file holding another pack than the entry runs"
        )]
        other_pack: bool,
        #[arg(
            long,
            conflicts_with_all = ["version", "downgrade", "other_pack"],
            help = "Only report whether a newer pack version exists, without applying"
        )]
        check: bool,
    },
    /// Take the pack out, keeping its content or removing it (asks when neither flag is given)
    #[command(visible_alias = "rm")]
    Remove {
        #[arg(
            long,
            conflicts_with = "all",
            help = "Keep everything the pack installed as your own content"
        )]
        keep: bool,
        #[arg(
            long,
            help = "Remove the pack's content too, except files you have edited"
        )]
        all: bool,
    },
}

pub async fn run(cmd: ModpackCmd) -> Result<()> {
    match cmd {
        ModpackCmd::Install(args) => {
            let client = connect().await?;
            install(&client, args).await
        }
        ModpackCmd::Browse(cmd) => content::run_browse(ContentKind::Modpack, cmd).await,
    }
}

pub async fn install(client: &Client, args: InstallArgs) -> Result<()> {
    let pack = pack_ref(client, &args).await?;
    let target = match &args.into {
        Some(entry) => ModpackTarget::Existing {
            entry: entry.clone(),
        },
        None => ModpackTarget::Create {
            name: args.name.clone().unwrap_or_default(),
        },
    };
    let to_server =
        args.server || matches!(&args.into, Some(entry) if is_server(client, entry).await?);

    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let done = match to_server {
        true => {
            let eula = args.eula || confirm_eula(&args)?;
            client
                .modpack()
                .install_server(pack, target, eula, args.port, move |p| progress.update(p))
                .await
        }
        false => {
            client
                .modpack()
                .install_instance(pack, target, move |p| progress.update(p))
                .await
        }
    };
    reporter.finish();
    report(done?, to_server)
}

pub async fn run_entry(entry: EntryKind, name: String, cmd: EntryModpackCmd) -> Result<()> {
    let client = connect().await?;
    match cmd {
        EntryModpackCmd::Status => status(&client, entry, &name).await,
        EntryModpackCmd::Update { check: true, .. } => check_update(&client, entry, &name).await,
        EntryModpackCmd::Update {
            version,
            downgrade,
            other_pack,
            ..
        } => {
            let to = ModpackMove {
                allow_downgrade: downgrade,
                allow_other_pack: other_pack,
                ..ModpackMove::default()
            };
            let to = match version.as_deref().and_then(pack_file) {
                Some(path) => confirm_file_update(&client, entry, &name, path?, to).await?,
                None => ModpackMove {
                    version: version.unwrap_or_default(),
                    ..to
                },
            };
            update(&client, entry, &name, to).await
        }
        EntryModpackCmd::Remove { keep, all } => {
            let keep_content = keep || (!all && ask_keep_content(&name)?);
            remove(&client, entry, &name, keep_content).await
        }
    }
}

async fn status(client: &Client, entry: EntryKind, name: &str) -> Result<()> {
    let pack = match entry {
        EntryKind::Server => client.modpack().server_status(name).await?,
        EntryKind::Instance => client.modpack().instance_status(name).await?,
    };
    let Some(pack) = pack else {
        return ui::show(View::note(format!("'{name}' was not built from a modpack")));
    };
    show_pack(&pack)
}

async fn check_update(client: &Client, entry: EntryKind, name: &str) -> Result<()> {
    let update = match entry {
        EntryKind::Server => client.modpack().server_check_update(name).await?,
        EntryKind::Instance => client.modpack().instance_check_update(name).await?,
    };
    let Some(update) = update else {
        return ui::show(View::note(format!(
            "'{name}' runs no pack that can be checked"
        )));
    };
    let fitting = update
        .fitting
        .filter(|fit| fit.version_id != update.latest_version_id);
    if !update.updatable && fitting.is_none() {
        return ui::show(View::note(format!(
            "'{name}' is on the newest pack version ({})",
            update.latest_version_number
        )));
    }
    let mut rows = vec![
        ("current".to_string(), update.current_version_number),
        ("latest".to_string(), update.latest_version_number),
    ];
    if let Some(fit) = fitting {
        rows.push((format!("for {}", fit.game_version), fit.version_number));
    }
    ui::show(View::detail(rows))
}

/// Ask before a pack file does what its flags did not allow: replace the pack
/// with another one, or take the entry back a game version. Errors when stdin
/// is not a terminal, so a script must pass the flag.
async fn confirm_file_update(
    client: &Client,
    entry: EntryKind,
    name: &str,
    path: String,
    to: ModpackMove,
) -> Result<ModpackMove> {
    let server = matches!(entry, EntryKind::Server);
    let (pack, preview) = match server {
        true => (
            client.modpack().server_status(name).await?,
            client.modpack().preview_server_update(name, &path).await?,
        ),
        false => (
            client.modpack().instance_status(name).await?,
            client
                .modpack()
                .preview_instance_update(name, &path)
                .await?,
        ),
    };
    let pack = pack.with_context(|| format!("'{name}' was not built from a modpack"))?;
    if !preview.same_pack && !to.allow_other_pack {
        let replace = ui::confirm(
            &format!(
                "that file is the pack '{}', not '{}' which '{name}' runs",
                preview.name, pack.name
            ),
            "replace the pack",
            "cancel",
        )
        .context("pass --other-pack to replace the pack with another one")?;
        if !replace {
            bail!("update cancelled");
        }
    }
    if preview.downgrade && !to.allow_downgrade {
        let (data, safety) = match server {
            true => (
                "a world",
                "the current data is backed up automatically first",
            ),
            false => (
                "saves",
                "nothing is backed up first (instances have no backups)",
            ),
        };
        super::mc::confirm_downgrade(
            name,
            data,
            safety,
            &pack.game_version,
            &preview.game_version,
        )?;
    }
    Ok(ModpackMove {
        path,
        allow_downgrade: true,
        allow_other_pack: true,
        ..to
    })
}

/// A positional that names a file on disk, as its absolute path — the same
/// rule `install` reads its own argument by.
fn pack_file(given: &str) -> Option<Result<String>> {
    std::path::Path::new(given).is_file().then(|| {
        Ok(std::fs::canonicalize(given)
            .with_context(|| format!("cannot resolve {given}"))?
            .to_string_lossy()
            .into_owned())
    })
}

async fn update(client: &Client, entry: EntryKind, name: &str, to: ModpackMove) -> Result<()> {
    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let done = match entry {
        EntryKind::Server => {
            client
                .modpack()
                .update_server(name, to, move |p| progress.update(p))
                .await
        }
        EntryKind::Instance => {
            client
                .modpack()
                .update_instance(name, to, move |p| progress.update(p))
                .await
        }
    };
    reporter.finish();
    report(done?, matches!(entry, EntryKind::Server))
}

/// Interactive fallback for a `remove` given neither `--keep` nor `--all`;
/// errors when stdin is not a terminal so scripts must say which they mean.
fn ask_keep_content(name: &str) -> Result<bool> {
    let picked = ui::select_detailed(
        &format!("Remove the modpack from '{name}'"),
        &["Keep its content".to_string(), "Remove it all".to_string()],
        &[
            "the pack is forgotten; its mods and files stay as your own".to_string(),
            "its mods and files are deleted, except files you have edited".to_string(),
        ],
    )
    .context("pass --keep to keep the pack's content or --all to remove it")?;
    Ok(picked == 0)
}

async fn remove(client: &Client, entry: EntryKind, name: &str, keep_content: bool) -> Result<()> {
    let result = match entry {
        EntryKind::Server => client.modpack().remove_server(name, keep_content).await?,
        EntryKind::Instance => client.modpack().remove_instance(name, keep_content).await?,
    };
    if keep_content {
        return ui::show(View::line(format!(
            "removed the pack from '{name}'; {} file(s) stay as your own content",
            result.released_files
        )));
    }
    ui::show(View::line(format!(
        "removed {} file(s) and {} game-directory file(s) from '{name}'",
        result.removed_files, result.removed_overrides
    )))?;
    if !result.kept.is_empty() {
        ui::show(View::note(format!(
            "kept {} file(s) you had edited: {}",
            result.kept.len(),
            result.kept.join(", ")
        )))?;
    }
    Ok(())
}

/// Turn what the caller gave into a reference the daemon takes. A path that
/// exists is a local `.mrpack`, anything with a scheme is a URL, and the rest
/// is a project on the chosen source — so the one positional argument covers
/// all three without the caller picking a flag for it.
async fn pack_ref(client: &Client, args: &InstallArgs) -> Result<ModpackRef> {
    let source = args.source.clone().unwrap_or_default();
    let version = args.version.clone().unwrap_or_default();
    let Some(given) = args.pack.clone() else {
        let project = search_for_pack(client, &source).await?;
        return Ok(ModpackRef {
            source,
            project,
            version,
            ..ModpackRef::default()
        });
    };
    if given.starts_with("http://") || given.starts_with("https://") {
        return Ok(ModpackRef {
            source,
            url: given,
            version,
            ..ModpackRef::default()
        });
    }
    if let Some(path) = pack_file(&given) {
        return Ok(ModpackRef {
            path: path?,
            ..ModpackRef::default()
        });
    }
    Ok(ModpackRef {
        source,
        project: given,
        version,
        ..ModpackRef::default()
    })
}

/// Ask what to install: a query, then a searchable picker over the hits. The
/// picker shows downloads and the blurb, because a pack's name alone rarely
/// says whether it is the one you meant.
async fn search_for_pack(client: &Client, source: &str) -> Result<String> {
    if !ui::interactive_output() {
        bail!("name a modpack to install (a slug, a URL, or a .mrpack path)");
    }
    let query = ui::input("search modpacks", "")?;
    let hits = {
        let _spinner = Spinner::start("searching");
        client
            .content()
            .search(&SearchQuery {
                source: source.to_string(),
                kind: ContentKind::Modpack,
                query,
                sort: SearchSort::Downloads,
                limit: 50,
                ..SearchQuery::default()
            })
            .await?
            .hits
    };
    if hits.is_empty() {
        bail!("no modpacks matched");
    }
    let items = hits
        .iter()
        .map(|h| PickerItem {
            label: h.title.clone(),
            tag: format!("{} · {}", compact(h.downloads), h.description),
            stable: true,
        })
        .collect();
    let picked = ui::pick("modpack", items)?;
    Ok(hits[picked].id.clone())
}

/// Creating a server needs the EULA asserted. On a terminal that is a question;
/// piped it is a refusal naming the flag, so a script cannot accept it by
/// accident.
fn confirm_eula(args: &InstallArgs) -> Result<bool> {
    if args.into.is_some() {
        return Ok(false);
    }
    ui::confirm(
        "Do you accept the Minecraft EULA (https://aka.ms/MinecraftEULA)?",
        "accept",
        "cancel",
    )
    .context("creating a server requires accepting the EULA (pass --eula)")
}

async fn is_server(client: &Client, reference: &str) -> Result<bool> {
    Ok(client
        .server()
        .list()
        .await?
        .iter()
        .any(|s| client::proto::naming::reference_matches(reference, &s.id, &s.name)))
}

fn report(done: ModpackDoneEvent, to_server: bool) -> Result<()> {
    let noun = if to_server { "server" } else { "instance" };
    ui::show(View::line(format!(
        "'{}' installed into {noun} '{}'",
        done.pack.name, done.entry_name
    )))?;
    show_pack(&done.pack)?;
    for failure in &done.failures {
        ui::show(View::note(format!(
            "could not install {}: {}",
            failure.item, failure.error
        )))?;
    }
    ui::show_warnings(&done.warnings)
}

fn show_pack(pack: &InstalledModpack) -> Result<()> {
    let loader = match pack.loader.is_empty() {
        true => "vanilla".to_string(),
        false => match pack.loader_version.is_empty() {
            true => pack.loader.clone(),
            false => format!("{} {}", pack.loader, pack.loader_version),
        },
    };
    ui::show(View::detail([
        ("modpack", pack.name.clone()),
        ("version", pack.version_number.clone()),
        ("game", pack.game_version.clone()),
        ("loader", loader),
        ("source", pack.source.clone()),
        ("content", pack.files.len().to_string()),
        ("pack files", pack.overrides.len().to_string()),
    ]))
}

/// `12.3k` — the same compaction the browse tables use for download counts.
fn compact(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}
