//! Running a flavor's build plan: the host downloads what the plan names, then
//! runs its steps under the process supervisor
//! ([0124](../../../../../docs/decisions/0124-a-build-is-a-plan-the-host-runs.md)).
//! A plan is guest-supplied data, so all of it is vetted before anything runs:
//! every step goes through the checks a guest's own `exec` meets, and every path
//! it names stays under the build area.

use std::future::Future;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};
use proto::plugin::{PluginRunProgress, BUILD_MOUNT};
use proto::provider::flavor::{BuildFetch, BuildPlan, BuildStep};
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio::time::Instant;

use super::build;
use super::Guest;
use crate::cancel::{Cancelled, Job};
use crate::checksum::hex;
use crate::plugins::context::{writable_dir, Mount};
use crate::plugins::download::DownloadScope;
use crate::plugins::imports::CommandRun;
use crate::plugins::process::{owner_of, Invoke, ProgramError};
use crate::plugins::runtime::{LoadedPlugin, CANCEL_GRACE, COMMAND_TIMEOUT};
use crate::process::Task;

const MAX_STEPS: usize = 64;
const MAX_FETCHES: usize = 64;
const MAX_PREFIXES: usize = 32;
const MAX_PREFIX_BYTES: usize = 128;
const PROGRESS_STEPS: u64 = 1000;

#[derive(Debug, thiserror::Error)]
pub(super) enum Fault {
    #[error("a plan may have at most {limit} {what}")]
    TooMany { what: &'static str, limit: usize },
    #[error("`{path}` is not a path under the build area")]
    Path { path: String },
    #[error("step {step}: {source}")]
    Program { step: usize, source: ProgramError },
    #[error("fetch {index}: {reason}")]
    Fetch { index: usize, reason: String },
    #[error("step {step}: the line filter has too many or too long prefixes")]
    Filter { step: usize },
}

/// The supervisor id a step runs under. It names the plugin, the game version
/// and the step, and carries a hash of what the step runs, so a build
/// that is still running (another create, or one from before a daemon restart)
/// is joined only by the same step of the same plan, never by a different one.
pub fn task_id(plugin: &str, version: &str, index: usize, step: &BuildStep) -> String {
    let digest = Sha256::digest(serde_json::to_vec(step).expect("a step is plain data"));
    format!("build-{plugin}-{version}-{index}-{}", &hex(&digest)[..8])
}

/// The path of the guest path `guest` relative to the build area, when it is
/// the area itself or a plain path beneath it.
fn relative(guest: &str) -> Option<PathBuf> {
    let rest = Path::new(guest).strip_prefix(BUILD_MOUNT).ok()?;
    rest.components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| rest.to_path_buf())
}

fn under_area(guest: &str) -> Result<PathBuf, Fault> {
    relative(guest).ok_or_else(|| Fault::Path {
        path: guest.to_string(),
    })
}

/// Checks the whole plan against the plugin's grant and the build area, before
/// any of it runs. Programs are not resolved here: that happens when a step
/// starts, and a program the computer lacks fails that step.
pub(super) fn vet(
    plugin: &LoadedPlugin,
    plan: &BuildPlan,
    mounts: &[Mount],
    game_java: Option<u32>,
) -> Result<(), Fault> {
    for (what, count, limit) in [
        ("steps", plan.steps.len(), MAX_STEPS),
        ("downloads", plan.fetch.len(), MAX_FETCHES),
    ] {
        if count > limit {
            return Err(Fault::TooMany { what, limit });
        }
    }
    for (index, fetch) in plan.fetch.iter().enumerate() {
        let reason = |reason: String| Fault::Fetch { index, reason };
        match fetch.checksum.as_ref() {
            Some(checksum) if !checksum.is_valid() => {
                return Err(reason("the checksum is malformed".into()))
            }
            _ => {}
        }
        plugin
            .downloads()
            .vet(&fetch.url, fetch.checksum.is_some())
            .map_err(|error| reason(error.to_string()))?;
        if under_area(&fetch.dest)?.as_os_str().is_empty() {
            return Err(Fault::Path {
                path: fetch.dest.clone(),
            });
        }
    }
    for (index, step) in plan.steps.iter().enumerate() {
        let step_number = index + 1;
        plugin
            .programs()
            .vet(&invoke(step), mounts, game_java)
            .map_err(|source| Fault::Program {
                step: step_number,
                source,
            })?;
        if let Some(cwd) = &step.cwd {
            under_area(cwd)?;
        }
        for created in &step.creates {
            under_area(created)?;
        }
        if let Some(filter) = &step.narrate {
            if filter.skip_prefixes.len() > MAX_PREFIXES
                || filter
                    .skip_prefixes
                    .iter()
                    .any(|prefix| prefix.len() > MAX_PREFIX_BYTES)
            {
                return Err(Fault::Filter { step: step_number });
            }
        }
    }
    build::vet_outputs(&plan.outputs).map_err(|path| Fault::Path { path })
}

