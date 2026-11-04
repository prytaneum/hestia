use hestia::proto::{server::ServerList, Empty};
use hestia::{Error, Plugin};

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
}

hestia::plugin!(Probe);
