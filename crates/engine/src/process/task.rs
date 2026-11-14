//! Running a program to completion under the supervisor — what every
//! provisioning step that shells out uses, so none of them spawns a bare child.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};
use proto::process::{LogSource, ProcessSpec, ProcessState};

use super::ProcessSupervisor;
use crate::cancel::Job;

const POLL: Duration = Duration::from_millis(250);
const REPORT_EVERY: Duration = Duration::from_secs(1);
const MAX_DETAIL: usize = 160;
/// A compile phase emits hundreds of tool lines between two of the build's own.
const NARRATION_WINDOW: usize = 400;

/// What to do with an output line: show it as progress, or keep it in the log.
pub type Narrator<'a> = &'a (dyn Fn(&str) -> bool + Sync);

fn silent(_line: &str) -> bool {
    false
}

pub struct Task<'a> {
    /// One id is one run: a task started while an identical id is still running
    /// joins it rather than starting a second.
    pub id: String,
    pub program: &'a Path,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub phase: ProvisionPhase,
    pub narrates: Narrator<'a>,
    /// Killed and treated as finished once this elapses.
    pub deadline: Option<Duration>,
    pub env: BTreeMap<String, String>,
    /// Start from `env` alone instead of the daemon's environment.
    pub clear_env: bool,
    /// Who the process belongs to, such as `plugin:<id>`.
    pub owner: Option<String>,
    /// Where this task sits in a sequence, shown as its progress; `(0, 0)` for a
    /// task on its own.
    pub position: (u64, u64),
}

impl<'a> Task<'a> {
    /// A task that narrates nothing, runs without a time limit in the daemon's
    /// environment and belongs to no one.
    pub fn new(
        id: String,
        program: &'a Path,
        args: Vec<String>,
        cwd: PathBuf,
        phase: ProvisionPhase,
    ) -> Self {
        Task {
            id,
            program,
            args,
            cwd,
            phase,
            narrates: &silent,
            deadline: None,
            env: BTreeMap::new(),
            clear_env: false,
            owner: None,
            position: (0, 0),
        }
    }
}

/// How a task ended, for the callers that read a result off disk rather than
/// off the exit code.
pub enum Outcome {
    Exited(Option<i32>),
    TimedOut,
}

impl Outcome {
    pub fn succeeded(&self) -> bool {
        matches!(self, Outcome::Exited(Some(0)))
    }
}

impl ProcessSupervisor {
    /// Run `task` to completion, relaying whatever it narrates as progress.
    /// Cancelling stops it through the supervisor, so the tools it spawned go
    /// with it.
    pub async fn run(&self, task: Task<'_>, job: &Job<'_>) -> Result<Outcome> {
        let already_running = self
            .status(&task.id)
            .is_some_and(|p| p.state == ProcessState::Running);
        if !already_running {
            self.start(ProcessSpec {
                id: task.id.clone(),
                program: task.program.to_string_lossy().into_owned(),
                args: task.args.clone(),
                cwd: Some(task.cwd.clone()),
                env: task.env.clone(),
                clear_env: task.clear_env,
                owner: task.owner.clone(),
                log: LogSource::Capture,
            })
            .await
            .map_err(|e| anyhow::anyhow!("cannot run {}: {e:?}", task.program.display()))?;
        }
        self.watch(&task, job).await
    }

    async fn watch(&self, task: &Task<'_>, job: &Job<'_>) -> Result<Outcome> {
        let started = Instant::now();
        let mut reported = started - REPORT_EVERY;
        let mut narration = String::new();
        loop {
            if job.is_cancelled() {
                self.stop(&task.id);
                job.check()?;
            }
            if task
                .deadline
                .is_some_and(|limit| started.elapsed() >= limit)
            {
                self.stop(&task.id);
                return Ok(Outcome::TimedOut);
            }
            let info = self
                .status(&task.id)
                .with_context(|| format!("{} vanished while running", task.id))?;
            match info.state {
                ProcessState::Running => {}
                ProcessState::Killed => bail!("{} was stopped", task.id),
                ProcessState::Exited => return Ok(Outcome::Exited(info.exit_code)),
            }
            if reported.elapsed() >= REPORT_EVERY {
                reported = Instant::now();
                if let Some(step) = self.last_narrated(task) {
                    narration = step;
                }
                if !narration.is_empty() {
                    job.report(&ProvisionProgress {
                        phase: task.phase,
                        current: task.position.0,
                        total: task.position.1,
                        detail: narration.clone(),
                        ..ProvisionProgress::default()
                    });
                }
            }
            tokio::time::sleep(POLL).await;
        }
    }

    fn last_narrated(&self, task: &Task<'_>) -> Option<String> {
        self.logs(&task.id, Some(NARRATION_WINDOW))?
            .into_iter()
            .rev()
            .map(|line| line.line)
            .find(|line| line.len() <= MAX_DETAIL && (task.narrates)(line))
    }
}
