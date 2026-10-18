//! Server backups: gzipped tar archives of a server's `data/` directory
//! under the entry root's `backups/` (appearing on demand, like the other
//! managed content directories). An archive is named
//! `<utc-stamp>-<kind>.tar.gz` — the disk is the registry, as with the entry
//! stores. Creation skips the top-level names the caller can re-materialise
//! (jar, libraries, logs); restore swaps the archive back in, carrying those
//! same names over from the current tree.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::backup::{BackupInfo, BackupKind};
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use serde::{Deserialize, Serialize};

use crate::schema::{self, Document};

const BACKUPS: &str = "backups";
const EXTENSION: &str = ".tar.gz";
/// The archive's first entry: what it is and which schema it was written with,
/// so a restore refuses an archive from a newer build instead of scattering
/// whatever it finds into the server's data directory.
const MANIFEST: &str = "hestia.backup.json";
const SESSION_LOCK: &str = "session.lock";

/// Progress for an archive/restore pass: `(files done, files total)`; total
/// is 0 when unknown (restoring streams the archive once).
/// Per-file archive progress. Returns `Err` to stop: a cancelled archive
/// unwinds through the same path a write failure does, and the `.part` it was
/// building is discarded rather than promoted.
pub type OnProgress<'a> = &'a (dyn Fn(u64, u64) -> Result<()> + Send + Sync);

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
struct Manifest {
    created_unix: i64,
    created_by: String,
}

impl Document for Manifest {
    const NAME: &'static str = MANIFEST;
}

pub fn backups_dir(entry_dir: &Path) -> PathBuf {
    entry_dir.join(BACKUPS)
}

/// Drop any `.part` archive left behind by an interrupted create. A partial
/// archive is never promoted (the rename is the commit), so one on disk means
/// the job that was writing it died — see `crate::reclaim`.
pub fn reclaim(entry_dir: &Path) -> crate::reclaim::Reclaimed {
    crate::reclaim::suffixed(&backups_dir(entry_dir), common::atomic::SUFFIX)
}

/// Settle what an interrupted restore left beside `data_dir`. A restore sets the
/// live data aside as `.discard-*` before moving `.restore-*` into place, so a
/// kill in between leaves the user's only copy in the discard directory: it goes
/// back when `data_dir` is missing, and is deleted only once `data_dir` exists.
/// A `.restore-*` is never complete, so it is always dropped.
pub fn recover(entry_dir: &Path, data_dir: &Path) -> crate::reclaim::Reclaimed {
    let mut freed = crate::reclaim::Reclaimed::default();
    let Ok(entries) = std::fs::read_dir(entry_dir) else {
        return freed;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if name.starts_with(".discard-") && !data_dir.exists() {
            match std::fs::rename(&path, data_dir) {
                Ok(()) => tracing::warn!(
                    from = %path.display(),
                    to = %data_dir.display(),
                    "put back the data an interrupted restore set aside"
                ),
                Err(e) => tracing::error!(
                    path = %path.display(),
                    error = %e,
                    "cannot put back the data an interrupted restore set aside"
                ),
            }
        } else if name.starts_with(".discard-") || name.starts_with(".restore-") {
            let bytes = crate::usage::dir_size(&path);
            match std::fs::remove_dir_all(&path) {
                Ok(()) => {
                    tracing::warn!(path = %path.display(), bytes, "removed a leftover restore directory");
                    freed.add(bytes);
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "cannot remove a leftover restore directory")
                }
            }
        }
    }
    freed
}

