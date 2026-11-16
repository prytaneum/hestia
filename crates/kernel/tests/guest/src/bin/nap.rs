#![no_main]

use hestia::{Error, Lifecycle};

struct Nap;

impl Lifecycle for Nap {
    fn init() -> Result<(), Error> {
        std::thread::sleep(std::time::Duration::from_millis(50));
        Ok(())
    }
}

hestia::export!(Nap);
