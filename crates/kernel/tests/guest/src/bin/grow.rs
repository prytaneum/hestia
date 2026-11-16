#![no_main]

use hestia::{Error, Lifecycle};

struct Grow;

impl Lifecycle for Grow {
    fn init() -> Result<(), Error> {
        let hog = vec![1u8; 512 << 20];
        std::hint::black_box(&hog);
        Ok(())
    }
}

hestia::export!(Grow);