/// Archive `data_dir` into a new backup under the entry root, skipping the
/// top-level `exclude` names and Minecraft's transient `session.lock` files.
/// The archive lands whole or not at all (written through a `.part` temp file).
pub fn create(
    entry_dir: &Path,
    data_dir: &Path,
    kind: BackupKind,
    exclude: &[String],
    on_progress: OnProgress<'_>,
) -> Result<BackupInfo> {
    if !data_dir.is_dir() {
        bail!(proto::error::ErrorInfo::NothingToDo {
            what: proto::error::Task::BackUp
        });
    }
    let dir = backups_dir(entry_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    // A crash during an earlier archive leaves its `.part` behind; drop it here
    // too, so the store is clean whether or not the daemon restarted since.
    let freed = reclaim(entry_dir);
    if !freed.is_empty() {
        tracing::info!(
            entries = freed.entries,
            bytes = freed.bytes,
            "reclaimed abandoned backup archives"
        );
    }
    let id = allocate_id(entry_dir, kind);
    let path = dir.join(format!("{id}{EXTENSION}"));
    let part = common::atomic::staged(&path);

    let written = write_archive(&part, data_dir, exclude, on_progress);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &path).with_context(|| format!("cannot finalise {}", path.display()))?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    tracing::info!(id, kind = kind.as_str(), size, "backup created");
    Ok(BackupInfo {
        id,
        kind,
        created_unix: common::now_unix(),
        size,
    })
}

fn write_archive(
    dest: &Path,
    data_dir: &Path,
    exclude: &[String],
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let file = File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.follow_symlinks(false);
    append_manifest(&mut builder)?;

    let total = count_files(data_dir, exclude)?;
    let mut done = 0u64;
    for entry in included_roots(data_dir, exclude)? {
        append_tree(
            &mut builder,
            data_dir,
            &entry,
            &mut done,
            total,
            on_progress,
        )?;
    }
    builder
        .into_inner()
        .context("finalising the archive")?
        .finish()
        .context("flushing the archive")?
        .sync_all()
        .context("syncing the archive")?;
    Ok(())
}

fn append_manifest<W: std::io::Write>(builder: &mut tar::Builder<W>) -> Result<()> {
    let manifest = Manifest {
        created_unix: common::now_unix(),
        created_by: format!("{} {}", common::app::NAME, common::app::VERSION),
    };
    let encoded = serde_json::to_vec_pretty(&schema::encode(&manifest)?)
        .context("the backup manifest serializes")?;
    let mut header = tar::Header::new_gnu();
    header.set_size(encoded.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, MANIFEST, encoded.as_slice())
        .context("cannot write the backup manifest")
}

/// The top-level names under `data_dir` a backup includes (everything not
/// excluded, skipping non-UTF-8 names — tar headers are byte-exact but the
/// exclude match is string-based, so an unmatchable name is safer skipped).
/// The manifest name is reserved: a data file called that would otherwise be
/// archived twice and read as the manifest on the way back.
fn included_roots(data_dir: &Path, exclude: &[String]) -> Result<Vec<PathBuf>> {
    let mut roots = Vec::new();
    let entries = std::fs::read_dir(data_dir)
        .with_context(|| format!("cannot read {}", data_dir.display()))?;
    for entry in entries {
        let entry = entry.context("reading the data directory")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            tracing::warn!(path = %entry.path().display(), "skipping non-UTF-8 name");
            continue;
        };
        if name == MANIFEST || exclude.iter().any(|e| e == name) {
            continue;
        }
        roots.push(PathBuf::from(name));
    }
    roots.sort();
    Ok(roots)
}

fn count_files(data_dir: &Path, exclude: &[String]) -> Result<u64> {
    fn walk(path: &Path) -> u64 {
        if is_session_lock(path) {
            return 0;
        }
        if !path.is_dir() || path.is_symlink() {
            return 1;
        }
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| walk(&e.path())).sum())
            .unwrap_or(0)
    }
    Ok(included_roots(data_dir, exclude)?
        .iter()
        .map(|rel| walk(&data_dir.join(rel)))
        .sum())
}

/// Append `rel` (a path under `base`) file by file, so progress ticks per
/// file rather than per top-level directory.
fn append_tree(
    builder: &mut tar::Builder<flate2::write::GzEncoder<File>>,
    base: &Path,
    rel: &Path,
    done: &mut u64,
    total: u64,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let path = base.join(rel);
    if is_session_lock(&path) {
        return Ok(());
    }
    if path.is_symlink() || !path.is_dir() {
        builder
            .append_path_with_name(&path, rel)
            .with_context(|| format!("archiving {}", rel.display()))?;
        *done += 1;
        on_progress(*done, total)?;
        return Ok(());
    }
    builder
        .append_dir(rel, &path)
        .with_context(|| format!("archiving {}", rel.display()))?;
    let entries =
        std::fs::read_dir(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut children: Vec<_> = entries
        .collect::<std::io::Result<Vec<_>>>()
        .context("reading a data subdirectory")?;
    children.sort_by_key(|e| e.file_name());
    for child in children {
        append_tree(
            builder,
            base,
            &rel.join(child.file_name()),
            done,
            total,
            on_progress,
        )?;
    }
    Ok(())
}

/// A world lock is transient state, not world data. It must not survive into
/// an archive, regardless of which world directory contains it.
fn is_session_lock(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == SESSION_LOCK)
}