fn invoke(step: &BuildStep) -> Invoke<'_> {
    Invoke {
        program: &step.program,
        args: step.args.clone(),
        env: step
            .env
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        java_major: step.java_major,
    }
}

/// What a plan runs in: the plugin, the build area and its mounts, the game
/// version the build is for and where the build reports to.
pub(super) struct Run<'a> {
    pub guest: &'a Guest,
    pub area: &'a Path,
    pub mounts: &'a [Mount],
    pub game_java: Option<u32>,
    pub version: &'a str,
    pub phase: ProvisionPhase,
    pub on: &'a Job<'a>,
}

impl Run<'_> {
    fn failed(&self, detail: String) -> anyhow::Error {
        self.guest.failed(format!("build: {detail}"))
    }

    fn stopped(&self, error: anyhow::Error) -> anyhow::Error {
        match self.on.is_cancelled() {
            true => Cancelled.into(),
            false => error,
        }
    }

    fn satisfied(&self, step: &BuildStep) -> bool {
        !step.creates.is_empty()
            && step.creates.iter().all(|created| {
                relative(created)
                    .is_some_and(|rel| std::fs::symlink_metadata(self.area.join(rel)).is_ok())
            })
    }
}

/// Runs a vetted plan: the downloads, if any step still has work to do, then
/// each step in order. Cancelling is honoured before every download and step,
/// and stops the one running.
pub(super) async fn execute(run: &Run<'_>, plan: &BuildPlan) -> anyhow::Result<()> {
    let needed = plan.steps.is_empty() || plan.steps.iter().any(|step| !run.satisfied(step));
    if needed {
        for (index, fetch) in plan.fetch.iter().enumerate() {
            run.on.check()?;
            download(run, index, fetch).await?;
        }
    }
    let total = plan.steps.len() as u64;
    for (index, step) in plan.steps.iter().enumerate() {
        run.on.check()?;
        let plugin = run.guest.plugin();
        if run.satisfied(step) {
            tracing::debug!(plugin = plugin.id(), program = %step.program, step = index + 1, "build step already done");
            continue;
        }
        run_step(run, index, total, step).await?;
    }
    Ok(())
}

async fn download(run: &Run<'_>, index: usize, fetch: &BuildFetch) -> anyhow::Result<()> {
    let plugin = run.guest.plugin();
    let rel = under_area(&fetch.dest)?;
    let destination = run.area.join(&rel);
    place_parents(run.area, &rel).map_err(|error| run.failed(error.to_string()))?;
    let (progress, reports) = watch::channel(None);
    let command = CommandRun {
        cancel: run.on.cancel().clone(),
        progress,
        grace: CANCEL_GRACE,
    };
    let scope = DownloadScope {
        deadline: Instant::now() + COMMAND_TIMEOUT,
        run: Some(&command),
    };
    let transfer =
        plugin
            .downloads()
            .fetch(&fetch.url, fetch.checksum.clone(), &destination, &scope);
    relay(reports, transfer, run.on, run.phase)
        .await
        .map(drop)
        .map_err(|error| {
            tracing::warn!(
                plugin = plugin.id(),
                index,
                kind = error.kind(),
                "build download failed"
            );
            run.stopped(run.failed(format!("download {}: {error}", index + 1)))
        })
}

/// Creates the directories of `rel` beneath `area`, refusing to pass through a
/// symlink a program left there, so the host never creates directories outside it.
fn place_parents(area: &Path, rel: &Path) -> std::io::Result<()> {
    let mut at = area.to_path_buf();
    let Some(parent) = rel.parent() else {
        return Ok(());
    };
    for part in parent.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(std::io::Error::other(
                    "a directory of the path is a symlink",
                ))
            }
            Ok(_) => {}
            Err(_) => std::fs::create_dir(&at)?,
        }
    }
    Ok(())
}

