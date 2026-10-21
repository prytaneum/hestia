//! An entry's game directory as a tree a front-end browses and edits. Every
//! path crossing this boundary is relative to the root and `/`-separated;
//! nothing a caller names can resolve outside it.

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};
use proto::error::{ErrorInfo, Field, FileFault, IoOp, Reason};
use proto::files::FileEntry;

use crate::transfer::archive;

/// The most a file is read or written whole at — an editor's worth, not a
/// world's.
const EDITABLE_BYTES: u64 = 2 * 1024 * 1024;

pub struct FileTree {
    root: PathBuf,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        FileTree { root }
    }

    /// The entries of the directory at `path`, directories first.
    pub fn list(&self, path: &str) -> Result<Vec<FileEntry>> {
        let dir = self.resolve(path)?;
        if !dir.is_dir() {
            bail!(self.missing_or(path, &dir, FileFault::NotADirectory));
        }
        let parent = wire_path(Path::new(path));
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(io(IoOp::Read, path))? {
            let entry = entry.map_err(io(IoOp::Read, path))?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                tracing::debug!(path = %entry.path().display(), "skipping a non-UTF-8 name");
                continue;
            };
            // A dangling link has no metadata to show; it is simply not listed.
            let Ok(metadata) = std::fs::metadata(entry.path()) else {
                continue;
            };
            entries.push(FileEntry {
                path: match parent.is_empty() {
                    true => name.clone(),
                    false => format!("{parent}/{name}"),
                },
                name,
                dir: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
                modified_unix: common::modified_unix(&metadata).unwrap_or(0),
            });
        }
        entries.sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(entries)
    }

    pub fn read(&self, path: &str) -> Result<String> {
        let file = self.resolve_entry(path)?;
        let metadata = std::fs::metadata(&file).map_err(io(IoOp::Read, path))?;
        if metadata.is_dir() {
            bail!(rejected(path, FileFault::IsADirectory));
        }
        if metadata.len() > EDITABLE_BYTES {
            bail!(rejected(path, FileFault::TooLarge));
        }
        let bytes = std::fs::read(&file).map_err(io(IoOp::Read, path))?;
        String::from_utf8(bytes).map_err(|_| rejected(path, FileFault::NotText).into())
    }

    /// Create the file or replace its content, staged beside it and renamed in.
    pub fn write(&self, path: &str, content: &str) -> Result<()> {
        let file = self.resolve_entry(path)?;
        if file.is_dir() {
            bail!(rejected(path, FileFault::IsADirectory));
        }
        if content.len() as u64 > EDITABLE_BYTES {
            bail!(rejected(path, FileFault::TooLarge));
        }
        create_parent(&file, path)?;
        common::atomic::write(&file, content.as_ref()).map_err(io(IoOp::Write, path))?;
        tracing::debug!(path, bytes = content.len(), "file written");
        Ok(())
    }

    pub fn mkdir(&self, path: &str) -> Result<()> {
        let dir = self.resolve_entry(path)?;
        if dir.exists() {
            bail!(rejected(path, FileFault::AlreadyExists));
        }
        std::fs::create_dir_all(&dir).map_err(io(IoOp::Create, path))?;
        Ok(())
    }

    /// Rename or move. Never replaces what is already at `to`.
    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (source, target) = self.transfer(from, to)?;
        std::fs::rename(&source, &target).map_err(io(IoOp::Move, from))?;
        tracing::debug!(from, to, "file moved");
        Ok(())
    }

    pub fn copy(&self, from: &str, to: &str) -> Result<()> {
        let (source, target) = self.transfer(from, to)?;
        copy_tree(&source, &target).map_err(io(IoOp::Copy, from))?;
        tracing::debug!(from, to, "file copied");
        Ok(())
    }

    pub fn remove(&self, paths: &[String]) -> Result<()> {
        let targets = paths
            .iter()
            .map(|path| Ok((path, self.existing(path)?)))
            .collect::<Result<Vec<_>>>()?;
        for (path, target) in targets {
            match target.is_dir() {
                true => std::fs::remove_dir_all(&target),
                false => std::fs::remove_file(&target),
            }
            .map_err(io(IoOp::Remove, path))?;
            tracing::info!(path, "file removed");
        }
        Ok(())
    }

    /// Copy files from elsewhere on the machine into the directory at `path`.
    /// A file already there under the same name is replaced, as an upload
    /// replaces it; a directory is merged into.
    pub fn import(&self, path: &str, sources: &[String]) -> Result<()> {
        let dir = self.resolve(path)?;
        if !dir.is_dir() {
            bail!(self.missing_or(path, &dir, FileFault::NotADirectory));
        }
        for source in sources {
            let from = Path::new(source);
            if !from.is_absolute() {
                bail!(ErrorInfo::InvalidValue {
                    field: Field::Path,
                    reason: Reason::AbsolutePath,
                });
            }
            let Some(name) = from.file_name() else {
                bail!(ErrorInfo::FileNotFound {
                    path: source.clone()
                });
            };
            if !from.exists() {
                bail!(ErrorInfo::FileNotFound {
                    path: source.clone()
                });
            }
            copy_tree(from, &dir.join(name)).map_err(io(IoOp::Copy, source))?;
            tracing::info!(source, into = path, "file imported");
        }
        Ok(())
    }

    /// Write `paths` into a new `.zip` at `to`, each under its own name.
    pub fn compress(&self, paths: &[String], to: &str) -> Result<()> {
        let target = self.resolve_entry(to)?;
        if !to.ends_with(".zip") {
            bail!(rejected(to, FileFault::ArchiveFormat));
        }
        if target.exists() {
            bail!(rejected(to, FileFault::AlreadyExists));
        }
        let sources = paths
            .iter()
            .map(|path| self.existing(path))
            .collect::<Result<Vec<_>>>()?;

        let part = common::atomic::staged(&target);
        let mut writer = archive::Writer::create(&target)?;
        for source in &sources {
            let Some(name) = source.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !source.is_dir() {
                writer.add_file(name, source)?;
                continue;
            }
            // The archive may be written into a directory it is archiving.
            let members = archive::plan(source, &format!("{name}/"), &|_| true);
            for member in members {
                if member.source != part && member.source != target {
                    writer.add_file(&member.name, &member.source)?;
                }
            }
        }
        let written = writer.finish()?;
        tracing::info!(to, files = written.files, "files compressed");
        Ok(())
    }

    /// Unpack the `.zip` or `.tar.gz` at `from` into the directory at `to`.
    pub fn extract(&self, from: &str, to: &str) -> Result<()> {
        let source = self.existing(from)?;
        let supported = [".zip", ".tar.gz", ".tgz"]
            .iter()
            .any(|extension| from.ends_with(extension));
        if source.is_dir() || !supported {
            bail!(rejected(from, FileFault::ArchiveFormat));
        }
        let target = self.resolve(to)?;
        if target.exists() && !target.is_dir() {
            bail!(rejected(to, FileFault::NotADirectory));
        }
        crate::extract::extract_archive(&source, &target).map_err(|e| ErrorInfo::Io {
            operation: IoOp::Extract,
            detail: format!("{from}: {e:#}"),
        })?;
        tracing::info!(from, to, "archive extracted");
        Ok(())
    }

    /// Both ends of a move or copy: a source that exists and a free target.
    fn transfer(&self, from: &str, to: &str) -> Result<(PathBuf, PathBuf)> {
        let source = self.existing(from)?;
        let target = self.resolve_entry(to)?;
        if target.exists() {
            bail!(rejected(to, FileFault::AlreadyExists));
        }
        if target.starts_with(&source) {
            bail!(rejected(to, FileFault::IntoItself));
        }
        create_parent(&target, to)?;
        Ok((source, target))
    }

    fn existing(&self, path: &str) -> Result<PathBuf> {
        let target = self.resolve_entry(path)?;
        if !target.exists() {
            bail!(ErrorInfo::FileNotFound {
                path: path.to_string()
            });
        }
        Ok(target)
    }

    /// A location below the root — never the root itself, which is not the
    /// caller's to rename, replace or remove.
    fn resolve_entry(&self, path: &str) -> Result<PathBuf> {
        let target = self.resolve(path)?;
        if target == self.root {
            bail!(rejected(path, FileFault::OutsideEntry));
        }
        Ok(target)
    }

    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let relative = Path::new(path);
        if relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!(rejected(path, FileFault::OutsideEntry));
        }
        let target = self.root.join(relative);
        // A link inside the tree may point anywhere, so the lexical check is
        // not enough: whatever part of the path already exists has to resolve
        // back under the root.
        let root = self.root.canonicalize().map_err(io(IoOp::Open, path))?;
        let real = target
            .ancestors()
            .find_map(|ancestor| ancestor.canonicalize().ok())
            .unwrap_or_else(|| root.clone());
        if !real.starts_with(&root) {
            tracing::warn!(path, "refused a path resolving outside the tree");
            bail!(rejected(path, FileFault::OutsideEntry));
        }
        Ok(target)
    }

    fn missing_or(&self, path: &str, target: &Path, fault: FileFault) -> ErrorInfo {
        match target.exists() {
            true => rejected(path, fault),
            false => ErrorInfo::FileNotFound {
                path: path.to_string(),
            },
        }
    }
}

