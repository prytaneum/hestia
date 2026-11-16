#![no_main]

use hestia::{download, ipc, programs};
use hestia::{Error, Lifecycle, SettingsChanged};

struct Reach;

fn refused<T>(result: Result<T, Error>, permission: &str, what: &str) -> Result<(), Error> {
    match result {
        Err(Error::Denied(named)) if named == permission => Ok(()),
        _ => Err(Error::Other(format!("{what} was not refused as {permission}"))),
    }
}

impl Lifecycle for Reach {
    fn init() -> Result<(), Error> {
        let tool = std::fs::read_to_string("/data/tool").unwrap_or_default();
        let zeros = "0".repeat(64);
        refused(
            download::fetch("https://evil.example/a", "a", Some(&zeros)),
            "network:connect",
            "a host outside the grant",
        )?;
        refused(
            download::fetch("https://93.184.216.34/a", "a", Some(&zeros)),
            "network:connect",
            "an IP literal",
        )?;
        refused(
            download::fetch("https://files.example/a", "a", None),
            "network:connect",
            "an unhashed file from a host not declared for it",
        )?;
        refused(
            programs::run("/bin/sh", &["-c".to_owned(), "id".to_owned()]),
            "process:spawn",
            "a program outside the grant",
        )?;
        refused(
            programs::run(tool.trim(), &["--undeclared".to_owned()]),
            "process:spawn",
            "an undeclared invocation of a granted program",
        )?;
        refused(
            ipc::connect("/run/not-granted.sock"),
            "ipc:connect",
            "an endpoint outside the grant",
        )
    }
}

impl SettingsChanged for Reach {
    fn changed(_key: String, _value: String) -> Result<(), Error> {
        Ok(())
    }
}

hestia::export!(Reach);
