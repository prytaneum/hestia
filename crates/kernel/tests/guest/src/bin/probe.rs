#![no_main]

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};

use hestia::{Error, Lifecycle};

struct Probe;

impl Lifecycle for Probe {
    fn init() -> Result<(), Error> {
        let env = std::env::vars().count();
        let root = std::fs::read_dir("/").is_ok();
        let tcp = TcpStream::connect("93.184.215.14:443")
            .map(drop)
            .map_err(|error| error.kind());
        let lookup = "example.com:443".to_socket_addrs().is_ok();
        if env == 0 && !root && tcp == Err(ErrorKind::PermissionDenied) && !lookup {
            return Ok(());
        }
        Err(Error::Other(format!(
            "env={env} root={root} tcp={tcp:?} lookup={lookup}"
        )))
    }
}

hestia::export!(Probe);
