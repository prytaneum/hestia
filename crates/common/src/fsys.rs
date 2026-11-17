//! Filesystem primitives whose behaviour differs by platform, each with one
//! signature on every OS: owner-only permissions, retried renames and removals,
//! and an open that never follows a link.

use std::fs::{DirBuilder, File};
use std::io;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    File,
    Dir,
}

const ATTEMPTS: u32 = 5;
const FIRST_BACKOFF: Duration = Duration::from_millis(20);

/// Limits `path` to its owner: mode 0600 or 0700 on unix; on Windows a
/// protected DACL (inheritance from the parent is cut) granting only the
/// current user and SYSTEM, inherited by a directory's children.
pub fn restrict(path: &Path, entry: Entry) -> io::Result<()> {
    platform::restrict(path, entry)
}

/// Creates `dir` and any missing parents; a directory that did not exist is
/// then [`restrict`]ed. One that already exists keeps its permissions, since it
/// may be one the user owns for other reasons, such as the data home.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    let existed = dir.is_dir();
    DirBuilder::new().recursive(true).create(dir)?;
    if existed {
        return Ok(());
    }
    restrict(dir, Entry::Dir)
}

/// Opens `path` for reading without following a final link, so what is checked
/// on the handle is what is read. `None` when it is a link or not a regular
/// file. Never waits on a pipe.
pub fn open_regular(path: &Path) -> io::Result<Option<File>> {
    let file = match platform::open_unfollowed(path) {
        Ok(file) => file,
        Err(error) if platform::is_link_refusal(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let metadata = file.metadata()?;
    Ok((metadata.is_file() && !platform::is_reparse_point(&metadata)).then_some(file))
}

/// Runs `op` again, a few times with a short backoff, when Windows refuses it
/// because an antivirus scanner or an indexer holds the file. On unix it is a
/// plain call. Blocking.
pub fn retry<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut backoff = FIRST_BACKOFF;
    for _ in 1..ATTEMPTS {
        match op() {
            Err(error) if platform::is_held(&error) => {
                std::thread::sleep(backoff);
                backoff *= 2;
            }
            outcome => return outcome,
        }
    }
    op()
}

pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    retry(|| std::fs::rename(from.as_ref(), to.as_ref()))
}

pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    retry(|| std::fs::remove_file(path.as_ref()))
}

pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    retry(|| std::fs::remove_dir_all(path.as_ref()))
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    pub fn restrict(path: &Path, entry: Entry) -> io::Result<()> {
        let mode = match entry {
            Entry::File => 0o600,
            Entry::Dir => 0o700,
        };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    pub fn open_unfollowed(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }

    pub fn is_link_refusal(error: &io::Error) -> bool {
        error.raw_os_error() == Some(libc::ELOOP)
    }

    pub fn is_reparse_point(_: &std::fs::Metadata) -> bool {
        false
    }

    pub fn is_held(_: &io::Error) -> bool {
        false
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, HANDLE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    pub fn restrict(path: &Path, entry: Entry) -> io::Result<()> {
        let sid = current_user()?;
        let inherit = match entry {
            Entry::File => "",
            Entry::Dir => "OICI",
        };
        let sddl = format!("D:P(A;{inherit};FA;;;SY)(A;{inherit};FA;;;{sid})");
        let path = wide(path.as_os_str());
        let sddl = wide(sddl.as_ref());
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: both strings are NUL-terminated and outlive the calls; the
        // descriptor the first call allocates is freed after the DACL it owns
        // has been applied.
        unsafe {
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let (mut present, mut defaulted) = (0, 0);
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let found =
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
            let outcome = if found == 0 || present == 0 {
                Err(io::Error::last_os_error())
            } else {
                match SetNamedSecurityInfoW(
                    path.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    dacl,
                    std::ptr::null(),
                ) {
                    0 => Ok(()),
                    code => Err(io::Error::from_raw_os_error(code as i32)),
                }
            };
            LocalFree(descriptor);
            outcome
        }
    }

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn current_user() -> io::Result<String> {
        static SID: OnceLock<String> = OnceLock::new();
        if let Some(sid) = SID.get() {
            return Ok(sid.clone());
        }
        let sid = query_current_user()?;
        Ok(SID.get_or_init(|| sid).clone())
    }

    fn query_current_user() -> io::Result<String> {
        // SAFETY: the token handle is closed on every path; the buffer is
        // 8-aligned and sized by the first call, so the TOKEN_USER read from it
        // and the SID inside stay valid until the buffer drops; the string
        // `ConvertSidToStringSidW` allocates is copied, then freed.
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let outcome = (|| {
                let mut length = 0u32;
                GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut length);
                let mut buffer = vec![0u64; (length as usize).div_ceil(8).max(1)];
                if GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    length,
                    &mut length,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
                let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
                let mut text: *mut u16 = std::ptr::null_mut();
                if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let mut end = 0;
                while *text.add(end) != 0 {
                    end += 1;
                }
                let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, end));
                LocalFree(text.cast());
                Ok(sid)
            })();
            CloseHandle(token);
            outcome
        }
    }

    pub fn open_unfollowed(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    pub fn is_link_refusal(_: &io::Error) -> bool {
        false
    }

    pub fn is_reparse_point(metadata: &std::fs::Metadata) -> bool {
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    pub fn is_held(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
        )
    }
}
