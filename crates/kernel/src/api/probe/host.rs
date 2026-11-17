use wasmtime::component::{HasSelf, Linker};

use super::bindings::hestia::probe::{entries, read, work};
use super::contract::{Build, Create, Echo, EchoParams, EntryRef, Open, Shout};
use super::table::{ENTRIES, READ};
use crate::runtime::{GuestError, SandboxError, State};

impl read::Host for State {
    async fn echo(&mut self, text: String) -> Result<read::Echoed, GuestError> {
        let link = self.link(Some(READ))?;
        Ok(link.call::<Echo>(&EchoParams { text }).await?.into())
    }
}

impl work::Host for State {
    async fn shout(&mut self, text: String) -> Result<read::Echoed, GuestError> {
        let link = self.link(None)?;
        Ok(link.run_job::<Shout>(&EchoParams { text }).await?.into())
    }
}

impl entries::Host for State {
    async fn open(&mut self, id: String) -> Result<String, GuestError> {
        let link = self.link(Some(ENTRIES))?;
        Ok(link.call::<Open>(&EntryRef { id }).await?.id)
    }

    async fn create(&mut self, id: String) -> Result<String, GuestError> {
        let link = self.link(Some(ENTRIES))?;
        Ok(link.call::<Create>(&EntryRef { id }).await?.id)
    }

    async fn build(&mut self, id: String) -> Result<String, GuestError> {
        let link = self.link(Some(ENTRIES))?;
        Ok(link.run_job::<Build>(&EntryRef { id }).await?.id)
    }
}

/// Only the package's own interfaces: the kernel's `types` is already linked.
pub(crate) fn link(linker: &mut Linker<State>) -> Result<(), SandboxError> {
    read::add_to_linker::<State, HasSelf<State>>(linker, |state| state)
        .and_then(|()| work::add_to_linker::<State, HasSelf<State>>(linker, |state| state))
        .and_then(|()| entries::add_to_linker::<State, HasSelf<State>>(linker, |state| state))
        .map_err(SandboxError::Engine)
}
