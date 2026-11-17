//! `hestia plugin dev`: a project installed as a dev install, rebuilt and
//! reloaded whenever its sources change, with the plugin's log alongside.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Args;
use client::Client;
use notify::{RecursiveMode, Watcher};
use proto::plugin::{Choice, Grant, PluginSource};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::super::{approve, choose, show_plan, state_name, ChoiceArgs};
use super::project::{build, COMPONENT, LOCK, MANIFEST};
use crate::ui::{self, View};

const SETTLE: Duration = Duration::from_millis(300);
const LOG_POLL: Duration = Duration::from_millis(250);

#[derive(Args)]
pub struct DevArgs {
    /// The project directory
    #[arg(default_value = ".")]
    dir: PathBuf,
    #[command(flatten)]
    choice: ChoiceArgs,
    /// Approve what it asks for without prompting
    #[arg(long)]
    yes: bool,
}

/// What the last load approved, so a reload that asks for no more reuses it.
#[derive(Default)]
struct Approved {
    grant: Option<Grant>,
    choice: Option<Choice>,
}

/// Whether a change at `path` is one that calls for a rebuild: not the build's
/// own output, and not version control or the target directory.
fn relevant(path: &Path) -> bool {
    let ignored_dir = path
        .components()
        .any(|part| matches!(part.as_os_str().to_str(), Some("target" | ".git")));
    let own_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [COMPONENT, LOCK].contains(&name) || name.ends_with(".hplugin") || name.ends_with('~')
        });
    !ignored_dir && !own_file
}

fn watch(dir: &Path) -> Result<(notify::RecommendedWatcher, UnboundedReceiver<()>)> {
    let (sender, receiver) = unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if !event.kind.is_access() && event.paths.iter().any(|path| relevant(path)) {
                let _ = sender.send(());
            }
        }
    })
    .context("cannot watch for changes")?;
    watcher
        .watch(dir, RecursiveMode::Recursive)
        .with_context(|| format!("cannot watch {}", dir.display()))?;
    Ok((watcher, receiver))
}

/// The daemon's log lines of `id`, as they are written: a plugin's log goes to
/// the daemon's log file, not over the socket.
async fn follow_log(id: String) {
    let path = common::paths::log_dir(None)
        .join("hestiad")
        .join("latest.log");
    let needle = format!("plugin={id}");
    let mut offset = std::fs::metadata(&path).map_or(0, |meta| meta.len());
    loop {
        tokio::time::sleep(LOG_POLL).await;
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };
        let length = file.metadata().map_or(0, |meta| meta.len());
        if length < offset {
            offset = 0;
        }
        let mut text = String::new();
        if file.seek(SeekFrom::Start(offset)).is_err() || file.read_to_string(&mut text).is_err() {
            continue;
        }
        let complete = text.rfind('\n').map_or(0, |end| end + 1);
        offset += complete as u64;
        for line in text[..complete].lines() {
            if line.contains(&needle) && line.contains(" plugin: ") {
                let _ = ui::show(View::line(line));
            }
        }
    }
}

async fn reload(
    client: &Client,
    dir: &Path,
    args: &DevArgs,
    approved: &mut Approved,
) -> Result<()> {
    let project = dir.to_owned();
    tokio::task::spawn_blocking(move || build(&project))
        .await
        .context("the build stopped")??;
    let path = dir.to_string_lossy().into_owned();
    let inspection = client.plugin().inspect(&path).await?;
    let id = inspection.id.clone();
    let installed = client
        .plugin()
        .list()
        .await?
        .into_iter()
        .find(|plugin| plugin.id == id);
    if installed
        .as_ref()
        .is_some_and(|p| p.source != PluginSource::Dev)
    {
        bail!("{id} is installed from a repository; remove it first");
    }
    let unchanged = approved
        .grant
        .as_ref()
        .is_some_and(|before| !kernel::widens(before, &inspection.grant));
    let choice = match (&approved.choice, unchanged) {
        (Some(choice), true) => choice.clone(),
        _ => {
            show_plan(&inspection)?;
            approve("Install it with these permissions?", args.yes)?;
            choose(client, &inspection.grant, args.choice.clone(), args.yes).await?
        }
    };
    if installed.is_some() {
        client.plugin().remove(&id).await?;
    }
    let plugin = client
        .plugin()
        .install(&path, &inspection.approval_digest, choice.clone())
        .await?;
    ui::show(View::line(format!(
        "loaded {} ({})",
        plugin.id,
        state_name(plugin.state)
    )))?;
    *approved = Approved {
        grant: Some(inspection.grant),
        choice: Some(choice),
    };
    Ok(())
}

pub async fn run(args: DevArgs) -> Result<()> {
    let dir = std::fs::canonicalize(&args.dir)
        .with_context(|| format!("cannot read {}", args.dir.display()))?;
    let manifest = std::fs::read_to_string(dir.join(MANIFEST))
        .with_context(|| format!("cannot read {MANIFEST} in {}", dir.display()))?;
    let id = kernel::Manifest::parse(&manifest)?.id;
    let client = super::super::super::connect().await?;
    let (_watcher, mut changes) = watch(&dir)?;
    let log = tokio::spawn(follow_log(id));
    ui::show(View::note(format!(
        "watching {}; press Ctrl-C to stop",
        dir.display()
    )))?;
    let mut approved = Approved::default();
    loop {
        if let Err(error) = reload(&client, &dir, &args, &mut approved).await {
            ui::show(View::warning(format!("{error:#}")))?;
        }
        while changes.try_recv().is_ok() {}
        tokio::select! {
            change = changes.recv() => {
                if change.is_none() {
                    break;
                }
                tokio::time::sleep(SETTLE).await;
                while changes.try_recv().is_ok() {}
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    log.abort();
    Ok(())
}
