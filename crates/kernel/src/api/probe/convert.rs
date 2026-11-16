use super::bindings::hestia::probe::read;
use super::contract::Echoed;

impl From<Echoed> for read::Echoed {
    fn from(echoed: Echoed) -> Self {
        read::Echoed {
            text: echoed.text,
            length: echoed.length,
        }
    }
}
