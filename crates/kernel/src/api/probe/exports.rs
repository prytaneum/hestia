use std::sync::Mutex;

use wasmtime::component::{Component, Instance, InstancePre};
use wasmtime::Store;

use super::bindings::exports::hestia::probe::filler::GuestIndices;
use super::bindings::exports::hestia::probe::{before_echo, echoed, entry_opened};
use super::events::{BEFORE_ECHO, ECHOED, ENTRY_OPENED};
use crate::api::Chain;
use crate::runtime::extension::{Budget, Extension, Outcome};
use crate::runtime::{Loaded, Sandbox, SandboxError, State};

pub const FILLER: &str = "hestia:probe/filler@0.1.0";

struct Fill<'a> {
    text: &'a str,
    filled: Mutex<Option<String>>,
}

impl Extension for Fill<'_> {
    fn budget(&self) -> Budget {
        Budget::Event
    }

    fn call<'a>(
        &'a self,
        store: &'a mut Store<State>,
        instance: &'a Instance,
        pre: &'a InstancePre<State>,
    ) -> Outcome<'a> {
        Box::pin(async move {
            let filler = GuestIndices::new(pre)?.load(&mut *store, instance)?;
            let filled = filler.call_fill(&mut *store, self.text).await?;
            Ok(filled.map(|text| {
                *self.filled.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
            }))
        })
    }
}

/// A `filler` export of the wrong type fails the load.
pub(crate) fn check_exports(
    component: &Component,
    pre: &InstancePre<State>,
) -> Result<(), SandboxError> {
    if component.get_export_index(None, FILLER).is_some() {
        GuestIndices::new(pre).map_err(SandboxError::Load)?;
    }
    if component.get_export_index(None, ENTRY_OPENED).is_some() {
        entry_opened::GuestIndices::new(pre).map_err(SandboxError::Load)?;
    }
    if component.get_export_index(None, ECHOED).is_some() {
        echoed::GuestIndices::new(pre).map_err(SandboxError::Load)?;
    }
    if component.get_export_index(None, BEFORE_ECHO).is_some() {
        before_echo::GuestIndices::new(pre).map_err(SandboxError::Load)?;
    }
    Ok(())
}

impl Sandbox {
    /// Fills the extension point through `loaded`'s `filler` export, or answers
    /// `None` when the component does not export it.
    pub async fn fill(
        &self,
        loaded: &Loaded,
        chain: Chain,
        text: &str,
    ) -> Result<Option<String>, SandboxError> {
        if !loaded.exports(FILLER) {
            return Ok(None);
        }
        let fill = Fill {
            text,
            filled: Mutex::new(None),
        };
        self.extend(loaded, chain, &fill).await?;
        Ok(fill.filled.into_inner().unwrap_or_else(|e| e.into_inner()))
    }
}
