#![no_main]

use hestia::{settings, Error, Lifecycle};

struct Settler;

impl Lifecycle for Settler {
    fn init() -> Result<(), Error> {
        let _ = settings::get("probe");
        Ok(())
    }
}

hestia::export!(Settler);
