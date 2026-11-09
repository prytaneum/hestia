use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Stdio;

use proto::process::ProcessSpec;
use tokio::io::AsyncWriteExt;
use tokio::net::unix::pipe;

use super::Launch;

const FILE: &str = "stdin";

pub fn create(
    spec: &ProcessSpec,
    proc_dir: &Path,
    _relay_program: Option<&Path>,
) -> std::io::Result<Launch> {
    let path = proc_dir.join(FILE);
    remove_path(&path);
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(std::io::Error::other)?;
    // SAFETY: c_path is a valid NUL-terminated path for the duration of the call.
    if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Read+write keeps a reader and a writer on the FIFO while the daemon is
    // away, so the child never sees EOF.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)?;
    Ok(Launch {
        target: path.to_string_lossy().into_owned(),
        stdin: Stdio::from(held),
        spec: spec.clone(),
    })
}

pub async fn write(target: &str, line: &str) -> std::io::Result<()> {
    let mut sender = pipe::OpenOptions::new().open_sender(target)?;
    sender.write_all(format!("{line}\n").as_bytes()).await
}

pub fn remove(target: &str) -> bool {
    remove_path(Path::new(target))
}

fn remove_path(path: &Path) -> bool {
    std::fs::remove_file(path).is_ok()
}
