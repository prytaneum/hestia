//! The program is opened without write or delete sharing, and the handle stays
//! open until the process has started, so the file cannot be replaced or
//! changed between the hash and the launch.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::Path;

use tokio::process::Command;

use crate::grant::{hash, open};

pub(super) struct Held(#[allow(dead_code)] File);

pub(super) fn hold(path: &Path) -> io::Result<(Held, String)> {
    let file = open(path)?;
    let digest = hash(&file, None)?;
    Ok((Held(file), digest))
}

/// What most programs cannot start without; TEMP and TMP point into the
/// plugin's own directory rather than the user's.
const SYSTEM: [&str; 5] = ["SystemRoot", "SystemDrive", "windir", "ComSpec", "PATHEXT"];

/// The working directory is the plugin's `/data`, which the plugin writes.
/// Windows looks for a program named without a path there first, in
/// `CreateProcess` and in `cmd.exe`, unless this variable is set; children
/// inherit it.
const NO_EXE_SEARCH_IN_CWD: &str = "NoDefaultCurrentDirectoryInExePath";

impl Held {
    pub fn command(&self, path: &str, data: &Path) -> Command {
        let mut command = Command::new(path);
        command.env_clear();
        for name in SYSTEM {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let temp: OsString = data.as_os_str().to_owned();
        command
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .env(NO_EXE_SEARCH_IN_CWD, "1");
        command
    }
}
