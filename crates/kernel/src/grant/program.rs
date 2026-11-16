use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// A `programs` scope entry: the absolute path of a program and the exact
/// arguments it may run with, separated by single spaces.
///
/// shortcut: an argument cannot contain a space, upgrade when a plugin needs one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
}

impl Invocation {
    pub fn parse(entry: &str) -> Option<Self> {
        let mut words = entry.split(' ');
        let program = words.next().filter(|p| Path::new(p).is_absolute())?;
        let args: Vec<String> = words.map(str::to_owned).collect();
        if args.iter().any(String::is_empty) {
            return None;
        }
        Some(Invocation {
            program: program.to_owned(),
            args,
        })
    }
}

/// The SHA-256 of the file at `path`, in lowercase hex.
pub fn digest(path: &Path) -> io::Result<String> {
    let mut hash = Sha256::new();
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer)? {
            0 => break,
            read => hash.update(&buffer[..read]),
        }
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
