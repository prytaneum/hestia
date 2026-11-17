//! `hestia plugin dev`: a project installed as a dev install, rebuilt and
//! reloaded whenever its sources change, with the plugin's log alongside.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Args;
use client::Client;
use notify::{RecursiveMode, Watcher};
use proto::plugin::{Choice, Grant, PluginSource};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::super::{approve, choose, show_plan, state_name, ChoiceArgs};
use super::log;
use super::project::{build, Profile, COMPONENT, LOCK, MANIFEST};
use crate::commands::connect;
use crate::ui::{self, View};

const SETTLE: Duration = Duration::from_millis(300);
const IGNORED_DIRS: [&str; 2] = ["target", ".git"];

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

/// Whether a change at `path` in `dir` is one that calls for a rebuild: not the
/// build's own output, and not version control or the target directory.
fn relevant(dir: &Path, path: &Path) -> bool {
    let inside = path.strip_prefix(dir).unwrap_or(path);
    let ignored_dir = inside.components().any(|part| {
        IGNORED_DIRS
            .iter()
            .any(|ignored| part.as_os_str() == *ignored)
    });
    let own_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [COMPONENT, LOCK, "Cargo.lock"].contains(&name)
                || name.ends_with(".hplugin")
                || name.ends_with('~')
        });
    !ignored_dir && !own_file
}

/// Watches the project's own files and each directory in it but the ignored
/// ones, so the build's output under `target` costs no watch. A directory
/// created later is not watched until `dev` restarts.
fn watch(dir: &Path) -> Result<(notify::RecommendedWatcher, UnboundedReceiver<()>)> {
    let (sender, receiver) = unbounded_channel();
    let root = dir.to_owned();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if !event.kind.is_access() && event.paths.iter().any(|path| relevant(&root, path)) {
                let _ = sender.send(());
            }
        }
    })
    .context("cannot watch for changes")?;
    let watching = |error| format!("cannot watch {}: {error}", dir.display());
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|error| anyhow::anyhow!(watching(error)))?;
    for entry in std::fs::read_dir(dir)?.flatten() {
        let ignored = entry
            .file_name()
            .to_str()
            .is_none_or(|name| IGNORED_DIRS.contains(&name) || name.starts_with('.'));
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) && !ignored {
            watcher
                .watch(&entry.path(), RecursiveMode::Recursive)
                .map_err(|error| anyhow::anyhow!(watching(error)))?;
        }
    }
    Ok((watcher, receiver))
}

async fn reload(
    client: &Client,
    dir: &Path,
    args: &DevArgs,
    approved: &mut Option<Grant>,
) -> Result<()> {
    let project = dir.to_owned();
    tokio::task::spawn_blocking(move || build(&project, Profile::Dev))
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
    let asks_again = approved
        .as_ref()
        .is_none_or(|before| kernel::widens(before, &inspection.grant));
    if asks_again {
        show_plan(&inspection)?;
        approve("Install it with these permissions?", args.yes)?;
    }
    let choice = match installed {
        Some(_) => Choice::default(),
        None => choose(client, &inspection.grant, args.choice.clone(), args.yes).await?,
    };
    let plugin = client
        .plugin()
        .install(&path, &inspection.approval_digest, choice)
        .await?;
    ui::show(View::line(format!(
        "loaded {} ({})",
        plugin.id,
        state_name(plugin.state)
    )))?;
    *approved = Some(inspection.grant);
    Ok(())
}

async fn watch_loop(client: Client, dir: PathBuf, args: DevArgs) -> Result<()> {
    let (_watcher, mut changes) = watch(&dir)?;
    let mut approved = None;
    loop {
        if let Err(error) = reload(&client, &dir, &args, &mut approved).await {
            ui::show(View::warning(format!("{error:#}")))?;
        }
        if changes.recv().await.is_none() {
            return Ok(());
        }
        tokio::time::sleep(SETTLE).await;
        while changes.try_recv().is_ok() {}
    }
}

pub async fn run(args: DevArgs) -> Result<()> {
    let dir = std::fs::canonicalize(&args.dir)
        .with_context(|| format!("cannot read {}", args.dir.display()))?;
    let manifest = std::fs::read_to_string(dir.join(MANIFEST))
        .with_context(|| format!("cannot read {MANIFEST} in {}", dir.display()))?;
    let id = kernel::Manifest::parse(&manifest)?.id;
    let client = connect().await?;
    let developer = client.config().get("plugins.developer").await?;
    if developer != Some(serde_json::Value::Bool(true)) {
        bail!("a dev install needs developer mode: `hestia config set plugins.developer true`");
    }
    let log = tokio::spawn(log::follow(id));
    ui::show(View::note(format!(
        "watching {}; press Ctrl-C to stop",
        dir.display()
    )))?;
    let outcome = watch_loop(client, dir, args).await;
    log.abort();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_projects_own_sources_call_for_a_rebuild() {
        let dir = Path::new("/work/target/proj");
        for (path, expected) in [
            ("/work/target/proj/src/lib.rs", true),
            ("/work/target/proj/plugin.toml", true),
            (
                "/work/target/proj/target/wasm32-wasip2/release/proj.wasm",
                false,
            ),
            ("/work/target/proj/.git/index", false),
            ("/work/target/proj/plugin.wasm", false),
            ("/work/target/proj/plugin.grant", false),
            ("/work/target/proj/Cargo.lock", false),
            ("/work/target/proj/src/lib.rs~", false),
        ] {
            assert_eq!(relevant(dir, Path::new(path)), expected, "{path}");
        }
    }
}