/// Every stored backup, newest first.
pub fn list(entry_dir: &Path) -> Vec<BackupInfo> {
    let mut backups = Vec::new();
    if let Ok(entries) = std::fs::read_dir(backups_dir(entry_dir)) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(stem) = name.strip_suffix(EXTENSION) else {
                continue;
            };
            let Some(kind) = kind_of(stem) else { continue };
            let metadata = entry.metadata().ok();
            backups.push(BackupInfo {
                id: stem.to_string(),
                kind,
                created_unix: metadata
                    .as_ref()
                    .and_then(common::modified_unix)
                    .unwrap_or(0),
                size: metadata.map(|m| m.len()).unwrap_or(0),
            });
        }
    }
    backups.sort_by(|a, b| b.id.cmp(&a.id));
    backups
}

pub fn find(entry_dir: &Path, reference: &str) -> Option<BackupInfo> {
    list(entry_dir).into_iter().find(|b| b.id == reference)
}

/// Delete one backup archive. Returns false when no backup matches.
pub fn remove(entry_dir: &Path, reference: &str) -> Result<bool> {
    let Some(backup) = find(entry_dir, reference) else {
        return Ok(false);
    };
    let path = backups_dir(entry_dir).join(format!("{}{EXTENSION}", backup.id));
    std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
    tracing::info!(id = %backup.id, "backup removed");
    Ok(true)
}

/// Replace `data_dir` with a backup's content. The archive extracts into a
/// staging directory first and the trees swap only when extraction succeeded,
/// so a failure leaves the current data untouched. The top-level `preserve`
/// names (the same set excluded at create) carry over from the current tree —
/// they belong to the entry's *current* version, not the backup's.
pub fn restore(
    entry_dir: &Path,
    data_dir: &Path,
    reference: &str,
    preserve: &[String],
    on_progress: OnProgress<'_>,
) -> Result<BackupInfo> {
    let backup =
        find(entry_dir, reference).with_context(|| format!("no backup matches '{reference}'"))?;
    let archive = backups_dir(entry_dir).join(format!("{}{EXTENSION}", backup.id));
    recover(entry_dir, data_dir);
    let staging = entry_dir.join(format!(".restore-{}", backup.id));
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("cannot create {}", staging.display()))?;

    let extracted = extract_archive(&archive, &staging, on_progress).and_then(|()| {
        for name in preserve {
            let current = data_dir.join(name);
            if !current.exists() {
                continue;
            }
            let kept = staging.join(name);
            if kept.exists() {
                if kept.is_dir() {
                    std::fs::remove_dir_all(&kept)
                } else {
                    std::fs::remove_file(&kept)
                }
                .with_context(|| format!("cannot drop the archived {name}"))?;
            }
            std::fs::rename(&current, &kept)
                .with_context(|| format!("cannot carry {name} over"))?;
        }
        Ok(())
    });
    if let Err(e) = extracted {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    let trash = entry_dir.join(format!(".discard-{}", backup.id));
    if data_dir.exists() {
        std::fs::rename(data_dir, &trash).context("cannot set the current data aside")?;
    }
    if let Err(e) = std::fs::rename(&staging, data_dir) {
        let _ = std::fs::rename(&trash, data_dir);
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e).context("cannot move the restored data into place");
    }
    if let Err(e) = std::fs::remove_dir_all(&trash) {
        if trash.exists() {
            tracing::warn!(path = %trash.display(), error = %e, "replaced data left behind");
        }
    }
    tracing::info!(id = %backup.id, "backup restored");
    Ok(backup)
}

