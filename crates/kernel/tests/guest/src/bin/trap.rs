#![no_main]

use hestia::{Error, Lifecycle};

struct Trap;

impl Lifecycle for Trap {
    fn init() -> Result<(), Error> {
        panic!("guest trap");
    }
}

hestia::export!(Trap);
