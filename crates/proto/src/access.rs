//! How a contract a plugin may call reaches servers and instances. A built-in's
//! adapter can only call a contract that states it, with no default, so none can
//! forget: one that touches nothing returns an empty list on purpose. The
//! daemon resolves each target to the entry's stable id before it checks the
//! plugin's `entries` scope.

use crate::contract::Contract;
use crate::error::EntryTarget;
use crate::job::Job;

pub trait EntryAccess: Contract {
    /// The entries a call acts on. The router refuses the call unless every one
    /// is inside the scope of every plugin in the call chain.
    fn touches(params: &Self::Params) -> Vec<EntryTarget>;

    /// The entries a successful call made. They join the calling plugin's scope.
    fn creates(result: &Self::Result) -> Vec<EntryTarget>;
}

/// A job start channel's reach: what the job made, known when it is done.
/// [`EntryAccess::creates`] of a job answers only the start, which makes none.
pub trait JobAccess: Job + EntryAccess {
    fn creates_done(done: &Self::Done) -> Vec<EntryTarget>;
}