fn rejected(path: &str, reason: FileFault) -> ErrorInfo {
    ErrorInfo::FileRejected {
        path: path.to_string(),
        reason,
    }
}

fn io(operation: IoOp, path: &str) -> impl Fn(std::io::Error) -> anyhow::Error + '_ {
    move |error| {
        match error.kind() {
            std::io::ErrorKind::NotFound => ErrorInfo::FileNotFound {
                path: path.to_string(),
            },
            _ => ErrorInfo::Io {
                operation,
                detail: format!("{path}: {error}"),
            },
        }
        .into()
    }
}

fn wire_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

fn create_parent(target: &Path, path: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io(IoOp::Create, path))?;
    }
    Ok(())
}

/// Copy a file, or a directory and everything under it. Links are not
/// followed: a tree linking to its own ancestor would never finish.
pub(crate) fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(from)?;
    if metadata.is_symlink() {
        tracing::debug!(path = %from.display(), "skipping a link while copying");
        return Ok(());
    }
    if !metadata.is_dir() {
        std::fs::copy(from, to)?;
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (tempfile::TempDir, FileTree) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data");
        std::fs::create_dir_all(root.join("world")).unwrap();
        std::fs::write(root.join("server.properties"), "motd=hi\n").unwrap();
        std::fs::write(root.join("world/level.dat"), [0xff, 0xfe, 0x00]).unwrap();
        (dir, FileTree::new(root))
    }

    fn fault(result: Result<impl std::fmt::Debug>) -> FileFault {
        match result.unwrap_err().downcast::<ErrorInfo>().unwrap() {
            ErrorInfo::FileRejected { reason, .. } => reason,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    fn names(tree: &FileTree, path: &str) -> Vec<String> {
        tree.list(path)
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect()
    }

    #[test]
    fn a_listing_leads_with_directories_and_carries_wire_paths() {
        let (_dir, tree) = tree();
        let root = tree.list("").unwrap();
        assert_eq!(
            root.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["world", "server.properties"]
        );
        assert!(root[0].dir);
        assert_eq!(root[1].size, 8);
        assert_eq!(tree.list("world").unwrap()[0].path, "world/level.dat");
    }

    #[test]
    fn a_path_cannot_climb_or_start_at_the_filesystem_root() {
        let (_dir, tree) = tree();
        for path in ["..", "world/../..", "/etc/passwd", "./world"] {
            assert_eq!(fault(tree.list(path)), FileFault::OutsideEntry, "{path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_tree_is_not_followed() {
        let (dir, tree) = tree();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, tree.root.join("escape")).unwrap();

        assert_eq!(fault(tree.read("escape/secret")), FileFault::OutsideEntry);
        assert_eq!(
            fault(tree.write("escape/planted", "x")),
            FileFault::OutsideEntry
        );
        assert!(!outside.join("planted").exists());
    }

    #[test]
    fn the_root_itself_is_never_a_target() {
        let (_dir, tree) = tree();
        assert_eq!(
            fault(tree.remove(&[String::new()])),
            FileFault::OutsideEntry
        );
        assert!(tree.root.is_dir());
    }

    #[test]
    fn a_write_creates_then_replaces_and_reads_back() {
        let (_dir, tree) = tree();
        tree.write("plugins/config.yml", "a: 1\n").unwrap();
        tree.write("plugins/config.yml", "a: 2\n").unwrap();
        assert_eq!(tree.read("plugins/config.yml").unwrap(), "a: 2\n");
        assert_eq!(names(&tree, "plugins"), ["config.yml"]);
    }

    #[test]
    fn a_binary_or_oversized_file_is_refused_rather_than_mangled() {
        let (_dir, tree) = tree();
        assert_eq!(fault(tree.read("world/level.dat")), FileFault::NotText);
        assert_eq!(fault(tree.read("world")), FileFault::IsADirectory);
        let huge = "x".repeat(EDITABLE_BYTES as usize + 1);
        assert_eq!(fault(tree.write("huge.txt", &huge)), FileFault::TooLarge);
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let (_dir, tree) = tree();
        let error = tree.read("nope.txt").unwrap_err();
        assert!(matches!(
            error.downcast::<ErrorInfo>().unwrap(),
            ErrorInfo::FileNotFound { path } if path == "nope.txt"
        ));
    }

    #[test]
    fn a_move_or_copy_never_replaces_or_nests_into_itself() {
        let (_dir, tree) = tree();
        tree.copy("world", "world-copy").unwrap();
        assert_eq!(names(&tree, "world-copy"), ["level.dat"]);
        assert_eq!(
            fault(tree.rename("world", "world-copy")),
            FileFault::AlreadyExists
        );
        assert_eq!(
            fault(tree.copy("world", "world/inner")),
            FileFault::IntoItself
        );
        tree.rename("world-copy", "backups/old").unwrap();
        assert_eq!(names(&tree, "backups/old"), ["level.dat"]);
    }

    #[test]
    fn a_removal_is_refused_whole_when_one_path_is_missing() {
        let (_dir, tree) = tree();
        let paths = ["server.properties".to_string(), "nope".to_string()];
        assert!(tree.remove(&paths).is_err());
        assert!(tree.root.join("server.properties").exists());
        tree.remove(&["world".to_string()]).unwrap();
        assert_eq!(names(&tree, ""), ["server.properties"]);
    }

    #[test]
    fn an_import_copies_in_and_replaces_a_file_of_the_same_name() {
        let (dir, tree) = tree();
        let plugin = dir.path().join("plugin.jar");
        std::fs::write(&plugin, "v1").unwrap();
        let source = [plugin.to_str().unwrap().to_string()];
        tree.mkdir("plugins").unwrap();
        tree.import("plugins", &source).unwrap();
        std::fs::write(&plugin, "v2").unwrap();
        tree.import("plugins", &source).unwrap();
        assert_eq!(tree.read("plugins/plugin.jar").unwrap(), "v2");
        assert!(tree.import("plugins", &["plugin.jar".to_string()]).is_err());
    }

    #[test]
    fn an_archive_round_trips_a_directory_and_a_file() {
        let (_dir, tree) = tree();
        let paths = ["world".to_string(), "server.properties".to_string()];
        tree.compress(&paths, "world/bundle.zip").unwrap();
        assert_eq!(names(&tree, "world"), ["bundle.zip", "level.dat"]);

        tree.extract("world/bundle.zip", "restored").unwrap();
        assert_eq!(names(&tree, "restored"), ["world", "server.properties"]);
        assert_eq!(names(&tree, "restored/world"), ["level.dat"]);
    }

    #[test]
    fn only_a_known_archive_format_is_written_or_unpacked() {
        let (_dir, tree) = tree();
        let paths = ["world".to_string()];
        assert_eq!(
            fault(tree.compress(&paths, "world.rar")),
            FileFault::ArchiveFormat
        );
        assert_eq!(
            fault(tree.extract("server.properties", "out")),
            FileFault::ArchiveFormat
        );
    }
}
