//! The zip plumbing import and export share: planning a directory tree into
//! archive members, writing them through a `.part` that is renamed on success,
//! and reading members back with the same path safety the pack reader applies.
//!
//! Symlinks are **followed**, unlike the server backup's tar. An instance's
//! `data/saves` is routinely a link into the shared store (`sync`), and an
//! export that carried the link rather than the worlds would hand someone an
//! archive with no game data in it. Following links means guarding against
//! cycles, which the planner does by remembering the directories it has been in.

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};
use zip::write::SimpleFileOptions;

use crate::cancel::Job;
use crate::content::pack::is_safe_path;

/// The in-progress archive: renamed onto the real name only once written whole,
/// exactly as a backup is.
const PART_SUFFIX: &str = ".part";

/// One planned archive member: the file to read and what it is called inside
/// the archive.
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) source: PathBuf,
}

/// Every file under `root`, named `<prefix><relative path>` inside the archive.
/// `keep` is asked for each member's archive-relative name (the part after the
/// prefix) and for each directory as it is entered, so a whole subtree is
/// skipped by refusing its directory.
pub(crate) fn plan(root: &Path, prefix: &str, keep: &dyn Fn(&str) -> bool) -> Vec<Member> {
    let mut members = Vec::new();
    let mut visited = HashSet::new();
    walk(root, prefix, "", keep, &mut visited, &mut members);
    members.sort_by(|a, b| a.name.cmp(&b.name));
    members
}

fn walk(
    dir: &Path,
    prefix: &str,
    relative: &str,
    keep: &dyn Fn(&str) -> bool,
    visited: &mut HashSet<PathBuf>,
    members: &mut Vec<Member>,
) {
    // Following links means a directory can be reached twice; stop the second
    // time rather than recursing forever.
    let identity = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(identity) {
        tracing::debug!(path = %dir.display(), "skipping an already-visited directory");
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(path = %dir.display(), error = %e, "cannot read a directory to archive");
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            tracing::warn!(path = %entry.path().display(), "skipping non-UTF-8 name");
            continue;
        };
        let relative = match relative.is_empty() {
            true => name.to_string(),
            false => format!("{relative}/{name}"),
        };
        if !keep(&relative) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk(&path, prefix, &relative, keep, visited, members);
        } else {
            members.push(Member {
                name: format!("{prefix}{relative}"),
                source: path,
            });
        }
    }
}

pub(crate) struct Writer {
    /// `None` once finished, which is also how [`Drop`] tells a completed
    /// archive from an abandoned one.
    zip: Option<zip::ZipWriter<File>>,
    part: PathBuf,
    target: PathBuf,
    files: u64,
}

/// What a finished archive came to.
pub(crate) struct Written {
    pub(crate) path: PathBuf,
    pub(crate) size_bytes: u64,
    pub(crate) files: u64,
}

impl Writer {
    pub(crate) fn create(target: &Path) -> Result<Writer> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let mut name = target.file_name().unwrap_or_default().to_os_string();
        name.push(PART_SUFFIX);
        let part = target.with_file_name(name);
        let file =
            File::create(&part).with_context(|| format!("cannot create {}", part.display()))?;
        Ok(Writer {
            zip: Some(zip::ZipWriter::new(file)),
            part,
            target: target.to_path_buf(),
            files: 0,
        })
    }

    pub(crate) fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let zip = self.zip.as_mut().expect("writer is open");
        zip.start_file::<_, ()>(name, SimpleFileOptions::default())
            .with_context(|| format!("cannot add {name} to the archive"))?;
        zip.write_all(bytes)
            .with_context(|| format!("cannot write {name} into the archive"))?;
        self.files += 1;
        Ok(())
    }

    pub(crate) fn add_file(&mut self, name: &str, source: &Path) -> Result<()> {
        let zip = self.zip.as_mut().expect("writer is open");
        let mut file =
            File::open(source).with_context(|| format!("cannot read {}", source.display()))?;
        zip.start_file::<_, ()>(name, SimpleFileOptions::default())
            .with_context(|| format!("cannot add {name} to the archive"))?;
        std::io::copy(&mut file, zip)
            .with_context(|| format!("cannot write {name} into the archive"))?;
        self.files += 1;
        Ok(())
    }

    /// Write every planned member, reporting one progress tick per file and
    /// stopping at the first cancellation checkpoint after one is requested.
    pub(crate) fn add_all(&mut self, members: &[Member], job: &Job<'_>) -> Result<()> {
        let total = members.len() as u64;
        for (done, member) in members.iter().enumerate() {
            job.check()?;
            tick(
                job,
                ProvisionPhase::Archive,
                done as u64,
                total,
                &member.name,
            );
            // A file that vanished mid-export (a log rotating, a world saving)
            // is not a reason to lose the whole archive.
            if !member.source.is_file() {
                tracing::debug!(path = %member.source.display(), "skipping a vanished file");
                continue;
            }
            self.add_file(&member.name, &member.source)?;
        }
        tick(job, ProvisionPhase::Archive, total, total, "");
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> Result<Written> {
        let zip = self.zip.take().expect("writer is open");
        zip.finish().context("finalising the archive")?.sync_all()?;
        std::fs::rename(&self.part, &self.target)
            .with_context(|| format!("cannot finalise {}", self.target.display()))?;
        let size_bytes = std::fs::metadata(&self.target)
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(Written {
            path: self.target.clone(),
            size_bytes,
            files: self.files,
        })
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if self.zip.is_none() {
            return;
        }
        // Never finished: a failure or a cancellation. The rename is the
        // commit, so there is nothing to keep.
        if let Err(e) = std::fs::remove_file(&self.part) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %self.part.display(), error = %e, "cannot discard a partial archive");
            }
        }
    }
}

