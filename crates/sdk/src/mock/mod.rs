//! The mock host: every `hestia:kernel` import on native builds, answered from
//! memory and checked against the grant a test sets, so a call the grant lacks
//! answers `denied(<permission>)` in a unit test instead of failing at install.
//! The permission of each gated interface is read from its WIT doc ("Permission
//! `x`"), so the mock cannot name one the WIT does not.
//!
//! Calls that succeed do the least that is observable: `download::fetch` writes
//! nothing, `programs::run` exits 0 with no output, an `ipc` connection reads
//! end-of-stream, and `settings::set` does not check that the value is JSON.
//!
//! What the mock does not model of the host:
//!
//! - State and the grant are per thread, which is per test. A plugin that runs on
//!   another thread (a multi-thread runtime, `std::thread::spawn`) sees nothing
//!   granted and gets `denied`.
//! - `notify::confirm` answers what [`confirm`] says, not `unsupported` in a call
//!   no person started; notices, links and `open_url` have no budget or check.
//! - `app::info().online` is always true; there is no offline mode.
//! - Locks are not released when a call ends: call [`reset`] between calls.
//! - `permissions::entries` answers what [`entries`] set, unchecked.
//! - There is no `/data`: `std::fs` on `/data/...` touches the real filesystem.
//! - An `ipc` permission left out of the real grant fails the plugin's load
//!   there; here the call answers `denied`.

pub(crate) mod imports;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use crate::base::hestia::kernel::log::Level;
use crate::base::hestia::kernel::permissions::EntryScope;
use crate::Error;

/// The WIT of each gated interface, whose doc names its permission.
const GATED: [(&str, &str); 4] = [
    (
        "hestia:kernel/secrets",
        include_str!("../../wit/kernel/secrets.wit"),
    ),
    (
        "hestia:kernel/download",
        include_str!("../../wit/kernel/download.wit"),
    ),
    (
        "hestia:kernel/programs",
        include_str!("../../wit/kernel/programs.wit"),
    ),
    (
        "hestia:kernel/ipc",
        include_str!("../../wit/kernel/ipc.wit"),
    ),
];

/// What the grant's scopes allow, as the user approved them.
#[derive(Default, Clone, Debug)]
pub struct Scope {
    /// Hosts `download::fetch` may reach: exact names or `*.domain`, which
    /// matches exactly one label below `domain`.
    pub hosts: Vec<String>,
    /// Those of `hosts` that may serve a file with no checksum.
    pub unhashed: Vec<String>,
    /// The invocations `programs::run` may make.
    pub programs: Vec<Program>,
    /// The exact socket paths or pipe names `ipc::connect` may open.
    pub endpoints: Vec<String>,
}

/// One approved invocation: the path and the exact arguments, optionally for one
/// OS (`"linux"`, `"windows"`, `"macos"`).
#[derive(Clone, Debug)]
pub struct Program {
    pub path: String,
    pub args: Vec<String>,
    pub os: Option<String>,
}

#[derive(Default)]
struct State {
    granted: HashSet<String>,
    scope: Scope,
    entries: Option<EntryScope>,
    calls: Vec<String>,
    logs: Vec<(Level, String)>,
    settings: BTreeMap<String, String>,
    secrets: BTreeMap<String, String>,
    locks: HashSet<String>,
    confirm: bool,
    cancelled: bool,
    jobs: u32,
}

thread_local! {
    static HOST: RefCell<State> = RefCell::new(State::default());
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    HOST.with(|host| f(&mut host.borrow_mut()))
}

fn note(interface: &str, function: &str) {
    with(|host| host.calls.push(format!("{interface}#{function}")));
}

fn permission_of(wit: &str) -> Option<&str> {
    wit.split("Permission `").nth(1)?.split('`').next()
}

fn gate(interface: &str, function: &str) -> Result<(), Error> {
    note(interface, function);
    let needed = GATED
        .iter()
        .find(|(name, _)| *name == interface)
        .and_then(|(_, wit)| permission_of(wit));
    match needed {
        Some(permission) if !with(|host| host.granted.contains(permission)) => {
            Err(Error::Denied(permission.to_owned()))
        }
        _ => Ok(()),
    }
}

fn denied(interface: &str) -> Error {
    let wit = GATED.iter().find(|(name, _)| *name == interface);
    Error::Denied(
        wit.and_then(|(_, wit)| permission_of(wit))
            .unwrap_or_default()
            .to_owned(),
    )
}

/// Starts again from nothing granted and no history.
pub fn reset() {
    with(|host| *host = State::default());
}

/// Sets the grant: the permissions the plugin holds (`"secrets"`,
/// `"network:connect"`) and the scopes of the ones that take any. State is per
/// thread: see the module docs.
pub fn grant(permissions: &[&str], scope: Scope) {
    with(|host| {
        host.granted = permissions.iter().map(|name| (*name).to_owned()).collect();
        host.scope = scope;
    });
}

/// Sets what `permissions::entries` answers; none by default.
pub fn entries(scope: EntryScope) {
    with(|host| host.entries = Some(scope));
}

/// What `notify::confirm` answers; declined by default.
pub fn confirm(answer: bool) {
    with(|host| host.confirm = answer);
}

/// What `progress::cancelled` answers.
pub fn cancel(cancelled: bool) {
    with(|host| host.cancelled = cancelled);
}

/// Every import called so far, as `interface#function`.
pub fn calls() -> Vec<String> {
    with(|host| host.calls.clone())
}

/// Every line written to `log::write`.
pub fn logs() -> Vec<(Level, String)> {
    with(|host| host.logs.clone())
}
