use std::future::Future;
use std::pin::Pin;

use wasmtime::component::{Instance, InstancePre};
use wasmtime::Store;

use super::error::GuestError;
use super::host::State;

/// The wall clock an extension point's call gets, by its kind.
pub(crate) enum Budget {
    /// As long as an event such as `settings-changed`.
    Event,
}

pub(crate) type Outcome<'a> =
    Pin<Box<dyn Future<Output = wasmtime::Result<Result<(), GuestError>>> + Send + 'a>>;

/// One call into an export a built-in's package defines. The adapter looks the
/// export up per interface in the instance, calls it, and keeps what it
/// returns; the sandbox supplies the slot, store, budget and trap accounting.
pub(crate) trait Extension: Send + Sync {
    fn budget(&self) -> Budget;

    fn call<'a>(
        &'a self,
        store: &'a mut Store<State>,
        instance: &'a Instance,
        pre: &'a InstancePre<State>,
    ) -> Outcome<'a>;
}