pub(crate) struct Reader {
    zip: zip::ZipArchive<File>,
}

impl Reader {
    pub(crate) fn open(path: &Path) -> Result<Reader> {
        let file = File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
        let zip = zip::ZipArchive::new(file).map_err(|e| {
            tracing::warn!(path = %path.display(), error = %e, "cannot open an archive");
            proto::error::ErrorInfo::ArchiveInvalid {
                format: "zip".to_string(),
                detail: format!("'{}' is incomplete or not a zip ({e})", filename_of(path)),
            }
        })?;
        Ok(Reader { zip })
    }

    /// Every member name in the archive, directories included.
    pub(crate) fn names(&self) -> Vec<String> {
        self.zip.file_names().map(str::to_string).collect()
    }

    pub(crate) fn read_bytes(&mut self, name: &str) -> Result<Vec<u8>> {
        let mut entry = self
            .zip
            .by_name(name)
            .with_context(|| format!("'{name}' is missing from the archive"))?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("cannot read '{name}' from the archive"))?;
        Ok(bytes)
    }

    pub(crate) fn read_text(&mut self, name: &str) -> Result<String> {
        let bytes = self.read_bytes(name)?;
        String::from_utf8(bytes).with_context(|| format!("'{name}' is not valid UTF-8"))
    }

    /// Extract every member under `prefix` into `dest`, named relative to that
    /// prefix. `keep` is asked for each relative name; answering false leaves
    /// what is on disk alone. Returns how many files were written.
    ///
    /// A member whose path would escape `dest` fails the whole extraction: an
    /// archive that tries it is hostile, not merely malformed.
    pub(crate) fn extract_under(
        &mut self,
        prefix: &str,
        dest: &Path,
        job: &Job<'_>,
        keep: &dyn Fn(&str) -> bool,
    ) -> Result<u64> {
        let planned: Vec<(usize, String)> = (0..self.zip.len())
            .filter_map(|index| {
                let entry = self.zip.by_index(index).ok()?;
                if entry.is_dir() {
                    return None;
                }
                let relative = entry.name().strip_prefix(prefix)?.trim_start_matches('/');
                match relative.is_empty() {
                    true => None,
                    false => Some((index, relative.to_string())),
                }
            })
            .collect();

        let total = planned.len() as u64;
        let mut written = 0;
        for (done, (index, relative)) in planned.into_iter().enumerate() {
            job.check()?;
            tick(job, ProvisionPhase::Extract, done as u64, total, &relative);
            if !is_safe_path(&relative) {
                anyhow::bail!(proto::error::ErrorInfo::ArchiveInvalid {
                    format: "zip".to_string(),
                    detail: format!("member '{relative}' escapes the instance directory"),
                });
            }
            if !keep(&relative) {
                continue;
            }
            self.write_entry(index, &dest.join(&relative))?;
            written += 1;
        }
        tick(job, ProvisionPhase::Extract, total, total, "");
        Ok(written)
    }

    fn write_entry(&mut self, index: usize, target: &Path) -> Result<()> {
        let mut entry = self
            .zip
            .by_index(index)
            .context("cannot read an archive entry")?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let mut out =
            File::create(target).with_context(|| format!("cannot write {}", target.display()))?;
        std::io::copy(&mut entry, &mut out)
            .with_context(|| format!("cannot write {}", target.display()))?;
        Ok(())
    }
}

fn tick(job: &Job<'_>, phase: ProvisionPhase, current: u64, total: u64, detail: &str) {
    job.report(&ProvisionProgress {
        phase,
        current,
        total,
        detail: detail.to_string(),
        ..Default::default()
    });
}

