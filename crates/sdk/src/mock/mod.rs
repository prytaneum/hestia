//! The mock host: every `hestia:kernel` import on native builds, answered from
//! memory and checked against the grant a test sets, so a call the grant lacks
//! answers `denied(<permission>)` in a unit test instead of failing at install.
//!
//! State is per thread, which is per test. Calls that succeed do the least that
//! is observable: `download::fetch` writes nothing, `programs::run` exits 0 with
//! no output, an `ipc` connection reads end-of-stream, and `settings::set` does
//! not check that the value is JSON.

mod imports;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use crate::base::hestia::kernel::log::Level;
use crate::base::hestia::kernel::permissions::EntryScope;
use crate::Error;

pub use imports::{
    app, data, download, ipc, jobs, log, messages, notify, permissions, programs, progress,
    secrets, settings,
};

/// `interface permission` per line: the permission a gated `hestia:kernel`
/// interface needs. `crates/kernel/tests/grant.rs` keeps it equal to the host's
/// permission table.
const PERMISSIONS: &str = include_str!("../../permissions.txt");

#[derive(Default)]
struct State {
    granted: HashSet<String>,
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

fn gate(interface: &str, function: &str) -> Result<(), Error> {
    note(interface, function);
    let needed = PERMISSIONS
        .lines()
        .filter_map(|line| line.split_once(' '))
        .find(|(name, _)| *name == interface)
        .map(|(_, permission)| permission);
    match needed {
        Some(permission) if !with(|host| host.granted.contains(permission)) => {
            Err(Error::Denied(permission.to_owned()))
        }
        _ => Ok(()),
    }
}

/// Starts again from nothing granted and no history.
pub fn reset() {
    with(|host| *host = State::default());
}

/// Sets the grant: the permissions the plugin holds (`"secrets"`, `"network:connect"`).
pub fn grant(permissions: &[&str]) {
    with(|host| host.granted = permissions.iter().map(|name| (*name).to_owned()).collect());
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
