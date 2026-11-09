//! Bedrock worlds: a folder under `data/worlds/` holding `level.dat`,
//! `levelname.txt` and `world_icon.jpeg`, described the way
//! [`minecraft::world`](crate::minecraft::world) describes a Java save — every
//! field best-effort, so a damaged world still lists as its folder (0022).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use proto::instance::{Difficulty, GameMode, WorldInfo};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use super::nbt::{self, Level};
use crate::cancel::Job;
use crate::minecraft::world::read_icon;

const LEVEL_DAT: &str = "level.dat";
const LEVEL_NAME_TXT: &str = "levelname.txt";
const ICON: &str = "world_icon.jpeg";
const STAGING: &str = ".import.part";
const MAX_SLUG_CHARS: usize = 64;

/// The size of the `level.dat` header: a storage version and the payload length.
const HEADER_LEN: usize = 8;

pub fn describe(worlds_dir: &Path, folder: &str) -> WorldInfo {
    let dir = worlds_dir.join(folder);
    let mut world = WorldInfo {
        folder: folder.to_string(),
        name: folder.to_string(),
        size_bytes: crate::usage::dir_size(&dir),
        icon: read_icon(&dir.join(ICON)),
        ..WorldInfo::default()
    };
    if let Some(name) = std::fs::read_to_string(dir.join(LEVEL_NAME_TXT))
        .ok()
        .and_then(|text| text.lines().next().map(|line| line.trim().to_string()))
        .filter(|name| !name.is_empty())
    {
        world.name = name;
    }

    let Some(level) = read_level(&dir.join(LEVEL_DAT)) else {
        return world;
    };
    world.read = true;
    if let Some(name) = level.name.filter(|name| !name.trim().is_empty()) {
        world.name = name;
    }
    world.game_mode = game_mode(level.game_type.unwrap_or_default());
    world.difficulty = difficulty(level.difficulty.unwrap_or_default());
    // Bedrock stores LastPlayed in seconds, unlike Java's milliseconds.
    world.last_played_unix = level.last_played.filter(|seconds| *seconds > 0);
    world.version = level
        .last_opened_with
        .map(|parts| {
            let parts: Vec<String> = parts.iter().take(3).map(i32::to_string).collect();
            parts.join(".")
        })
        .unwrap_or_default();
    world
}

/// Every world folder, the active one first and then the most recently played.
pub fn list(worlds_dir: &Path, active: &str) -> Vec<WorldInfo> {
    let mut worlds: Vec<WorldInfo> = std::fs::read_dir(worlds_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|folder| !folder.starts_with('.'))
        .map(|folder| describe(worlds_dir, &folder))
        .collect();
    worlds.sort_by(|a, b| {
        (a.folder != active)
            .cmp(&(b.folder != active))
            .then(b.last_played_unix.cmp(&a.last_played_unix))
            .then(a.folder.cmp(&b.folder))
    });
    worlds
}

/// Unpack a `.mcworld` into `data/worlds/<slug>` and describe it. The slug comes
/// from the world's own name and never collides with an existing folder. The
/// world is extracted beside its destination and renamed into place, so a
/// failure or a cancel leaves no partial folder.
pub fn import(data_dir: &Path, archive: &Path, job: &Job<'_>) -> Result<WorldInfo> {
    let not_a_world = |reason| ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Zip,
        reason,
    };
    let is_mcworld = archive
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mcworld"));
    if !is_mcworld || !archive.is_file() {
        return Err(not_a_world(ArchiveFault::NotAnArchive).into());
    }
    let root = world_root(archive)?;

    let worlds = data_dir.join("worlds");
    let staging = worlds.join(STAGING);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&worlds)
        .with_context(|| format!("cannot create {}", worlds.display()))?;

    let placed = unpack(archive, &root, &worlds, &staging, job);
    if placed.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    let folder = placed?;
    tracing::info!(folder, "imported a bedrock world");
    Ok(describe(&worlds, &folder))
}

