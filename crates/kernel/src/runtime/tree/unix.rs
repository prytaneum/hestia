use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::process::{Child, Command};

/// Puts the program in a group of its own, so the tree can be signalled.
pub(in crate::runtime) fn isolate(command: &mut Command) {
    command.process_group(0);
}

/// The processes of one run. Dropping it ends them, unless they were ended
/// already: the group is signalled once.
pub(in crate::runtime) struct Tree(libc::pid_t, AtomicBool);

impl Tree {
    pub fn of(child: &Child) -> io::Result<Tree> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the program already ended"))?;
        Ok(Tree(pid as libc::pid_t, AtomicBool::new(false)))
    }

    pub fn start(&self, _child: &Child) -> io::Result<()> {
        Ok(())
    }

    /// shortcut: once the whole group has ended and been reaped its id could be
    /// reused; a call is short and the window is tiny, so no pidfd.
    pub fn kill(&self) {
        if self.1.swap(true, Ordering::Relaxed) {
            return;
        }
        unsafe {
            libc::killpg(self.0, libc::SIGKILL);
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
    }
}
