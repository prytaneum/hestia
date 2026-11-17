//! A program and everything it starts, ended together: a process group on
//! unix, a job object on Windows.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Puts the program in a group of its own, so the tree can be signalled.
pub(super) fn isolate(command: &mut Command) {
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
}

/// The processes of one run. Dropping it ends them, unless they were ended
/// already: the group is signalled once.
pub(super) struct Tree(Handle, AtomicBool);

#[cfg(unix)]
type Handle = libc::pid_t;
#[cfg(windows)]
type Handle = windows_sys::Win32::Foundation::HANDLE;

#[cfg(windows)]
unsafe impl Send for Tree {}

#[cfg(unix)]
impl Tree {
    pub fn of(child: &Child) -> io::Result<Tree> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the program already ended"))?;
        Ok(Tree(pid as libc::pid_t, AtomicBool::new(false)))
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

#[cfg(windows)]
impl Tree {
    /// shortcut: the program runs for an instant before it joins the job, so a
    /// child it starts in that instant escapes; upgrade by starting it suspended.
    pub fn of(child: &Child) -> io::Result<Tree> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("the program already ended"))?;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Tree(job, AtomicBool::new(false));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                tree.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_mut(&mut limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 || AssignProcessToJobObject(tree.0, process as _) == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(tree)
        }
    }

    pub fn kill(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        if self.1.swap(true, Ordering::Relaxed) {
            return;
        }
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        self.kill();
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
