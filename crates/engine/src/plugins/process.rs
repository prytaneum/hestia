//! What a plugin's `process` import does: run the programs its grant declares
//! and supervise the ones it starts
//! ([0101](../../../../docs/decisions/0101-plugins-spawn-only-declared-programs.md)).
//! Program ids, never paths, cross the boundary, and argument lists and
//! environment names are checked against the grant on every call. Arguments
//! and values may carry credentials, so only program ids and statuses are logged.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use proto::plugin::{Grant, Invocation, Permission, PluginRunProgress, Program};
use proto::process::{ProcessInfo, ProcessSpec, ProcessState};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::Instant;

use super::context::Mount;
use super::imports::HostCalls;
use super::invocation::{self, Call, Selection};
use super::program::{self, ManagedJava, Pins, Resolver, StageError};
use crate::cancel::Cancel;
use crate::process::identity::Tree;
use crate::process::ProcessSupervisor;

const OUTPUT_CAP: usize = 1024 * 1024;
const LIVE_LINE_BYTES: usize = 512;
const CANCEL_POLL: Duration = Duration::from_millis(50);
const MOUNT_NAMES: [&str; 3] = ["build", "vanilla", "data"];
const MAX_RUNNING: usize = 8;
const MAX_LOG_LINES: usize = 1000;
const MAX_ARGS: usize = 256;
const MAX_TOKEN_BYTES: usize = 32 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ProgramError {
    #[error("{0}")]
    Denied(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("`{name}` is not installed on this computer")]
    Missing { name: String, url: Option<String> },
    #[error("`{0}` was not on this computer when the plugin was approved; approve the plugin again to pin it")]
    Unapproved(String),
    #[error("{0}")]
    Failed(String),
}

pub(crate) struct Output {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// What bounds and resolves one `exec`: the mounts of the current call, the
/// instant its budget ends, and the signal that ends it early.
pub(crate) struct CallScope<'a> {
    pub mounts: &'a [Mount],
    pub deadline: Instant,
    pub cancel: Option<&'a Cancel>,
    pub game_java: Option<u32>,
    /// Where the call shows progress, when it has a sink: each line the
    /// program prints is shown as it arrives.
    pub progress: Option<&'a watch::Sender<Option<PluginRunProgress>>>,
}

/// Which program to run and how: what a guest's `exec` and a build plan's step
/// both name, checked against the same declared invocations.
pub(crate) struct Invoke<'a> {
    pub program: &'a str,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub java_major: Option<u32>,
}

/// What a guest's `exec` asked for.
pub(crate) struct ExecRequest<'a> {
    pub invoke: Invoke<'a>,
    pub timeout_ms: u32,
    pub cwd: Option<PathBuf>,
}

/// An authorised call made ready to start: the file to run, the arguments with
/// their mounts expanded and the environment it runs with.
pub(crate) struct Prepared {
    pub program: Program,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub vars: BTreeMap<String, String>,
}

/// Kills the child's whole tree when dropped, so a cancelled, timed-out or
/// dropped call leaves nothing behind. Disarmed once the child has been reaped:
/// a group id outlives its leader and could by then belong to a stranger.
struct TreeGuard(Option<Tree>);

impl Drop for TreeGuard {
    fn drop(&mut self) {
        if let Some(tree) = self.0.take() {
            tree.kill();
        }
    }
}

/// One plugin's grip on the programs it declared. Holds nothing mutable: every
/// call re-reads the supervisor, so a fresh instance sees the same processes.
pub(crate) struct Programs {
    pub(crate) plugin: Arc<str>,
    pub(crate) grant: Arc<Grant>,
    /// The `PATH` programs, pinned when the grant was approved. A program
    /// absent here was not found then and runs only after a new approval.
    pub(crate) pins: Pins,
    pub(crate) resolver: Resolver,
    pub(crate) host: Arc<dyn HostCalls>,
    pub(crate) data_dir: PathBuf,
    pub(crate) bin_dir: PathBuf,
    pub(crate) supervisor: Arc<ProcessSupervisor>,
    pub(crate) java: Arc<dyn ManagedJava>,
}

