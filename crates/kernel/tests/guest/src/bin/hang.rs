#![no_main]

use hestia::{Error, Lifecycle};

struct Hang;

impl Lifecycle for Hang {
    fn init() -> Result<(), Error> {
        std::thread::sleep(std::time::Duration::from_secs(3600));
        Ok(())
    }
}

hestia::export!(Hang);
