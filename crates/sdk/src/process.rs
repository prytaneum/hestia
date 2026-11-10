//! Run the programs the manifest declares.
//!
//! The plugin needs the `process:spawn` permission and lists every program in
//! `[[programs]]` with an `id`, a `command` (a name on `PATH`, or a path inside
//! `data/`, which also needs a `sha256` per target; or `java = <major>` instead
//! of a command, for the runtime Hestia manages), the `args` patterns it may
//! be called with (a literal, `*` for one token, a trailing `**` for the rest)
//! and the `env` names it may be given. A program, argument list or variable
//! the manifest did not declare is [`Error::PermissionDenied`]. There is no
//! shell: the working directory is `data/` unless [`exec_in`] names another, and
//! the environment starts empty.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! let status = hestia::process::exec("tailscale", &["status", "--json"], Duration::from_secs(10))?;
//! let agent = hestia::process::spawn("agent", &["run"], &[("TS_AUTHKEY", "secret")])?;
//! hestia::process::stop(&agent)?;
//! # Ok::<(), hestia::Error>(())
//! ```

use std::time::Duration;

use crate::bindings::hestia::plugin::process as host;
use crate::Error;

/// What a finished program left behind. Each stream is cut at 1 MiB.
#[derive(Debug, Clone)]
pub struct Output {
    /// `None` when the program was ended by a signal.
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Running,
    Exited,
    Killed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    pub exit_code: Option<i32>,
}

/// Runs a program to completion; one that outlasts `timeout`, or what the
/// current call has left, is killed with everything it started. An argument
/// may hold `${build}`, `${vanilla}` or `${data}`, replaced by the host path
/// of that mount of this call.
pub fn exec(program: &str, args: &[&str], timeout: Duration) -> Result<Output, Error> {
    exec_with(program, args, &[], timeout, None)
}

/// [`exec`] in `cwd`, a guest path under a writable mount of this call: `/data`,
/// or `/build` while a flavor's `build` runs. Any other directory is
/// [`Error::PermissionDenied`].
pub fn exec_in(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cwd: &str,
) -> Result<Output, Error> {
    exec_with(program, args, &[], timeout, Some(cwd))
}

/// [`exec`] with `env`, which may only name variables the program declares, and
/// an optional `cwd` as in [`exec_in`]. The program's `PATH` holds only the
/// programs its manifest lists in `path`.
pub fn exec_with(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Duration,
    cwd: Option<&str>,
) -> Result<Output, Error> {
    let timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let output = host::exec(program, &strings(args), &pairs(env), timeout_ms, cwd)?;
    Ok(Output {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// Starts a long-lived program under the daemon's supervisor and returns its
/// process id, which stays valid after the daemon restarts. `env` may only
/// name variables the program declares.
pub fn spawn(program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<String, Error> {
    Ok(host::spawn(program, &strings(args), &pairs(env))?)
}

/// Asks one of the plugin's own processes to stop. A process the plugin did
/// not start is [`Error::NotFound`].
pub fn stop(id: &str) -> Result<(), Error> {
    Ok(host::stop(id)?)
}

pub fn status(id: &str) -> Result<Status, Error> {
    let status = host::status(id)?;
    Ok(Status {
        state: match status.state {
            host::State::Running => State::Running,
            host::State::Exited => State::Exited,
            host::State::Killed => State::Killed,
        },
        exit_code: status.exit_code,
    })
}

/// The last `tail` captured output lines of one of the plugin's processes.
pub fn logs(id: &str, tail: u32) -> Result<Vec<String>, Error> {
    Ok(host::logs(id, tail)?)
}

/// The ids of the plugin's own running processes, including ones started
/// before the daemon last restarted.
pub fn list() -> Result<Vec<String>, Error> {
    Ok(host::list()?)
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| arg.to_string()).collect()
}

fn pairs(env: &[(&str, &str)]) -> Vec<(String, String)> {
    env.iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}
