#![no_main]

use hestia::log::{write, Level};
use hestia::{app, Error, Lifecycle};

struct Hello;

impl Lifecycle for Hello {
    fn init() -> Result<(), Error> {
        let info = app::info();
        write(
            Level::Info,
            &format!("hello from the guest, {} {}", info.name, info.version),
        );
        Ok(())
    }
}

hestia::export!(Hello);