/// The `owner` the supervisor records for processes `plugin` started.
pub fn owner_of(plugin: &str) -> String {
    format!("plugin:{plugin}")
}

impl Programs {
    pub(crate) async fn exec(
        &self,
        run: ExecRequest<'_>,
        scope: &CallScope<'_>,
    ) -> Result<Output, ProgramError> {
        let ExecRequest {
            invoke,
            timeout_ms,
            cwd,
        } = run;
        let Prepared {
            program,
            executable,
            args,
            vars,
        } = self.prepare(invoke, scope.mounts, scope.game_java).await?;
        let remaining = scope.deadline.saturating_duration_since(Instant::now());
        let timeout = Duration::from_millis(u64::from(timeout_ms).max(1)).min(remaining);
        if timeout.is_zero() {
            return Err(ProgramError::Failed(
                "the call has no time left to run a program".into(),
            ));
        }

        let mut command = Command::new(&executable);
        command
            .args(&args)
            .env_clear()
            .envs(vars)
            .current_dir(cwd.as_ref().unwrap_or(&self.data_dir))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        #[cfg(windows)]
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        let mut child = command.spawn().map_err(|error| {
            ProgramError::Failed(format!("cannot run `{}`: {error}", program.id))
        })?;
        let mut tree = TreeGuard(Tree::hold(&child));
        let (stdout, stderr) = (
            child.stdout.take().expect("stdout is piped"),
            child.stderr.take().expect("stderr is piped"),
        );
        let finished = async {
            tokio::join!(
                child.wait(),
                capped(stdout, scope.progress),
                capped(stderr, scope.progress)
            )
        };
        let (status, stdout, stderr) = tokio::select! {
            finished = finished => finished,
            () = tokio::time::sleep(timeout) => {
                tracing::warn!(plugin = %self.plugin, program = %program.id, ?timeout, "plugin program timed out");
                return Err(ProgramError::Failed(format!(
                    "`{}` did not finish within {timeout:?}",
                    program.id
                )));
            }
            () = cancelled(scope.cancel) => {
                tracing::info!(plugin = %self.plugin, program = %program.id, "plugin program cancelled");
                return Err(ProgramError::Failed(format!("`{}` was cancelled", program.id)));
            }
        };
        tree.0 = None;
        let status = status
            .map_err(|error| {
                ProgramError::Failed(format!("cannot wait for `{}`: {error}", program.id))
            })?
            .code();
        tracing::info!(plugin = %self.plugin, program = %program.id, ?status, "plugin program ran");
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    /// Whether `invoke` names a declared program, fits one of its invocations
    /// and refers only to mounts `mounts` has, without resolving or running
    /// anything.
    pub(crate) fn vet(
        &self,
        invoke: &Invoke<'_>,
        mounts: &[Mount],
        game_java: Option<u32>,
    ) -> Result<(), ProgramError> {
        self.authorize(
            invoke.program,
            &invoke.args,
            &invoke.env,
            invoke.java_major,
            game_java,
        )?;
        invoke
            .args
            .iter()
            .try_for_each(|arg| expand_mounts(arg, mounts).map(drop))
    }

    /// Authorises `invoke`, expands the mounts of the call in its arguments and
    /// resolves the program to the file that runs.
    pub(crate) async fn prepare(
        &self,
        invoke: Invoke<'_>,
        mounts: &[Mount],
        game_java: Option<u32>,
    ) -> Result<Prepared, ProgramError> {
        let (program, selection) = self.authorize(
            invoke.program,
            &invoke.args,
            &invoke.env,
            invoke.java_major,
            game_java,
        )?;
        let args = invoke
            .args
            .iter()
            .map(|arg| expand_mounts(arg, mounts))
            .collect::<Result<Vec<_>, _>>()?;
        let vars = self.child_env(&program, &selection.invocation, invoke.env)?;
        let executable = self.executable(&program, selection.java).await?;
        Ok(Prepared {
            program,
            executable,
            args,
            vars,
        })
    }

    pub(crate) async fn spawn(
        &self,
        program: &str,
        args: Vec<String>,
        env: Vec<(String, String)>,
        java_major: Option<u32>,
    ) -> Result<String, ProgramError> {
        let (program, selection) = self.authorize(program, &args, &env, java_major, None)?;
        let vars = self.child_env(&program, &selection.invocation, env)?;
        if self.running().len() >= MAX_RUNNING {
            return Err(ProgramError::Failed(format!(
                "a plugin may keep at most {MAX_RUNNING} processes running"
            )));
        }
        let executable = self.executable(&program, selection.java).await?;
        let info = self
            .supervisor
            .start(ProcessSpec {
                program: executable.to_string_lossy().into_owned(),
                args,
                cwd: Some(self.data_dir.clone()),
                env: vars,
                clear_env: true,
                owner: Some(owner_of(&self.plugin)),
                ..ProcessSpec::default()
            })
            .await
            .map_err(|error| {
                ProgramError::Failed(format!("cannot start `{}`: {error}", program.id))
            })?;
        tracing::info!(plugin = %self.plugin, program = %program.id, process = %info.id, "plugin program started");
        Ok(info.id)
    }

    pub(crate) fn stop(&self, id: &str) -> Result<(), ProgramError> {
        self.require_spawn()?;
        let info = self.owned(id)?;
        self.supervisor.stop(&info.id);
        tracing::info!(plugin = %self.plugin, process = %info.id, "plugin stopped its program");
        Ok(())
    }

    pub(crate) fn status(&self, id: &str) -> Result<ProcessInfo, ProgramError> {
        self.require_spawn()?;
        self.owned(id)
    }

    pub(crate) fn logs(&self, id: &str, tail: u32) -> Result<Vec<String>, ProgramError> {
        self.require_spawn()?;
        let info = self.owned(id)?;
        let tail = (tail as usize).min(MAX_LOG_LINES);
        Ok(self
            .supervisor
            .logs(&info.id, Some(tail))
            .unwrap_or_default()
            .into_iter()
            .map(|line| line.line)
            .collect())
    }

    pub(crate) fn list(&self) -> Result<Vec<String>, ProgramError> {
        self.require_spawn()?;
        Ok(self.running().into_iter().map(|info| info.id).collect())
    }

    /// The environment `invocation` of `program` runs with: the base, the
    /// variables it declares (`authorize` refused anything else), and a `PATH`
    /// of only the pinned directories of the programs it lists in `path`.
    fn child_env(
        &self,
        program: &Program,
        invocation: &Invocation,
        env: Vec<(String, String)>,
    ) -> Result<BTreeMap<String, String>, ProgramError> {
        let mut vars: BTreeMap<String, String> = base_env().collect();
        if !invocation.path.is_empty() {
            let directories = invocation
                .path
                .iter()
                .map(|id| {
                    self.pinned(id)
                        .and_then(|path| path.parent().map(PathBuf::from))
                        .ok_or_else(|| {
                            ProgramError::Failed(format!(
                                "`{id}`, on the path of `{}`, was not found where it was pinned",
                                program.id
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let joined = std::env::join_paths(directories)
                .map_err(|error| ProgramError::Invalid(error.to_string()))?;
            vars.insert("PATH".into(), joined.to_string_lossy().into_owned());
        }
        vars.extend(env);
        Ok(vars)
    }

    /// The path the `PATH` program `id` was pinned to at approval, while it is
    /// still executable there.
    fn pinned(&self, id: &str) -> Option<PathBuf> {
        self.pins
            .get(id)
            .map(|pin| PathBuf::from(&pin.path))
            .filter(|path| program::is_executable(path))
    }

    /// The pinned program `id` a launch may run through, when this plugin holds
    /// `process:spawn`.
    pub(crate) fn wrapper(&self, id: &str) -> Option<PathBuf> {
        self.require_spawn().ok()?;
        self.pinned(id)
    }

    fn require_spawn(&self) -> Result<(), ProgramError> {
        if self.grant.holds(Permission::ProcessSpawn) {
            return Ok(());
        }
        Err(ProgramError::Denied(
            "running programs needs the `process:spawn` permission".into(),
        ))
    }

    /// The declared program `id` and how this call runs it, when the call fits
    /// one of its invocations. A refusal says which rule failed, never what the
    /// arguments were.
    fn authorize(
        &self,
        id: &str,
        args: &[String],
        env: &[(String, String)],
        java_major: Option<u32>,
        game_java: Option<u32>,
    ) -> Result<(Program, Selection), ProgramError> {
        self.require_spawn()?;
        if id.contains('\0')
            || args.len() > MAX_ARGS
            || args
                .iter()
                .any(|arg| arg.contains('\0') || arg.len() > MAX_TOKEN_BYTES)
            || env
                .iter()
                .any(|(name, value)| name.contains('\0') || value.contains('\0'))
        {
            return Err(ProgramError::Invalid(
                "a program id and its arguments must be text without NUL, within the size limits"
                    .into(),
            ));
        }
        let program = self.grant.program(id).ok_or_else(|| {
            tracing::warn!(plugin = %self.plugin, program = id, "undeclared program refused");
            ProgramError::Denied(format!("`{id}` is not a program this plugin declares"))
        })?;
        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
        let selection = invocation::select(
            program,
            &Call {
                args,
                env: &names,
                java_major,
                game_java,
            },
        )
        .inspect_err(|error| {
            tracing::warn!(plugin = %self.plugin, program = id, %error, "call outside the declared invocations refused");
        })?;
        Ok((program.clone(), selection))
    }

    async fn executable(
        &self,
        program: &Program,
        java: Option<u32>,
    ) -> Result<PathBuf, ProgramError> {
        if let Some(major) = java {
            return self.java.executable(major).await.map_err(|error| {
                tracing::warn!(plugin = %self.plugin, program = %program.id, major, error = %format!("{error:#}"), "managed java unavailable");
                match &program.requires {
                    Some(_) => missing(program),
                    None => ProgramError::Missing {
                        name: format!("Hestia-managed Java {major}"),
                        url: None,
                    },
                }
            });
        }
        if program.on_path() {
            return self.pinned_executable(program);
        }
        let (data_dir, bin_dir, owned) =
            (self.data_dir.clone(), self.bin_dir.clone(), program.clone());
        let staged = tokio::task::spawn_blocking(move || {
            program::stage(&data_dir, &bin_dir, &owned, &program::target_triple())
        })
        .await
        .map_err(|error| ProgramError::Failed(error.to_string()))?;
        staged.map_err(|error| {
            tracing::warn!(plugin = %self.plugin, %error, "program refused");
            match &error {
                StageError::HashMismatch | StageError::NoHashForTarget(_) => {
                    ProgramError::Denied(error.to_string())
                }
                StageError::Missing(_) => missing(program),
                StageError::Io(_) => ProgramError::Failed(error.to_string()),
            }
        })
    }

    /// The file a `PATH` program was approved at. A program the approval did
    /// not find is never run from a path nobody approved: when it exists now,
    /// the plugin is flagged for approval so the user can pin it, and the call
    /// fails.
    fn pinned_executable(&self, program: &Program) -> Result<PathBuf, ProgramError> {
        if self.pins.contains_key(&program.id) {
            return self.pinned(&program.id).ok_or_else(|| {
                tracing::warn!(plugin = %self.plugin, program = %program.id, "program not found at its pinned path");
                missing(program)
            });
        }
        if (self.resolver)(&program.command).is_some() {
            tracing::warn!(plugin = %self.plugin, program = %program.id, "program found after approval; the plugin needs approval to run it");
            self.host.needs_approval(&self.plugin, &program.id);
            return Err(ProgramError::Unapproved(program.id.clone()));
        }
        tracing::warn!(plugin = %self.plugin, program = %program.id, "program not found on PATH");
        Err(missing(program))
    }

    /// The process `id` when this plugin started it; a stranger's is reported
    /// as absent so ids of other owners cannot be probed.
    fn owned(&self, id: &str) -> Result<ProcessInfo, ProgramError> {
        let owner = owner_of(&self.plugin);
        self.supervisor
            .status(id)
            .filter(|info| info.owner.as_deref() == Some(owner.as_str()))
            .ok_or_else(|| ProgramError::NotFound(format!("no process `{id}` of this plugin")))
    }

    fn running(&self) -> Vec<ProcessInfo> {
        let owner = owner_of(&self.plugin);
        self.supervisor
            .list()
            .into_iter()
            .filter(|info| {
                info.state == ProcessState::Running && info.owner.as_deref() == Some(owner.as_str())
            })
            .collect()
    }
}

fn missing(program: &Program) -> ProgramError {
    let requirement = program::requirement_of(program);
    ProgramError::Missing {
        name: requirement.name,
        url: Some(requirement.url).filter(|url| !url.is_empty()),
    }
}

/// `arg` with each `${build}`, `${vanilla}` and `${data}` replaced by the host
/// path of that mount of the current call. Patterns were matched against the
/// guest form first, so a grant stays host-independent.
fn expand_mounts(arg: &str, mounts: &[Mount]) -> Result<String, ProgramError> {
    let mut expanded = arg.to_string();
    for name in MOUNT_NAMES {
        let token = format!("${{{name}}}");
        if !expanded.contains(&token) {
            continue;
        }
        let mount = mounts
            .iter()
            .find(|mount| mount.guest.strip_prefix('/') == Some(name))
            .ok_or_else(|| {
                ProgramError::Denied(format!("this call has no `{name}` directory to refer to"))
            })?;
        expanded = expanded.replace(&token, &mount.host.to_string_lossy());
    }
    Ok(expanded)
}

/// Resolves once `cancel` is raised; never when there is nothing to cancel.
async fn cancelled(cancel: Option<&Cancel>) {
    let Some(cancel) = cancel else {
        return std::future::pending().await;
    };
    while !cancel.is_cancelled() {
        tokio::time::sleep(CANCEL_POLL).await;
    }
}

/// What every program gets: nothing, except on Windows the system directory
/// most programs cannot start without.
fn base_env() -> impl Iterator<Item = (String, String)> {
    #[cfg(windows)]
    let vars = std::env::var("SystemRoot")
        .ok()
        .map(|root| ("SystemRoot".to_string(), root));
    #[cfg(not(windows))]
    let vars: Option<(String, String)> = None;
    vars.into_iter()
}

/// Keeps the first [`OUTPUT_CAP`] bytes and drains the rest, so a chatty
/// program neither fills memory nor blocks on a full pipe. With a `live` sink,
/// each line is also shown as the call's progress as it arrives.
async fn capped(
    mut reader: impl AsyncRead + Unpin,
    live: Option<&watch::Sender<Option<PluginRunProgress>>>,
) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut line = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => return kept,
            Ok(read) => {
                let room = OUTPUT_CAP - kept.len();
                kept.extend_from_slice(&chunk[..read.min(room)]);
                if let Some(live) = live {
                    show_lines(live, &mut line, &chunk[..read]);
                }
            }
        }
    }
}

/// Shows each line completed by `bytes`, keeping the unfinished one in `line`.
fn show_lines(live: &watch::Sender<Option<PluginRunProgress>>, line: &mut Vec<u8>, bytes: &[u8]) {
    for &byte in bytes {
        if byte != b'\n' {
            if line.len() < LIVE_LINE_BYTES {
                line.push(byte);
            }
            continue;
        }
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if !text.is_empty() {
            live.send_replace(Some(PluginRunProgress {
                fraction: None,
                message: text.to_string(),
            }));
        }
        line.clear();
    }
}
