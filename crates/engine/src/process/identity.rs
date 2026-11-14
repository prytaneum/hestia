//! Platform probes for process identity and termination. The identity token is
//! a start-time value that is stable for one process and different for any
//! later process reusing its pid — the guard that re-adoption never grabs a
//! stranger.
//!
//! Termination addresses the whole tree: a supervised workload is routinely not
//! a leaf, so signalling the pid alone orphans its children. POSIX uses the
//! process group; Windows uses the job object held at the spawn site.

#[cfg(target_os = "linux")]
pub fn identify(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Fields after the last ')' start at field 3 (state); starttime is field 22.
    let rest = stat.rsplit_once(')')?.1;
    let mut fields = rest.split_whitespace();
    if fields.next()? == "Z" {
        return None;
    }
    fields.nth(18)?.parse().ok()
}

#[cfg(target_os = "macos")]
pub fn identify(pid: u32) -> Option<u64> {
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        pid as libc::c_int,
    ];
    let mut info = std::mem::MaybeUninit::<libc::kinfo_proc>::uninit();
    let mut size = std::mem::size_of::<libc::kinfo_proc>();
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            info.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size < std::mem::size_of::<libc::kinfo_proc>() {
        return None;
    }
    let info = unsafe { info.assume_init() };
    if info.kp_proc.p_pid != pid as libc::pid_t {
        return None;
    }
    let tv = info.kp_proc.p_starttime;
    Some((tv.tv_sec as u64) * 1_000_000 + tv.tv_usec as u64)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
pub fn identify(pid: u32) -> Option<u64> {
    // No start-time oracle wired for this platform: existence only, which
    // cannot rule out pid reuse.
    (unsafe { libc::kill(pid as libc::pid_t, 0) } == 0).then_some(1)
}

#[cfg(windows)]
pub fn identify(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut exit_code = 0u32;
        let running =
            GetExitCodeProcess(handle, &mut exit_code) != 0 && exit_code == STILL_ACTIVE as u32;
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut creation = zero;
        let (mut exit, mut kernel, mut user) = (zero, zero, zero);
        let timed = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
        CloseHandle(handle);
        if !running || !timed {
            return None;
        }
        Some(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
    }
}

pub fn is_same(pid: u32, token: u64) -> bool {
    token != 0 && identify(pid) == Some(token)
}

/// A spawned process and everything under it, addressed as one unit.
#[cfg(unix)]
pub struct Tree(u32);

#[cfg(unix)]
impl Tree {
    pub fn hold(child: &tokio::process::Child) -> Option<Tree> {
        child.id().map(Tree)
    }

    pub fn stop(&self) {
        signal_tree(self.0, libc::SIGTERM);
    }

    pub fn kill(&self) {
        signal_tree(self.0, libc::SIGKILL);
    }
}

#[cfg(unix)]
pub fn request_stop(pid: u32) {
    signal_tree(pid, libc::SIGTERM);
}

#[cfg(unix)]
pub fn kill(pid: u32) {
    signal_tree(pid, libc::SIGKILL);
}

/// Signal the group this process leads, falling back to the bare pid when it
/// leads none. pid 0 is skipped: negated it addresses every process on the
/// system, un-negated the daemon's own group.
#[cfg(unix)]
fn signal_tree(pid: u32, signal: libc::c_int) {
    if pid == 0 {
        return;
    }
    unsafe {
        if libc::kill(-(pid as libc::pid_t), signal) != 0 {
            libc::kill(pid as libc::pid_t, signal);
        }
    }
}

#[cfg(windows)]
pub fn request_stop(pid: u32) {
    terminate(pid);
}

#[cfg(windows)]
pub fn kill(pid: u32) {
    terminate(pid);
}

/// Windows has no process group that cascades a kill, so the tree is a job
/// object: its members die with the handle.
#[cfg(windows)]
pub struct Tree(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
unsafe impl Send for Tree {}
#[cfg(windows)]
unsafe impl Sync for Tree {}

#[cfg(windows)]
impl Tree {
    pub fn stop(&self) {
        self.kill();
    }

    pub fn hold(child: &tokio::process::Child) -> Option<Tree> {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        let process = child.raw_handle()?;
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let tree = Tree(job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                tree.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_mut(&mut limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 || AssignProcessToJobObject(tree.0, process as HANDLE) == 0 {
                return None;
            }
            Some(tree)
        }
    }

    pub fn kill(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Tree {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn terminate(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_wrong_token() {
        let token = identify(std::process::id()).unwrap();
        assert!(!is_same(std::process::id(), token.wrapping_add(1)));
        assert!(!is_same(std::process::id(), 0));
    }
}
