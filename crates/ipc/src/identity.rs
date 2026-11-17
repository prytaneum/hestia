//! The Windows user behind a pipe: the SID that names the endpoint, the access
//! list the daemon's pipe is created with, and the check that a server answering
//! on it runs as that same user.

use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by this module and is closed once.
        unsafe { CloseHandle(self.0) };
    }
}

struct Sid {
    buffer: Vec<u64>,
}

impl Sid {
    fn psid(&self) -> PSID {
        // SAFETY: the buffer holds a TOKEN_USER written by GetTokenInformation,
        // aligned to 8 bytes, whose SID points into the same heap buffer.
        unsafe { (*self.buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    fn of_token(token: HANDLE) -> io::Result<Sid> {
        let mut needed = 0u32;
        // SAFETY: a null buffer of length 0 only asks for the size.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: the buffer is at least `needed` bytes.
        let read = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        if read == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Sid { buffer })
    }

    fn of_process(process: HANDLE) -> io::Result<Sid> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: `token` receives a handle that Handle then owns.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Handle(token);
        Sid::of_token(token.0)
    }

    fn current() -> io::Result<Sid> {
        // SAFETY: the pseudo handle of the current process needs no closing.
        Sid::of_process(unsafe { GetCurrentProcess() })
    }

    fn text(&self) -> io::Result<String> {
        let mut raw: *mut u16 = std::ptr::null_mut();
        // SAFETY: on success `raw` is a NUL-terminated string that LocalFree releases.
        if unsafe { ConvertSidToStringSidW(self.psid(), &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        // SAFETY: the string is NUL-terminated, so the scan stops inside it.
        while unsafe { *raw.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: `length` units were read above.
        let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, length) });
        // SAFETY: `raw` was allocated by the conversion call.
        unsafe { LocalFree(raw.cast()) };
        Ok(text)
    }

    fn is(&self, other: &Sid) -> bool {
        // SAFETY: both point at valid SIDs.
        unsafe { EqualSid(self.psid(), other.psid()) != 0 }
    }
}

/// The current user's SID as text, the part of the daemon's pipe name that makes
/// it per user. The pipe's access list and its server check still hold when this
/// is unavailable.
pub fn user_tag() -> String {
    static TAG: OnceLock<String> = OnceLock::new();
    TAG.get_or_init(|| {
        Sid::current()
            .and_then(|sid| sid.text())
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "the user's SID is unavailable; the pipe name is not per user");
                "unknown".to_owned()
            })
    })
    .clone()
}

/// An access list that grants the current user and the system, and nobody else.
pub struct PipeSecurity {
    descriptor: PSECURITY_DESCRIPTOR,
    attributes: SECURITY_ATTRIBUTES,
}

// SAFETY: the descriptor is not modified after construction and is freed once on drop.
unsafe impl Send for PipeSecurity {}
unsafe impl Sync for PipeSecurity {}

impl PipeSecurity {
    pub fn current_user() -> io::Result<PipeSecurity> {
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", Sid::current()?.text()?)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated; the descriptor is released with LocalFree.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(PipeSecurity {
            descriptor,
            attributes: SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            },
        })
    }

    pub fn attributes(&self) -> *mut c_void {
        std::ptr::addr_of!(self.attributes).cast_mut().cast()
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from the conversion call and is freed once.
        unsafe { LocalFree(self.descriptor) };
    }
}

/// Refuses a pipe whose server runs as another user: the name is predictable, so
/// another local user could have created it first.
pub fn ensure_served_by_current_user(pipe: &impl AsRawHandle) -> io::Result<()> {
    let denied = |why: &str| io::Error::new(io::ErrorKind::PermissionDenied, why.to_owned());
    let mut pid = 0u32;
    // SAFETY: the handle is a live pipe handle held by the caller.
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a plain open; the result is checked.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(denied("the pipe's server process cannot be inspected"));
    }
    let process = Handle(process);
    let owner = Sid::of_process(process.0)?;
    if owner.is(&Sid::current()?) {
        Ok(())
    } else {
        tracing::warn!(pid, "a pipe served by another user was refused");
        Err(denied("the pipe is served by another user"))
    }
}