async fn run_step(run: &Run<'_>, index: usize, total: u64, step: &BuildStep) -> anyhow::Result<()> {
    let plugin = run.guest.plugin();
    let number = index + 1;
    let prepared = plugin
        .programs()
        .prepare(invoke(step), run.mounts, run.game_java)
        .await
        .map_err(|error| run.failed(format!("step {number}: {error}")))?;
    let cwd = match &step.cwd {
        Some(guest) => writable_dir(run.mounts, guest).map_err(|reason| run.failed(reason))?,
        None => run.area.to_path_buf(),
    };
    run.on.report(&ProvisionProgress {
        phase: run.phase,
        current: index as u64,
        total,
        ..ProvisionProgress::default()
    });
    tracing::info!(plugin = plugin.id(), program = %step.program, step = number, of = total, "build step starting");
    let filter = step.narrate.clone();
    let narrates = move |line: &str| filter.as_ref().is_some_and(|f| f.narrates(line));
    let task = Task {
        narrates: &narrates,
        deadline: Some(COMMAND_TIMEOUT),
        env: prepared.vars,
        clear_env: true,
        owner: Some(owner_of(plugin.id())),
        position: (index as u64, total),
        ..Task::new(
            task_id(plugin.id(), run.version, index, step),
            &prepared.executable,
            prepared.args,
            cwd,
            run.phase,
        )
    };
    let outcome = plugin
        .programs()
        .supervisor
        .run(task, run.on)
        .await
        .with_context(|| format!("step {number} (`{}`)", step.program))
        .map_err(|error| match crate::cancel::is_cancelled(&error) {
            true => error,
            false => run.failed(format!("{error:#}")),
        })?;
    if !outcome.succeeded() {
        tracing::warn!(plugin = plugin.id(), program = %step.program, step = number, "build step failed");
        bail!(run.failed(format!("step {number} (`{}`) failed", step.program)));
    }
    if !step.creates.is_empty() && !run.satisfied(step) {
        bail!(run.failed(format!(
            "step {number} (`{}`) did not create what it declared",
            step.program
        )));
    }
    Ok(())
}

/// Awaits `work`, showing what it reports as the provision's progress meanwhile.
pub(super) async fn relay<T>(
    mut reports: watch::Receiver<Option<PluginRunProgress>>,
    work: impl Future<Output = T>,
    on: &Job<'_>,
    phase: ProvisionPhase,
) -> T {
    tokio::pin!(work);
    loop {
        tokio::select! {
            outcome = &mut work => return outcome,
            Ok(()) = reports.changed() => {
                let latest = reports.borrow_and_update().clone();
                if let Some(latest) = latest {
                    on.report(&progress_of(phase, &latest));
                }
            }
        }
    }
}

fn progress_of(phase: ProvisionPhase, reported: &PluginRunProgress) -> ProvisionProgress {
    ProvisionProgress {
        phase,
        current: reported
            .fraction
            .map_or(0, |fraction| (fraction * PROGRESS_STEPS as f32) as u64),
        total: reported.fraction.map_or(0, |_| PROGRESS_STEPS),
        detail: reported.message.clone(),
        ..ProvisionProgress::default()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn step(args: &[&str]) -> BuildStep {
        BuildStep {
            program: "tool".into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            ..BuildStep::default()
        }
    }

    #[test]
    fn a_task_is_joined_only_by_the_same_step_of_the_same_plan() {
        let id = task_id("plug", "1.21", 0, &step(&["a"]));
        assert_eq!(id, task_id("plug", "1.21", 0, &step(&["a"])));
        assert_ne!(id, task_id("plug", "1.21", 0, &step(&["b"])));
        assert_ne!(id, task_id("plug", "1.21", 1, &step(&["a"])));
        assert_ne!(id, task_id("plug", "1.20", 0, &step(&["a"])));
        assert_ne!(id, task_id("other", "1.21", 0, &step(&["a"])));
        assert!(id.starts_with("build-plug-1.21-0-"));
    }

    #[test]
    fn only_a_plain_path_under_the_build_area_is_accepted() {
        assert_eq!(relative("/build"), Some(PathBuf::new()));
        assert_eq!(relative("/build/a/b.jar"), Some(PathBuf::from("a/b.jar")));
        for refused in [
            "/build/../etc",
            "/build/a/../../x",
            "/data/x",
            "/buildx",
            "build/x",
            "",
        ] {
            assert!(relative(refused).is_none(), "{refused}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_stops_the_host_creating_beneath_it() {
        let area = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), area.path().join("link")).unwrap();

        assert!(place_parents(area.path(), Path::new("link/deep/file.jar")).is_err());
        assert!(!outside.path().join("deep").exists());
        assert!(place_parents(area.path(), Path::new("fresh/deep/file.jar")).is_ok());
        assert!(area.path().join("fresh/deep").is_dir());
    }
}
