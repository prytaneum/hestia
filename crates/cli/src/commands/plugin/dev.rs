use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use client::proto::plugin::PluginState;
use client::DevEvent;

use super::add::show_grant;
use super::cargo::{self, Component, Metadata};
use super::pack::package_files;
use super::repo::{PluginDir, Repository, CRATE_MANIFEST, MANIFEST};
use super::state_label;
use crate::commands::connect;
use crate::ui::{self, View};

const STAGE: &str = "dist/dev";
const POLL: Duration = Duration::from_secs(1);
const WATCHED_DIRS: [&str; 2] = ["assets", "themes"];

/// A plugin crate in a repository, rebuilt by the author and re-staged here
/// into a package folder the daemon reads in place.
struct Staged {
    plugin: PluginDir,
    component: Option<Component>,
    out: PathBuf,
}

impl Staged {
    fn write(&self) -> Result<()> {
        let (_, files) = package_files(&self.plugin, self.component.as_ref())?;
        for (name, bytes) in files {
            let target = self.out.join(&name);
            std::fs::create_dir_all(target.parent().unwrap_or(&self.out))?;
            let partial = target.with_extension("partial");
            std::fs::write(&partial, bytes)?;
            std::fs::rename(&partial, &target)?;
        }
        Ok(())
    }

    fn stamp(&self) -> Option<SystemTime> {
        let sources = self
            .component
            .iter()
            .map(|component| component.wasm.clone())
            .chain(std::iter::once(self.plugin.path.join(MANIFEST)));
        let dirs = WATCHED_DIRS.map(|dir| self.plugin.path.join(dir));
        sources
            .filter_map(|file| modified(&file))
            .chain(dirs.iter().filter_map(|dir| newest_in(dir)))
            .max()
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

fn newest_in(dir: &Path) -> Option<SystemTime> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                newest_in(&path)
            } else {
                modified(&path)
            }
        })
        .max()
}

/// A crate is built and staged; any other folder is already an unpacked package.
fn prepare(dir: &Path) -> Result<(PathBuf, Option<Staged>)> {
    if !dir.join(MANIFEST).is_file() {
        bail!(
            "{} has no {MANIFEST}; point at a plugin folder",
            dir.display()
        );
    }
    if !dir.join(CRATE_MANIFEST).is_file() {
        return Ok((dir.to_path_buf(), None));
    }
    let repo = Repository::containing(dir)
        .context("not inside a plugin repository (a Cargo.toml beside a plugins/ folder)")?;
    let plugin = repo
        .plugins()?
        .into_iter()
        .find(|plugin| std::fs::canonicalize(&plugin.path).is_ok_and(|path| path == dir))
        .context("this folder is not one of the repository's plugins")?;
    let component = if plugin.manifest.wasm.is_some() {
        let metadata = Metadata::read(repo.root())?;
        cargo::build(repo.root(), &[metadata.package_name(&plugin.path)?])?;
        Some(metadata.component(&plugin.path)?)
    } else {
        None
    };
    let out = repo.root().join(STAGE).join(&plugin.manifest.id);
    let staged = Staged {
        plugin,
        component,
        out: out.clone(),
    };
    staged.write()?;
    Ok((out, Some(staged)))
}

pub async fn run(dir: Option<&Path>, yes: bool) -> Result<()> {
    let dir = std::fs::canonicalize(dir.unwrap_or(Path::new(".")))
        .context("cannot resolve the plugin folder")?;
    let (package, staged) = prepare(&dir)?;
    let package = package.to_string_lossy().into_owned();

    let client = connect().await?;
    let plugins = client.plugin();
    let inspection = plugins.inspect(&package).await?;
    show_grant(&inspection)?;
    if !yes
        && !ui::confirm("run it in dev mode?", "run", "cancel")
            .context("pass --yes to approve exactly what is shown")?
    {
        bail!("dev run cancelled");
    }
    let id = inspection.manifest.id.clone();
    let mut events = plugins.follow(&id).await?;
    plugins
        .install_dev(&package, &inspection.grant_digest)
        .await?;
    ui::show(View::line(format!(
        "'{id}' runs from {package} and reloads when it changes; Ctrl-C to stop watching"
    )))?;

    let mut stamp = staged.as_ref().and_then(Staged::stamp);
    let mut tick = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick() => {
                let Some(staged) = &staged else { continue };
                let now = staged.stamp();
                if now != stamp {
                    stamp = now;
                    if let Err(error) = staged.write() {
                        ui::show(View::warning(format!("cannot stage the plugin: {error:#}")))?;
                    }
                }
            }
            event = events.recv() => match event {
                Some(DevEvent::Log(line)) => ui::show(View::line(format!("[{}] {}", line.level, line.text)))?,
                Some(DevEvent::State(change)) => {
                    let label = state_label(change.state);
                    let note = match (change.error, change.state) {
                        (Some(error), _) => format!("{label}: {error}"),
                        (None, PluginState::NeedsApproval) => format!(
                            "{label}: the manifest asks for more than you approved; stop and run `hestia plugin dev` again"
                        ),
                        (None, _) => label.to_string(),
                    };
                    ui::show(View::note(note))?;
                }
                None => bail!("the daemon closed the connection"),
            },
        }
    }
    ui::show(View::note(format!(
        "'{id}' stays installed; `hestia plugin remove {id}` removes it"
    )))
}
