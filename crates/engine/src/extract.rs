//! In-process archive extraction — the `tar` + `flate2` and `zip` crates replace
//! shelling out to the system `tar`/`bsdtar`, so it behaves the same on every
//! platform.

use std::fs::File;
use std::path::Path;

use anyhow::{bail, Context, Result};

/// Extract `archive` into `dest`, invoking `on_progress(done, total)` as entries
/// land. `total` is 0 (unknown) for streamed tarballs.
pub fn extract_archive(archive: &Path, dest: &Path, on_progress: impl Fn(u64, u64)) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    if name.ends_with(".zip") {
        extract_zip(archive, dest, &on_progress)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest, &on_progress)
    } else {
        bail!("unsupported archive format: {name}");
    }
}

fn extract_tar_gz(archive: &Path, dest: &Path, on_progress: &impl Fn(u64, u64)) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    tar.set_overwrite(true);

    let mut done = 0u64;
    for entry in tar.entries().context("reading tar entries")? {
        let mut entry = entry?;
        entry.unpack_in(dest).context("extracting tar entry")?;
        done += 1;
        on_progress(done, 0);
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path, on_progress: &impl Fn(u64, u64)) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("opening zip archive")?;
    let total = zip.len() as u64;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(path) = entry.enclosed_name() else {
            bail!("zip archive contains an unsafe path");
        };
        let out = dest.join(path);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut writer = File::create(&out)?;
            std::io::copy(&mut entry, &mut writer)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = entry.unix_mode() {
                    let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode));
                }
            }
        }
        on_progress(i as u64 + 1, total);
    }
    Ok(())
}