fn unpack(
    archive: &Path,
    root: &Path,
    worlds: &Path,
    staging: &Path,
    job: &Job<'_>,
) -> Result<String> {
    crate::extract::extract_zip_under(archive, root, staging, |done, total| {
        job.check()?;
        job.report(&ProvisionProgress {
            phase: ProvisionPhase::Extract,
            current: done,
            total,
            ..ProvisionProgress::default()
        });
        Ok(())
    })?;
    job.check()?;

    let described = describe(worlds, STAGING);
    let name = match described.name.as_str() {
        STAGING => archive
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
        name => name.to_string(),
    };
    let folder = free_folder(worlds, &slug(&name));
    std::fs::rename(staging, worlds.join(&folder))
        .with_context(|| format!("cannot move the world into {folder}"))?;
    Ok(folder)
}

/// Where `level.dat` sits in the archive: its root, or its one top folder.
/// A member path that would escape the destination refuses the whole archive.
fn world_root(archive: &Path) -> Result<PathBuf> {
    let invalid = |reason| ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Zip,
        reason,
    };
    let file = std::fs::File::open(archive)
        .with_context(|| format!("cannot open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| invalid(ArchiveFault::NotAnArchive))?;

    let mut nested = Vec::new();
    let mut at_root = false;
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|_| invalid(ArchiveFault::NotAnArchive))?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| invalid(ArchiveFault::UnsafePath))?;
        if entry.is_dir() || path.file_name().is_none_or(|name| name != LEVEL_DAT) {
            continue;
        }
        match path.components().count() {
            1 => at_root = true,
            2 => nested.push(path.parent().unwrap_or(Path::new("")).to_path_buf()),
            _ => {}
        }
    }
    match (at_root, nested.as_slice()) {
        (true, _) => Ok(PathBuf::new()),
        (false, [only]) => Ok(only.clone()),
        _ => Err(invalid(ArchiveFault::WorldMissing).into()),
    }
}

/// A folder name safe to hand to the server as `level-name`.
fn slug(name: &str) -> String {
    let mapped: String = name
        .chars()
        .map(
            |c| match c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                true => c,
                false => '-',
            },
        )
        .take(MAX_SLUG_CHARS)
        .collect();
    match mapped.trim_matches(|c| c == '-' || c == ' ') {
        "" => "world".to_string(),
        trimmed => trimmed.to_string(),
    }
}

fn free_folder(worlds: &Path, wanted: &str) -> String {
    let mut candidate = wanted.to_string();
    let mut n = 2;
    while worlds.join(&candidate).exists() {
        candidate = format!("{wanted}-{n}");
        n += 1;
    }
    candidate
}

fn read_level(path: &Path) -> Option<Level> {
    let bytes = std::fs::read(path).ok()?;
    let payload = payload(&bytes)?;
    nbt::read_level(payload)
        .map_err(|error| tracing::debug!(path = %path.display(), %error, "cannot read level.dat"))
        .ok()
}

fn payload(bytes: &[u8]) -> Option<&[u8]> {
    let length = u32::from_le_bytes(bytes.get(4..HEADER_LEN)?.try_into().ok()?);
    bytes.get(HEADER_LEN..HEADER_LEN.checked_add(usize::try_from(length).ok()?)?)
}

fn game_mode(value: i32) -> GameMode {
    match value {
        1 => GameMode::Creative,
        2 => GameMode::Adventure,
        3 | 6 => GameMode::Spectator,
        _ => GameMode::Survival,
    }
}