fn extract_archive(archive: &Path, dest: &Path, on_progress: OnProgress<'_>) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("cannot open {}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    tar.set_preserve_permissions(true);
    // unpack_in refuses entries that would escape `dest`.
    let mut done = 0u64;
    let mut checked = false;
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry.context("reading an archive entry")?;
        // The manifest is written first, so an archive this build cannot read is
        // refused before anything of it reaches the staging tree.
        if entry.path().is_ok_and(|p| p.as_os_str() == MANIFEST) {
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .context("reading the backup manifest")?;
            check_manifest(&text)?;
            checked = true;
            continue;
        }
        if !checked {
            return Err(manifest_missing().into());
        }
        entry
            .unpack_in(dest)
            .context("extracting an archive entry")?;
        done += 1;
        on_progress(done, 0)?;
    }
    if !checked {
        return Err(manifest_missing().into());
    }
    Ok(())
}

fn manifest_missing() -> ErrorInfo {
    ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Backup,
        reason: ArchiveFault::ManifestMissing,
    }
}

fn check_manifest(text: &str) -> Result<()> {
    let value = serde_json::from_str(text).map_err(|e| {
        tracing::warn!(error = %e, "{MANIFEST} is malformed");
        malformed()
    })?;
    schema::decode::<Manifest>(value).map_err(|e| match e {
        schema::SchemaError::FromTheFuture { found, .. } => ErrorInfo::ArchiveTooNew {
            format: ArchiveFormat::Backup,
            schema: found,
        },
        other => {
            tracing::warn!(error = %other, "{MANIFEST} cannot be decoded");
            malformed()
        }
    })?;
    Ok(())
}

fn malformed() -> ErrorInfo {
    ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Backup,
        reason: ArchiveFault::ManifestMalformed,
    }
}

/// Delete the oldest `kind` backups beyond `keep`; other kinds are untouched.
/// Returns the removed backups.
pub fn prune(entry_dir: &Path, kind: BackupKind, keep: usize) -> Result<Vec<BackupInfo>> {
    let expired: Vec<BackupInfo> = list(entry_dir)
        .into_iter()
        .filter(|b| b.kind == kind)
        .skip(keep)
        .collect();
    for backup in &expired {
        remove(entry_dir, &backup.id)?;
    }
    Ok(expired)
}

/// A fresh backup id: the current UTC time plus the kind, suffixed on the
/// (unlikely) same-second collision.
fn allocate_id(entry_dir: &Path, kind: BackupKind) -> String {
    let taken: HashSet<String> = list(entry_dir).into_iter().map(|b| b.id).collect();
    let base = format!(
        "{}-{}",
        crate::registry::utc_stamp(common::now_unix()),
        kind.as_str()
    );
    if !taken.contains(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken.contains(id))
        .expect("an unclaimed backup id exists")
}

