//! Uses `hestia:probe`: a typed call and a job function, and fills the `filler`
//! extension point by calling both.

#![no_main]

::hestia::wit_bindgen::generate!({
    path: ["../../../sdk/wit/kernel", "../../src/api/probe/wit"],
    world: "hestia:probe/host",
    runtime_path: "::hestia::wit_bindgen::rt",
    generate_all,
});

use crate::exports::hestia::probe::filler::Guest;
use crate::hestia::kernel::types::PluginError;
use crate::hestia::probe::{read, work};

struct Consumer;

impl Guest for Consumer {
    fn fill(text: String) -> Result<String, PluginError> {
        let echoed = read::echo(&text)?;
        let shouted = work::shout(&text)?;
        Ok(format!(
            "{}/{}/{}/{}",
            echoed.text, echoed.length, shouted.text, shouted.length
        ))
    }
}

export!(Consumer);
