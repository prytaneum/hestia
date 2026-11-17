//! Hears `entry-opened` by noting the entry in `/data/heard`, and customises
//! `before-echo`: text ending in `!` is vetoed, `fail` errors, `trap` traps,
//! anything else gets a `!`.

#![no_main]

::hestia::wit_bindgen::generate!({
    path: ["../../../sdk/wit/kernel", "../../src/api/probe/wit"],
    world: "hestia:probe/listener",
    runtime_path: "::hestia::wit_bindgen::rt",
    generate_all,
});

use std::io::Write;

use crate::exports::hestia::probe::before_echo::{self, Verdict};
use crate::exports::hestia::probe::entry_opened;
use crate::hestia::kernel::types::PluginError;

struct Events;

impl entry_opened::Guest for Events {
    fn heard(entry: String) -> Result<(), PluginError> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/data/heard")
            .map_err(|e| PluginError::Other(e.to_string()))?;
        writeln!(file, "{entry}").map_err(|e| PluginError::Other(e.to_string()))
    }
}

impl before_echo::Guest for Events {
    fn check(text: String) -> Result<Verdict, PluginError> {
        match text.as_str() {
            "fail" => Err(PluginError::Failed("asked to".into())),
            "trap" => panic!("asked to"),
            _ if text.ends_with('!') => Ok(Verdict::Veto("no shouting".into())),
            _ => Ok(Verdict::Proceed(format!("{text}!"))),
        }
    }
}

export!(Events);
