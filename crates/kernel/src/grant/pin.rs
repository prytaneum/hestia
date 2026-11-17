use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use proto::plugin::{Grant, ScopeKind};
use sha2::{Digest, Sha256};

use super::endpoint;
use super::program::{check, Family};
use super::GrantError;

const PROGRAM_BYTES_MAX: u64 = 256 * 1024 * 1024;

/// What a plugin may never be granted because it is Hestia's own: anything
/// under `directories`, the plugins' sandboxes (a program there is the
/// plugin's to rewrite; the rest of the data home stays grantable), the
/// `programs` that are Hestia's binaries and the `endpoints` that are its
/// daemon's.
#[derive(Debug, Clone, Default)]
pub struct Reserved {
    pub directories: Vec<PathBuf>,
    pub programs: Vec<PathBuf>,
    pub endpoints: Vec<String>,
}

fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

impl Reserved {
    fn covers_program(&self, program: &Path) -> bool {
        self.directories
            .iter()
            .any(|dir| program.starts_with(resolved(dir)))
            || self.programs.iter().any(|own| resolved(own) == program)
    }

    /// Whether `endpoint` is Hestia's own: a listed endpoint (compared as the
    /// file it resolves to, so a symlink to one is caught), or any pipe in
    /// Hestia's namespace or one Windows would read as something else.
    pub(crate) fn covers_endpoint(&self, endpoint: &str) -> bool {
        if let Some(name) = endpoint::pipe_name(endpoint) {
            if !endpoint::plain(name) || endpoint::in_hestia_namespace(endpoint) {
                return true;
            }
        }
        self.endpoints.iter().any(|own| {
            own.eq_ignore_ascii_case(endpoint)
                || resolved(Path::new(own)) == resolved(Path::new(endpoint))
        })
    }
}

/// Fills the digest of each program in `grant` and refuses what is reserved.
/// Blocking: it reads the program files, so callers run it off the async
/// executor. `derive` stays pure and leaves the digests empty.
pub fn pin(grant: &mut Grant, reserved: &Reserved) -> Result<(), GrantError> {
    for granted in &mut grant.permissions {
        let Some(scope) = &mut granted.scope else {
            continue;
        };
        let refuse = |entry: &str, reason: String| GrantError::Program {
            permission: granted.permission.clone(),
            entry: entry.to_owned(),
            reason,
        };
        match scope.kind {
            ScopeKind::Programs => {
                let mut known: Vec<(PathBuf, String)> = Vec::new();
                for program in &mut scope.programs {
                    check(&program.path, Family::HERE)
                        .map_err(|reason| refuse(&program.path, reason.to_owned()))?;
                    let path = resolved(Path::new(&program.path));
                    if reserved.covers_program(&path) {
                        return Err(refuse(
                            &program.path,
                            "it is Hestia's own, or lives in its data".into(),
                        ));
                    }
                    program.sha256 = match known.iter().find(|(seen, _)| *seen == path) {
                        Some((_, sha256)) => sha256.clone(),
                        None => {
                            let sha256 = digest(&path).map_err(|error| {
                                refuse(&program.path, format!("cannot read it: {error}"))
                            })?;
                            known.push((path, sha256.clone()));
                            sha256
                        }
                    };
                }
            }
            ScopeKind::Endpoints => {
                if let Some(own) = scope.values.iter().find(|v| reserved.covers_endpoint(v)) {
                    return Err(GrantError::Endpoint {
                        entry: own.clone(),
                        reason: "it is Hestia's own".into(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Opens the regular file at `path` for hashing, never in a way that waits on a
/// pipe, and refuses what is past [`PROGRAM_BYTES_MAX`]. On Windows the handle
/// denies writers and deleters for as long as it is open.
pub(crate) fn open(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    #[cfg(windows)]
    {
        const FILE_SHARE_READ: u32 = 1;
        std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, FILE_SHARE_READ);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > PROGRAM_BYTES_MAX {
        return Err(io::Error::other(
            "not a regular file, or larger than 256 MiB",
        ));
    }
    Ok(file)
}

/// The SHA-256 of what `file` holds, in lowercase hex, with every byte hashed
/// also written to `copy`.
pub(crate) fn hash(file: &File, mut copy: Option<&mut dyn Write>) -> io::Result<String> {
    let mut hash = Sha256::new();
    let mut reader = file.take(PROGRAM_BYTES_MAX + 1);
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        match reader.read(&mut buffer)? {
            0 => break,
            read => {
                total += read as u64;
                hash.update(&buffer[..read]);
                if let Some(copy) = copy.as_deref_mut() {
                    copy.write_all(&buffer[..read])?;
                }
            }
        }
    }
    if total > PROGRAM_BYTES_MAX {
        return Err(io::Error::other("larger than 256 MiB"));
    }
    Ok(hex::encode(hash.finalize()))
}

/// The SHA-256 of the regular file at `path`, in lowercase hex.
pub(crate) fn digest(path: &Path) -> io::Result<String> {
    hash(&open(path)?, None)
}
