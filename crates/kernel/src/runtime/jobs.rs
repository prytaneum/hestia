use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub use super::bindings::exports::hestia::kernel::on_job_done::Outcome as JobOutcome;
use super::bindings::hestia::kernel::jobs;
use super::bindings::hestia::kernel::types::PluginError;
use super::host::{Call, State};

/// What the host says about a command's call.
#[derive(Default)]
pub struct Invocation {
    /// Whether a person asked for it; only then may it ask them to confirm.
    pub person: bool,
    /// The job it runs in: its progress reports belong to that job.
    pub job: Option<String>,
    /// Raised when the user cancels the job; the plugin sees it through
    /// `progress.cancelled`.
    pub cancelled: Arc<AtomicBool>,
}

impl Invocation {
    pub(super) fn into_call(self) -> Call {
        Call {
            person: self.person,
            job: self.job,
            cancelled: self.cancelled,
            ..Call::default()
        }
    }
}

impl jobs::Host for State {
    fn start(&mut self, command: String, args: String) -> Result<String, PluginError> {
        if !self.shared.wakeups.spend(&self.plugin) {
            return Err(PluginError::Busy);
        }
        self.shared
            .services
            .start_job(&self.plugin, &command, &args)
    }
}
