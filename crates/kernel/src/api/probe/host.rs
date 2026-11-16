use wasmtime::component::{HasSelf, Linker};

use super::bindings::hestia::probe::{read, work};
use super::contract::{Echo, EchoParams, Shout};
use crate::runtime::{GuestError, SandboxError, State};

impl read::Host for State {
    async fn echo(&mut self, text: String) -> Result<read::Echoed, GuestError> {
        let link = self.link()?;
        Ok(link.call::<Echo>(&EchoParams { text }).await?.into())
    }
}

impl work::Host for State {
    async fn shout(&mut self, text: String) -> Result<read::Echoed, GuestError> {
        let link = self.link()?;
        Ok(link.run_job::<Shout>(&EchoParams { text }).await?.into())
    }
}

/// Only the package's own interfaces: the kernel's `types` is already linked.
pub(crate) fn link(linker: &mut Linker<State>) -> Result<(), SandboxError> {
    read::add_to_linker::<State, HasSelf<State>>(linker, |state| state)
        .and_then(|()| work::add_to_linker::<State, HasSelf<State>>(linker, |state| state))
        .map_err(SandboxError::Engine)
}
