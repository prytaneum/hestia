#![no_main]

use hestia::{download, secrets, Error, Lifecycle};

struct Net;

impl Lifecycle for Net {
    fn init() -> Result<(), Error> {
        if std::fs::metadata("/data/reach").is_ok() {
            download::fetch("https://a.example/x", "x", None)?;
            secrets::get("token")?;
        }
        Ok(())
    }
}

hestia::export!(Net);
