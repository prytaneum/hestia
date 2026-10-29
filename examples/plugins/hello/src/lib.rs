use hestia::{Error, Plugin};

struct Hello;

impl Plugin for Hello {
    fn init() -> Result<(), Error> {
        hestia::log::info("hello from a plugin");
        hestia::data::save("greeting", &"hello")
    }
}

hestia::plugin!(Hello);
