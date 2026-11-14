//! Host platform mapping (the vocabulary a Java provider resolves by) and locating the java binary
//! inside an extracted archive.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

/// The platform a Java build is resolved for: os (`windows`, `mac`, `linux`) and
/// architecture (`x64`, `aarch64`).
pub struct JavaTarget {
    pub os: String,
    pub arch: String,
}

pub fn host_target() -> Result<JavaTarget> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        bail!("no Java builds are published for this CPU architecture");
    };
    Ok(JavaTarget {
        os: os.to_string(),
        arch: arch.to_string(),
    })
}

fn java_exe() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

fn java_under(home: &Path) -> Option<PathBuf> {
    let direct = home.join("bin").join(java_exe());
    if direct.is_file() {
        return Some(direct);
    }
    let macos = home
        .join("Contents")
        .join("Home")
        .join("bin")
        .join(java_exe());
    if macos.is_file() {
        return Some(macos);
    }
    None
}

/// Find the java executable under an extracted archive: at the root, or one
/// directory level down (the usual `jdk-XX/` wrapper).
pub fn find_java_executable(root: &Path) -> Option<PathBuf> {
    if let Some(exe) = java_under(root) {
        return Some(exe);
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            if let Some(exe) = java_under(&entry.path()) {
                return Some(exe);
            }
        }
    }
    None
}
