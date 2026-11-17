//! Acts on entries through `hestia:probe` and reports what it was granted. The
//! text it is filled with says what to do: `open:ID`, `create:ID`, `build:ID`, `echo:TEXT`,
//! `granted:PERMISSION` or `scope`.

#![no_main]

::hestia::wit_bindgen::generate!({
    path: ["../../../sdk/wit/kernel", "../../src/api/probe/wit"],
    inline: "package test:entries; world entries { include hestia:probe/host@0.1.0; import hestia:kernel/permissions@0.1.0; }",
    world: "test:entries/entries",
    runtime_path: "::hestia::wit_bindgen::rt",
    generate_all,
});

use crate::exports::hestia::probe::filler::Guest;
use crate::hestia::kernel::permissions;
use crate::hestia::kernel::types::PluginError;
use crate::hestia::probe::{entries, read};

struct Entries;

impl Guest for Entries {
    fn fill(text: String) -> Result<String, PluginError> {
        match text.split_once(':') {
            Some(("open", id)) => entries::open(id),
            Some(("create", id)) => entries::create(id),
            Some(("build", id)) => entries::build(id),
            Some(("echo", text)) => Ok(read::echo(text)?.text),
            Some(("granted", permission)) => Ok(permissions::granted(permission).to_string()),
            _ => {
                let scope = permissions::entries();
                Ok(format!("{}:{}", scope.all, scope.ids.join(",")))
            }
        }
    }
}

export!(Entries);
