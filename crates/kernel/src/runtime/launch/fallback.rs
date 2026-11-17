//! shortcut: a unix without `memfd` and `/proc/self/fd` (macOS) cannot execute
//! a descriptor, so the program still starts by path. The file is only checked
//! to be the very one that was hashed, right before the start; a swap in that
//! instant gets through.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use tokio::process::Command;

use crate::grant::{hash, open};

pub(super) struct Held;

pub(super) fn hold(path: &Path) -> io::Result<(Held, String)> {
    let file = open(path)?;
    let digest = hash(&file, None)?;
    let (opened, listed) = (file.metadata()?, std::fs::metadata(path)?);
    if (opened.dev(), opened.ino()) != (listed.dev(), listed.ino()) {
        return Err(io::Error::other("the program changed while it was read"));
    }
    Ok((Held, digest))
}

impl Held {
    pub fn command(&self, path: &str, _data: &Path) -> Command {
        let mut command = Command::new(path);
        command.env_clear();
        command
    }
}
