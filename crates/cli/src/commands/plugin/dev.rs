use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use client::proto::plugin::PluginState;
use client::DevEvent;
use tokio::task::JoinHandle;

use super::add::show_grant;
use super::cargo::{self, Component, Metadata};
use super::pack::package_files;
use super::repo::{PluginDir, Repository, CRATE_MANIFEST, MANIFEST};
use super::sdk::{Sdk, SdkOption};
use super::state_label;
use super::watch::{Watch, POLL};
use crate::commands::connect;
use crate::ui::{self, View};

const STAGE: &str = "dist/dev";

/// A plugin crate in a repository, rebuilt on every change to its sources and
/// re-staged here into a package folder the daemon reads in place.
#[derive(Clone)]
struct Staged {
    plugin: PluginDir,
    component: Option<Component>,
    out: PathBuf,
    root: PathBuf,
    package: Option<String>,
    sdk: Option<Sdk>,
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

    /// Builds the crate and stages the result. Nothing is staged, and `self` is
    /// untouched, when either fails, so the running build stays as it was.
    fn rebuilt(&self) -> Result<Staged> {
        let mut next = self.clone();
        next.plugin = self.plugin.reload()?;
        if let Some(package) = &self.package {
            cargo::build_captured(&self.root, &[package.as_str()], self.sdk.as_ref())?;
        }
        next.write()?;
        Ok(next)
    }
}

/// A crate is built and staged; any other folder is already an unpacked
/// package. `built` is the metadata of a build that already covered this crate,
/// so a batch compiles once instead of once per plugin.
fn prepare(
    dir: &Path,
    sdk: Option<Sdk>,
    built: Option<&Metadata>,
) -> Result<(PathBuf, Option<Staged>)> {
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
    let (component, package) = if plugin.manifest.wasm.is_some() {
        let read;
        let metadata = match built {
            Some(metadata) => metadata,
            None => {
                read = Metadata::read(repo.root(), sdk.as_ref())?;
                let package = read.package_name(&plugin.path)?;
                cargo::build(repo.root(), &[package], sdk.as_ref())?;
                &read
            }
        };
        let package = metadata.package_name(&plugin.path)?.to_string();
        (Some(metadata.component(&plugin.path)?), Some(package))
    } else {
        (None, None)
    };
    let out = repo.root().join(STAGE).join(&plugin.manifest.id);
    let staged = Staged {
        plugin,
        component,
        out: out.clone(),
        root: repo.root().to_path_buf(),
        package,
        sdk,
    };
    staged.write()?;
    Ok((out, Some(staged)))
}

/// How a dev run behaves: whether the grant is approved without asking, and
/// whether the command returns once the plugin is installed instead of
/// watching its sources.
pub struct Mode {
    pub yes: bool,
    pub once: bool,
}

fn resolved(dir: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(dir)
        .with_context(|| format!("cannot resolve the plugin folder {}", dir.display()))
}

fn approve(inspection: &client::proto::plugin::PluginInspection, yes: bool) -> Result<()> {
    show_grant(inspection)?;
    if !yes
        && !ui::confirm("run it in dev mode?", "run", "cancel")
            .context("pass --yes to approve exactly what is shown")?
    {
        bail!("dev run cancelled");
    }
    Ok(())
}

/// Installs several plugins of one repository, compiling their crates in a
/// single cargo run.
async fn install_all(dirs: &[PathBuf], yes: bool, sdk: Option<Sdk>) -> Result<()> {
    let dirs = dirs
        .iter()
        .map(|dir| resolved(dir))
        .collect::<Result<Vec<_>>>()?;
    let crates: Vec<&PathBuf> = dirs
        .iter()
        .filter(|dir| dir.join(CRATE_MANIFEST).is_file())
        .collect();
    let built = match crates.first() {
        Some(first) => {
            let repo = Repository::containing(first).context(
                "not inside a plugin repository (a Cargo.toml beside a plugins/ folder)",
            )?;
            let metadata = Metadata::read(repo.root(), sdk.as_ref())?;
            let packages = crates
                .iter()
                .map(|dir| metadata.package_name(dir))
                .collect::<Result<Vec<_>>>()?;
            cargo::build(repo.root(), &packages, sdk.as_ref())?;
            Some(metadata)
        }
        None => None,
    };

    let client = connect().await?;
    let plugins = client.plugin();
    for dir in &dirs {
        let (package, _) = prepare(dir, sdk.clone(), built.as_ref())?;
        let package = package.to_string_lossy().into_owned();
        let inspection = plugins.inspect(&package).await?;
        approve(&inspection, yes)?;
        plugins
            .install_dev(&package, &inspection.grant_digest)
            .await?;
        ui::show(View::line(format!(
            "'{}' installed from {package}",
            inspection.manifest.id
        )))?;
    }
    Ok(())
}

pub async fn run(dirs: &[PathBuf], mode: Mode, sdk: &SdkOption) -> Result<()> {
    let Mode { yes, once } = mode;
    let sdk = Sdk::resolve(sdk)?;
    if dirs.len() > 1 {
        if !once {
            bail!("several folders are installed, not watched: pass --once, or name one folder");
        }
        return install_all(dirs, yes, sdk).await;
    }
    let dir = resolved(dirs.first().map_or(Path::new("."), PathBuf::as_path))?;
    let (package, staged) = prepare(&dir, sdk, None)?;
    let package = package.to_string_lossy().into_owned();

    let client = connect().await?;
    let plugins = client.plugin();
    let inspection = plugins.inspect(&package).await?;
    approve(&inspection, yes)?;
    let id = inspection.manifest.id.clone();
    let mut events = plugins.follow(&id).await?;
    plugins
        .install_dev(&package, &inspection.grant_digest)
        .await?;
    if once {
        return ui::show(View::line(format!(
            "'{id}' installed from {package}; `hestia plugin dev` again rebuilds it"
        )));
    }
    let watching = if staged.is_some() {
        "rebuilds when its sources change and "
    } else {
        ""
    };
    ui::show(View::line(format!(
        "'{id}' runs from {package}, {watching}reloads when it changes; Ctrl-C to stop watching"
    )))?;

    let mut staged = staged;
    let mut watch = staged.as_ref().map(|_| Watch::new(dir.clone()));
    let mut building: Option<JoinHandle<Result<Staged>>> = None;
    let mut tick = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tick.tick(), if building.is_none() => {
                let (Some(current), Some(watch)) = (&staged, &mut watch) else { continue };
                if watch.poll() {
                    ui::show(View::note(format!("'{id}' changed; rebuilding")))?;
                    let current = current.clone();
                    building = Some(tokio::task::spawn_blocking(move || current.rebuilt()));
                }
            }
            built = async { building.as_mut().expect("guarded by the branch condition").await }, if building.is_some() => {
                building = None;
                match built {
                    Ok(Ok(next)) => {
                        staged = Some(next);
                        ui::show(View::note(format!("'{id}' rebuilt")))?;
                    }
                    Ok(Err(error)) => {
                        tracing::warn!(plugin = %id, %error, "dev rebuild failed");
                        ui::show(View::warning(format!(
                            "'{id}' did not build; the previous build keeps running\n{error:#}"
                        )))?;
                    }
                    Err(error) => {
                        tracing::error!(plugin = %id, %error, "dev rebuild task failed");
                        ui::show(View::warning(format!("'{id}' did not build: {error}")))?;
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
