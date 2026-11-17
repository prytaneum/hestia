use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::process::{Child, Command};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

/// The program starts suspended, so that [`Tree::of`] can put it in its job
/// before it runs a single instruction and nothing it starts escapes.
pub(in crate::runtime) fn isolate(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
}

/// The processes of one run. Dropping it ends them, unless they were ended
/// already: the job is terminated once.
pub(in crate::runtime) struct Tree(HANDLE, AtomicBool);

unsafe impl Send for Tree {}

impl Tree {
    pub fn of(child: &Child) -> io::Result<Tree> {
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

    /// Resumes the suspended program. std does not hand out the thread of a
    /// process it created, so the process's only thread is found by snapshot.
    pub fn start(&self, child: &Child) -> io::Result<()> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the program already ended"))?;
        let mut resumed = false;
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut more = Thread32First(snapshot, &mut entry);
            while more != 0 {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        resumed |= ResumeThread(thread) != u32::MAX;
                        CloseHandle(thread);
                    }
                }
                more = Thread32Next(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        if resumed {
            Ok(())
        } else {
            Err(io::Error::other("the program could not be resumed"))
        }
    }

    pub fn kill(&self) {
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
        unsafe {
            CloseHandle(self.0);
        }
    }
}
