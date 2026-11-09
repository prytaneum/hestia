//! In-process archive extraction — the `tar` + `flate2` and `zip` crates replace
//! shelling out to the system `tar`/`bsdtar`, so it behaves the same on every
//! platform.

use std::fs::File;
use std::path::Path;

use anyhow::{bail, Context, Result};

/// Extract `archive` into `dest`.
pub fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else {
        bail!("unsupported archive format: {name}");
    }
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    tar.set_overwrite(true);

    for entry in tar.entries().context("reading tar entries")? {
        let mut entry = entry?;
        entry.unpack_in(dest).context("extracting tar entry")?;
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    extract_zip_under(archive, Path::new(""), dest, |_, _| Ok(()))
}

/// Extract the zip members under `root` into `dest` with `root` stripped
/// (an empty `root` takes everything). `after_file(done, total)` runs after
/// each file with uncompressed byte counts and may abort the extraction.
pub fn extract_zip_under(
    archive: &Path,
    root: &Path,
    dest: &Path,
    mut after_file: impl FnMut(u64, u64) -> Result<()>,
) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("opening zip archive")?;
    std::fs::create_dir_all(dest)?;

    let mut members = Vec::new();
    let mut total = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let Some(path) = entry.enclosed_name() else {
            bail!("zip archive contains an unsafe path");
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        total += entry.size();
        members.push((i, relative.to_path_buf()));
    }

    let mut done = 0;
    for (i, relative) in members {
        let mut entry = zip.by_index(i)?;
        let out = dest.join(relative);
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
        done += entry.size();
        after_file(done, total)?;
    }
    Ok(())
}
