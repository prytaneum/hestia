use std::time::Duration;

use hestia::proto::{server::ServerList, Empty};
use hestia::{json, Error, Plugin, Value};

struct Probe;

impl Plugin for Probe {
    fn init() -> Result<(), Error> {
        match hestia::data::load::<String>("mode")?.as_deref() {
            Some("list-servers") => hestia::call::<ServerList>(&Empty {}).map(drop),
            Some("exhaust-memory") => {
                let hoard = vec![1u8; 512 << 20];
                std::hint::black_box(&hoard);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn run(command: &str, args: Value) -> Result<Value, Error> {
        match command {
            "echo" => Ok(args),
            "progress" => {
                hestia::progress::report(Some(0.5), "halfway");
                hestia::progress::report(None, "finishing");
                Ok(json!({ "cancelled": hestia::progress::cancelled() }))
            }
            "wait" => {
                while !hestia::progress::cancelled() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(Error::Failed("stopped".into()))
            }
            "stubborn" => loop {
                std::thread::sleep(Duration::from_millis(10));
            },
            _ => Err(Error::NotFound(format!("unknown command `{command}`"))),
        }
    }
}

hestia::plugin!(Probe);
