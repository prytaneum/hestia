//! The test built-in. It owns `hestia:probe` (`wit/probe.wit`) the way a real
//! built-in owns its package: a table, an adapter, contracts and an extension
//! point, so tests of the mechanism (and of what is built on it) need no real
//! feature. Compiled only with the `probe` feature.

mod bindings;
mod calls;
mod contract;
mod convert;
mod exports;
mod host;
mod table;

use crate::builtin::{Builtin, Busy, Descriptor};
use crate::grant::Table;

pub use calls::{ProbeCalls, Recorded};
pub use contract::{Echo, EchoParams, Echoed, Shout};
pub(crate) use exports::check_exports;
pub use exports::FILLER;
pub(crate) use host::link;

pub const ID: &str = "probe";
pub const PACKAGE: &str = "hestia:probe";

pub struct Probe;

impl Builtin for Probe {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: ID,
            name: "Probe",
            provides: Some(PACKAGE),
            requires: &[],
        }
    }

    fn table(&self) -> Option<Table> {
        Some(table::table())
    }

    fn enable(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn disable(&self) -> Result<(), Busy> {
        Ok(())
    }
}