pub(crate) fn filename_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-archive-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn job_for(cancel: &crate::cancel::Cancel) -> Job<'_> {
        static NOOP: fn(&ProvisionProgress) = |_| {};
        Job::new(&NOOP, cancel)
    }

    #[test]
    fn a_plan_names_members_under_the_prefix_and_skips_refused_subtrees() {
        let dir = temp("plan");
        let root = dir.path();
        write(root, "data/options.txt", "a");
        write(root, "data/config/deep/x.toml", "b");
        write(root, "logs/session-1.log", "c");

        let members = plan(root, "", &|relative| relative != "logs");

        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["data/config/deep/x.toml", "data/options.txt"]);
    }

    #[test]
    fn a_round_trip_preserves_the_tree() {
        let dir = temp("roundtrip");
        let root = &dir.path().join("instance");
        write(root, "data/options.txt", "lang:en");
        write(root, "mods/sodium.jar", "jar bytes");

        let archive = dir.path().join("out.hestia");
        let cancel = crate::cancel::Cancel::new();
        let job = job_for(&cancel);
        let mut writer = Writer::create(&archive).unwrap();
        writer.add_bytes("hestia.instance.json", b"{}").unwrap();
        writer.add_all(&plan(root, "", &|_| true), &job).unwrap();
        let written = writer.finish().unwrap();

        assert_eq!(written.files, 3);
        assert!(written.size_bytes > 0);

        let back = temp("roundtrip-out");
        let mut reader = Reader::open(&archive).unwrap();
        assert_eq!(reader.read_text("hestia.instance.json").unwrap(), "{}");
        let count = reader
            .extract_under("", back.path(), &job, &|name| {
                name != "hestia.instance.json"
            })
            .unwrap();

        assert_eq!(count, 2);
        assert_eq!(
            std::fs::read_to_string(back.path().join("data/options.txt")).unwrap(),
            "lang:en"
        );
        assert!(back.path().join("mods/sodium.jar").is_file());
    }

    #[test]
    fn both_gauges_reach_their_total() {
        let dir = temp("ticks");
        let root = &dir.path().join("instance");
        write(root, "data/options.txt", "lang:en");
        write(root, "mods/sodium.jar", "jar bytes");

        let archive = dir.path().join("out.hestia");
        let cancel = crate::cancel::Cancel::new();
        let seen = std::sync::Mutex::new(Vec::new());
        let record = |p: &ProvisionProgress| {
            seen.lock().unwrap().push((p.phase, p.current, p.total));
        };
        let job = Job::new(&record, &cancel);

        let mut writer = Writer::create(&archive).unwrap();
        writer.add_all(&plan(root, "", &|_| true), &job).unwrap();
        writer.finish().unwrap();
        let back = temp("ticks-out");
        Reader::open(&archive)
            .unwrap()
            .extract_under("", back.path(), &job, &|_| true)
            .unwrap();

        let seen = seen.into_inner().unwrap();
        let last = |phase: ProvisionPhase| {
            seen.iter()
                .rev()
                .find(|(p, _, _)| *p == phase)
                .map(|(_, current, total)| (*current, *total))
        };
        assert_eq!(last(ProvisionPhase::Archive), Some((2, 2)));
        assert_eq!(last(ProvisionPhase::Extract), Some((2, 2)));
    }

    #[test]
    fn an_abandoned_writer_leaves_nothing_behind() {
        let dir = temp("abandoned");
        let archive = dir.path().join("out.hestia");
        {
            let mut writer = Writer::create(&archive).unwrap();
            writer.add_bytes("a.txt", b"x").unwrap();
        }
        assert!(!archive.exists(), "the target is never created early");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(
            leftovers.is_empty(),
            "the .part is discarded: {leftovers:?}"
        );
    }

    #[test]
    fn a_member_escaping_the_destination_is_refused() {
        let dir = temp("escape");
        let archive = dir.path().join("evil.zip");
        {
            let mut writer = Writer::create(&archive).unwrap();
            writer.add_bytes("../escape.txt", b"no").unwrap();
            writer.finish().unwrap();
        }
        let out = temp("escape-out");
        let cancel = crate::cancel::Cancel::new();
        assert!(Reader::open(&archive)
            .unwrap()
            .extract_under("", out.path(), &job_for(&cancel), &|_| true)
            .is_err());
    }

    #[test]
    fn a_truncated_archive_is_invalid_rather_than_unrecognised() {
        let dir = temp("truncated");
        let archive = dir.path().join("pack.mrpack");
        {
            let mut writer = Writer::create(&archive).unwrap();
            writer.add_bytes("modrinth.index.json", b"{}").unwrap();
            writer.finish().unwrap();
        }
        let whole = std::fs::read(&archive).unwrap();
        std::fs::write(&archive, &whole[..whole.len() / 2]).unwrap();

        let error = Reader::open(&archive).err().unwrap();
        assert!(matches!(
            error.downcast_ref::<proto::error::ErrorInfo>(),
            Some(proto::error::ErrorInfo::ArchiveInvalid { .. })
        ));
    }

    #[test]
    fn extraction_is_relative_to_the_root_prefix() {
        let dir = temp("prefix");
        let archive = dir.path().join("nested.zip");
        {
            let mut writer = Writer::create(&archive).unwrap();
            writer.add_bytes("MyPack/instance.cfg", b"name=x").unwrap();
            writer
                .add_bytes("MyPack/.minecraft/options.txt", b"lang:en")
                .unwrap();
            writer.finish().unwrap();
        }
        let out = temp("prefix-out");
        let cancel = crate::cancel::Cancel::new();
        Reader::open(&archive)
            .unwrap()
            .extract_under("MyPack/", out.path(), &job_for(&cancel), &|_| true)
            .unwrap();

        assert!(out.path().join("instance.cfg").is_file());
        assert!(out.path().join(".minecraft/options.txt").is_file());
    }
}
