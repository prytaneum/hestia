#![no_main]

use hestia::log::{write, Level};
use hestia::{Error, Lifecycle};

struct Chatty;

impl Lifecycle for Chatty {
    fn init() -> Result<(), Error> {
        for line in 0..1500 {
            write(Level::Info, &format!("chatty line {line}"));
        }
        Ok(())
    }
}

hestia::export!(Chatty);
