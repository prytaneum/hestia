#![no_main]

use hestia::{Error, Lifecycle};

struct Spin;

impl Lifecycle for Spin {
    fn init() -> Result<(), Error> {
        let mut n = 0u64;
        loop {
            n = std::hint::black_box(n.wrapping_add(1));
        }
    }
}

hestia::export!(Spin);