/// The kind segment of a backup id — the rightmost dash-separated token that
/// names a kind (a collision suffix may follow it).
fn kind_of(stem: &str) -> Option<BackupKind> {
    stem.rsplit('-').find_map(BackupKind::parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_entry(tag: &str) -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix(&format!("hestia-backup-{tag}-"))
            .tempdir()
            .expect("temp dir");
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn create_restore_round_trips_and_preserves_excluded_paths() {
        let entry_dir = temp_entry("round-trip");
        let entry = entry_dir.path();
        let data = entry.join("data");
        let exclude = vec!["server.jar".to_string(), "logs".to_string()];
        write(&data.join("world/level.dat"), "one");
        write(&data.join("server.properties"), "motd=hi");
        write(&data.join("server.jar"), "current-jar");
        write(&data.join("logs/latest.log"), "log");

        let backup = create(entry, &data, BackupKind::Manual, &exclude, &|_, _| Ok(())).unwrap();
        assert_eq!(list(entry).len(), 1);
        assert_eq!(list(entry)[0].id, backup.id);
        assert_eq!(list(entry)[0].kind, BackupKind::Manual);

        write(&data.join("world/level.dat"), "two");
        write(&data.join("server.jar"), "newer-jar");
        std::fs::remove_file(data.join("server.properties")).unwrap();

        let restored = restore(entry, &data, &backup.id, &exclude, &|_, _| Ok(())).unwrap();
        assert_eq!(restored.id, backup.id);
        let content = |p: &str| std::fs::read_to_string(data.join(p)).unwrap();
        assert_eq!(content("world/level.dat"), "one");
        assert_eq!(content("server.properties"), "motd=hi");
        // Excluded paths carry over from the current tree, not the archive.
        assert_eq!(content("server.jar"), "newer-jar");
        assert_eq!(content("logs/latest.log"), "log");

        assert!(
            !data.join(MANIFEST).exists(),
            "the manifest describes the archive, not the server's data"
        );

        assert!(remove(entry, &backup.id).unwrap());
        assert!(list(entry).is_empty());
    }

    #[test]
    fn a_backup_from_a_newer_build_is_refused_rather_than_unpacked() {
        let entry_dir = temp_entry("newer-backup");
        let entry = entry_dir.path();
        let data = entry.join("data");
        write(&data.join("world/level.dat"), "one");
        let backup = create(entry, &data, BackupKind::Manual, &[], &|_, _| Ok(())).unwrap();

        // Rewrite the archive with a manifest this build cannot read.
        let path = backups_dir(entry).join(format!("{}{EXTENSION}", backup.id));
        let mut manifest = schema::encode(&Manifest::default()).unwrap();
        manifest[schema::FIELD] = serde_json::Value::from(Manifest::VERSION + 1);
        let encoded = serde_json::to_vec(&manifest).unwrap();
        {
            let encoder = flate2::write::GzEncoder::new(
                File::create(&path).unwrap(),
                flate2::Compression::default(),
            );
            let mut builder = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_size(encoded.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, MANIFEST, encoded.as_slice())
                .unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }

        write(&data.join("world/level.dat"), "two");
        let error = restore(entry, &data, &backup.id, &[], &|_, _| Ok(())).expect_err("refused");

        assert!(matches!(
            error.downcast::<proto::error::ErrorInfo>().unwrap(),
            ErrorInfo::ArchiveTooNew { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(data.join("world/level.dat")).unwrap(),
            "two",
            "a refused restore leaves the current data alone"
        );
    }

    #[test]
    fn a_backup_with_no_manifest_is_refused() {
        let entry_dir = temp_entry("legacy-backup");
        let entry = entry_dir.path();
        let data = entry.join("data");
        write(&data.join("world/level.dat"), "one");

        let dir = backups_dir(entry);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("19700101-000000-manual{EXTENSION}"));
        {
            let encoder = flate2::write::GzEncoder::new(
                File::create(&path).unwrap(),
                flate2::Compression::default(),
            );
            let mut builder = tar::Builder::new(encoder);
            builder.append_dir_all("world", data.join("world")).unwrap();
            builder.into_inner().unwrap().finish().unwrap();
        }

        write(&data.join("world/level.dat"), "two");
        let error = restore(entry, &data, "19700101-000000-manual", &[], &|_, _| Ok(()))
            .expect_err("refused");

        assert!(matches!(
            error.downcast::<ErrorInfo>().unwrap(),
            ErrorInfo::ArchiveInvalid {
                reason: ArchiveFault::ManifestMissing,
                ..
            }
        ));
        assert_eq!(
            std::fs::read_to_string(data.join("world/level.dat")).unwrap(),
            "two"
        );
    }

    #[test]
    fn create_omits_session_locks_at_every_depth() {
        let entry_dir = temp_entry("session-lock");
        let entry = entry_dir.path();
        let data = entry.join("data");
        write(&data.join("session.lock"), "root lock");
        write(&data.join("world/session.lock"), "world lock");
        write(&data.join("world/region/r.0.0.mca"), "world data");

        let backup = create(entry, &data, BackupKind::Manual, &[], &|_, _| Ok(())).unwrap();
        let archive = backups_dir(entry).join(format!("{}{EXTENSION}", backup.id));
        let file = File::open(archive).unwrap();
        let decoder = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);
        let paths: Vec<PathBuf> = tar
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().into_owned())
            .collect();

        assert!(paths
            .iter()
            .any(|path| path == Path::new("world/region/r.0.0.mca")));
        assert!(!paths
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == SESSION_LOCK)));
    }

    #[test]
    fn an_interrupted_archive_leaves_no_part_behind() {
        let entry_dir = temp_entry("reclaim");
        let entry = entry_dir.path();
        let data = entry.join("data");
        write(&data.join("world/level.dat"), "x");

        // What a daemon killed mid-archive leaves: a `.part` that was never
        // renamed, so it is in no listing but still occupies the store.
        let orphan = backups_dir(entry).join("20260725-093946-manual.tar.gz.part");
        write(&orphan, "half an archive");
        assert!(list(entry).is_empty(), "a .part is not a backup");

        let freed = reclaim(entry);
        assert_eq!(freed.entries, 1);
        assert!(freed.bytes > 0);
        assert!(!orphan.exists(), "an abandoned .part is reclaimed");

        // And a fresh archive cleans up before writing, so the store is tidy
        // whether or not the daemon restarted in between.
        write(&orphan, "half an archive");
        create(entry, &data, BackupKind::Manual, &[], &|_, _| Ok(())).unwrap();
        assert!(!orphan.exists());
        assert_eq!(list(entry).len(), 1);
    }

    #[test]
    fn prune_drops_only_old_backups_of_the_kind() {
        let entry_dir = temp_entry("prune");
        let entry = entry_dir.path();
        let data = entry.join("data");
        write(&data.join("world/level.dat"), "x");

        let old = create(entry, &data, BackupKind::Scheduled, &[], &|_, _| Ok(())).unwrap();
        let mid = create(entry, &data, BackupKind::Scheduled, &[], &|_, _| Ok(())).unwrap();
        let new = create(entry, &data, BackupKind::Scheduled, &[], &|_, _| Ok(())).unwrap();
        let manual = create(entry, &data, BackupKind::Manual, &[], &|_, _| Ok(())).unwrap();

        let removed = prune(entry, BackupKind::Scheduled, 1).unwrap();
        let removed_ids: Vec<&str> = removed.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(removed_ids, vec![mid.id.as_str(), old.id.as_str()]);
        let mut kept: Vec<String> = list(entry).into_iter().map(|b| b.id).collect();
        kept.sort();
        let mut expected = vec![manual.id.clone(), new.id.clone()];
        expected.sort();
        assert_eq!(kept, expected);
    }

    #[test]
    fn missing_data_and_unknown_backups_are_errors() {
        let entry_dir = temp_entry("errors");
        let entry = entry_dir.path();
        let missing = entry.join("nope");
        assert!(create(entry, &missing, BackupKind::Manual, &[], &|_, _| Ok(())).is_err());
        assert!(restore(entry, &entry.join("data"), "ghost", &[], &|_, _| Ok(())).is_err());
        assert!(!remove(entry, "ghost").unwrap());
    }

    #[test]
    fn recover_puts_back_set_aside_data_and_drops_leftovers() {
        let entry_dir = temp_entry("recover");
        let entry = entry_dir.path();
        let data = entry.join("data");
        std::fs::remove_dir_all(&data).unwrap();
        write(&entry.join(".discard-x/world/level.dat"), "mine");
        write(&entry.join(".restore-y/partial"), "half");

        recover(entry, &data);
        assert_eq!(
            std::fs::read_to_string(data.join("world/level.dat")).unwrap(),
            "mine"
        );
        assert!(!entry.join(".restore-y").exists());

        write(&entry.join(".discard-z/old"), "old");
        recover(entry, &data);
        assert!(!entry.join(".discard-z").exists());
        assert!(data.join("world/level.dat").exists());
    }

    #[test]
    fn kinds_parse_from_ids() {
        assert_eq!(kind_of("20260707-120000-manual"), Some(BackupKind::Manual));
        assert_eq!(
            kind_of("20260707-120000-scheduled-2"),
            Some(BackupKind::Scheduled)
        );
        assert_eq!(kind_of("20260707-120000-update"), Some(BackupKind::Update));
        assert_eq!(kind_of("20260707-120000"), None);
    }
}
