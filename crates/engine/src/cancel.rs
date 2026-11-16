//! Cooperative cancellation for long-running engine work.
//!
//! A job is cancelled **at safe checkpoints**, never by killing anything
//! mid-write. Every long operation here already writes through a staging
//! artifact and commits with a rename, so the checkpoints are the boundaries
//! that discipline already created: between files in a batch, between chunks of
//! a download, between phases of a pipeline. Stopping at one leaves exactly what
//! stopping there would have left had the network failed — which the failure
//! paths already handle, and which `Engine::recover()` reclaims.
//!
//! The token rides with the progress reporter rather than as a second parameter:
//! the two travel to the same places for the same reason, and a function that
//! reports progress is by definition one that can be cancelled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use proto::minecraft::ProvisionProgress;

/// The error a cancelled operation returns. Distinguished from a failure by
/// [`is_cancelled`], so a caller can publish "cancelled" rather than "error".
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// Whether this error chain is a cancellation rather than a genuine failure.
pub fn is_cancelled(error: &anyhow::Error) -> bool {
    error.chain().any(|e| e.is::<Cancelled>())
}

/// A shared cancellation flag. Cloned into the job's worker; the daemon keeps a
/// handle so `job.cancel` can raise it.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Cancel::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// The raw flag, for a callee that polls it without knowing this type.
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.0.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// A safe checkpoint for work that carries the bare token rather than a
    /// whole [`Job`] — the Java installer, whose progress type is its own.
    pub fn check(&self) -> anyhow::Result<()> {
        match self.is_cancelled() {
            true => Err(Cancelled.into()),
            false => Ok(()),
        }
    }
}

/// What a long-running engine operation reports through and is cancelled by.
/// One parameter, because a step that reports progress is exactly a step that
/// can be stopped between.
pub struct Job<'a> {
    report: &'a (dyn Fn(&ProvisionProgress) + Send + Sync),
    cancel: &'a Cancel,
}

impl<'a> Job<'a> {
    pub fn new(report: &'a (dyn Fn(&ProvisionProgress) + Send + Sync), cancel: &'a Cancel) -> Self {
        Job { report, cancel }
    }

    pub fn report(&self, progress: &ProvisionProgress) {
        (self.report)(progress)
    }

    /// The token behind this job, so a wrapper that relabels progress (a batch
    /// tagging which item it is on) stays cancellable — losing the token there
    /// would leave the longest-running part of a batch uninterruptible.
    pub fn cancel(&self) -> &'a Cancel {
        self.cancel
    }

    /// A safe checkpoint: returns `Cancelled` when the job has been cancelled,
    /// so the caller unwinds through its ordinary failure path — which already
    /// cleans up.
    pub fn check(&self) -> anyhow::Result<()> {
        match self.cancel.is_cancelled() {
            true => Err(Cancelled.into()),
            false => Ok(()),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }
}
