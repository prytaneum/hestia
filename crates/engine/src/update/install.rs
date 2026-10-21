//! How this copy of Hestia was installed — detected, never recorded, since a
//! build that writes it down is wrong the moment someone moves it.

use proto::update::UpdateInstall;

/// The manifest `formats` key for this install. `None` where the platform's
/// default artifact is already the right one.
pub fn manifest_format(install: UpdateInstall) -> Option<&'static str> {
    match install {
        UpdateInstall::Deb => Some("deb"),
        UpdateInstall::Rpm => Some("rpm"),
        UpdateInstall::Nsis | UpdateInstall::AppImage | UpdateInstall::Unmanaged => None,
    }
}

#[cfg(feature = "portable")]
pub fn detect() -> UpdateInstall {
    UpdateInstall::Unmanaged
}

#[cfg(all(windows, not(feature = "portable")))]
pub fn detect() -> UpdateInstall {
    // The uninstaller sits at the layout root, one level above the `bin/` the
    // daemon and CLI install into — the only thing distinguishing an
    // NSIS-managed tree from the portable archive, which has the same shape.
    let managed = common::paths::install_root()
        .map(|root| root.join("uninstall.exe").is_file())
        .unwrap_or(false);
    if managed {
        UpdateInstall::Nsis
    } else {
        UpdateInstall::Unmanaged
    }
}

#[cfg(all(not(windows), not(feature = "portable")))]
pub fn detect() -> UpdateInstall {
    if common::appimage::image().is_some() {
        return UpdateInstall::AppImage;
    }
    let Ok(exe) = std::env::current_exe() else {
        return UpdateInstall::Unmanaged;
    };
    if !exe.starts_with("/usr/") && !exe.starts_with("/opt/") {
        return UpdateInstall::Unmanaged;
    }
    owning_package(&exe)
}

#[cfg(all(not(windows), not(feature = "portable")))]
fn owning_package(exe: &std::path::Path) -> UpdateInstall {
    use std::process::{Command, Stdio};

    let owns = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .arg(exe)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };

    if owns("dpkg-query", &["-S"]) {
        UpdateInstall::Deb
    } else if owns("rpm", &["-qf"]) {
        UpdateInstall::Rpm
    } else {
        UpdateInstall::Unmanaged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_binary_is_never_managed() {
        assert_eq!(detect(), UpdateInstall::Unmanaged);
    }
}