fn difficulty(value: i32) -> Difficulty {
    match value {
        0 => Difficulty::Peaceful,
        1 => Difficulty::Easy,
        3 => Difficulty::Hard,
        _ => Difficulty::Normal,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use proto::error::ErrorInfo;
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::cancel::Cancel;
    use crate::minecraft::bedrock::nbt::tests::level_payload;

    fn level_dat(world: &str) -> Vec<u8> {
        let payload = level_payload(world);
        let mut bytes = 10u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn mcworld(dir: &Path, name: &str, members: &[(&str, Vec<u8>)]) -> PathBuf {
        let path = dir.join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (member, bytes) in members {
            zip.start_file(*member, SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn run_import(data: &Path, archive: &Path) -> Result<WorldInfo> {
        import(data, archive, &Job::new(&|_| {}, &Cancel::new()))
    }

    #[test]
    fn a_world_is_described_from_its_level_dat() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("Bedrock level");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(world.join(LEVEL_DAT), level_dat("My Realm")).unwrap();
        std::fs::write(world.join(ICON), b"jpeg").unwrap();

        let info = describe(dir.path(), "Bedrock level");
        assert!(info.read);
        assert_eq!(info.folder, "Bedrock level");
        assert_eq!(info.name, "My Realm");
        assert_eq!(info.version, "1.21.44");
        assert_eq!(info.game_mode, GameMode::Creative);
        assert_eq!(info.difficulty, Difficulty::Hard);
        assert_eq!(info.last_played_unix, Some(1_751_000_000));
        assert!(!info.icon.is_empty());
    }

    #[test]
    fn a_corrupt_world_lists_as_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        for (folder, bytes) in [("corrupt", Some(&b"garbage"[..])), ("bare", None)] {
            let world = dir.path().join(folder);
            std::fs::create_dir_all(&world).unwrap();
            if let Some(bytes) = bytes {
                std::fs::write(world.join(LEVEL_DAT), bytes).unwrap();
            }
            let info = describe(dir.path(), folder);
            assert!(!info.read);
            assert_eq!(info.name, folder);
        }
    }

    #[test]
    fn the_active_world_lists_first_then_newest() {
        let dir = tempfile::tempdir().unwrap();
        for folder in ["a", "b", "active"] {
            std::fs::create_dir_all(dir.path().join(folder)).unwrap();
        }
        std::fs::create_dir_all(dir.path().join(".import.part")).unwrap();
        std::fs::write(dir.path().join("b").join(LEVEL_DAT), level_dat("B")).unwrap();

        let folders: Vec<String> = list(dir.path(), "active")
            .into_iter()
            .map(|w| w.folder)
            .collect();
        assert_eq!(folders, ["active", "b", "a"]);
    }

    #[test]
    fn a_world_at_the_archive_root_or_in_one_folder_imports() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let level = level_dat("Root World");
        let rooted = mcworld(
            dir.path(),
            "r.mcworld",
            &[("level.dat", level.clone()), ("db/x", vec![1])],
        );
        let folded = mcworld(
            dir.path(),
            "f.mcworld",
            &[("Top/level.dat", level), ("Top/db/x", vec![1])],
        );

        let first = run_import(&data, &rooted).unwrap();
        assert_eq!(first.folder, "Root World");
        assert!(first.read);
        assert!(data.join("worlds/Root World/db/x").is_file());

        let second = run_import(&data, &folded).unwrap();
        assert_eq!(
            second.folder, "Root World-2",
            "a collision gets its own folder"
        );
        assert!(data.join("worlds/Root World-2/db/x").is_file());
        assert!(!data.join("worlds").join(STAGING).exists());
    }

    #[test]
    fn an_unsafe_or_worldless_archive_is_refused_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let escape = mcworld(dir.path(), "e.mcworld", &[("../level.dat", level_dat("E"))]);
        let empty = mcworld(dir.path(), "n.mcworld", &[("db/x", vec![1])]);
        let two = mcworld(
            dir.path(),
            "t.mcworld",
            &[
                ("A/level.dat", level_dat("A")),
                ("B/level.dat", level_dat("B")),
            ],
        );
        let wrong_ext = mcworld(dir.path(), "w.zip", &[("level.dat", level_dat("W"))]);

        for (archive, fault) in [
            (escape, ArchiveFault::UnsafePath),
            (empty, ArchiveFault::WorldMissing),
            (two, ArchiveFault::WorldMissing),
            (wrong_ext, ArchiveFault::NotAnArchive),
        ] {
            let error = run_import(&data, &archive).unwrap_err();
            match error.downcast_ref::<ErrorInfo>() {
                Some(ErrorInfo::ArchiveInvalid { reason, .. }) => assert_eq!(*reason, fault),
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(!data.join("worlds").exists());
    }

    #[test]
    fn a_cancelled_import_leaves_no_folder() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let archive = mcworld(dir.path(), "c.mcworld", &[("level.dat", level_dat("C"))]);
        let cancel = Cancel::new();
        cancel.cancel();

        assert!(import(&data, &archive, &Job::new(&|_| {}, &cancel)).is_err());
        assert_eq!(std::fs::read_dir(data.join("worlds")).unwrap().count(), 0);
    }

    #[test]
    fn slugs_are_safe_folder_names() {
        assert_eq!(slug("My World!"), "My World");
        assert_eq!(slug("a=b\nc"), "a-b-c");
        assert_eq!(slug("???"), "world");
    }
}
