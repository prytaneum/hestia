#![no_main]

use hestia::{Error, Lifecycle};

struct Fit;

impl Lifecycle for Fit {
    fn init() -> Result<(), Error> {
        let big = vec![1u8; 240 << 20];
        std::hint::black_box(&big);
        Ok(())
    }
}

hestia::export!(Fit);
