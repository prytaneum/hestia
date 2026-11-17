//! The program is copied into a sealed `memfd` while it is hashed, and the copy
//! is what runs, through `/proc/self/fd`: nothing that happens to the file on
//! disk after it was read, a rename or a write in place, reaches the process.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::path::Path;

use tokio::process::Command;

use crate::grant::{hash, open};

pub(super) struct Held(File);

pub(super) fn hold(path: &Path) -> io::Result<(Held, String)> {
    let source = open(path)?;
    let mut copy = memfd()?;
    let digest = hash(&source, Some(&mut copy))?;
    let seals = libc::F_SEAL_SEAL | libc::F_SEAL_SHRINK | libc::F_SEAL_GROW | libc::F_SEAL_WRITE;
    if unsafe { libc::fcntl(copy.as_raw_fd(), libc::F_ADD_SEALS, seals) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((Held(copy), digest))
}

fn memfd() -> io::Result<File> {
    let fd = unsafe {
        libc::memfd_create(
            c"hestia-program".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

impl Held {
    pub fn command(&self, path: &str, _data: &Path) -> Command {
        let fd: RawFd = self.0.as_raw_fd();
        let mut command = Command::new(format!("/proc/self/fd/{fd}"));
        command.arg0(path).env_clear();
        // The descriptor is close-on-exec so no other spawn inherits it; this
        // child alone keeps it open, because an interpreter reopens the path.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command
    }
}
