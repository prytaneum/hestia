//! Reading a save world's own description out of its `level.dat`.
//!
//! A directory listing only ever yields folder names, and a folder name is not
//! the world: the player names a world in-game (`LevelName`), and only the save
//! knows which version wrote it, how it plays, or when it was last opened. So a
//! world is described from `level.dat` — gzipped NBT, one small compound —
//! rather than inferred from the filesystem.
//!
//! Every field is best-effort by design. Saves span more than a decade of
//! formats: an old one carries no `Version`, a corrupt or half-written one
//! cannot be parsed at all, and a world being written by a running game may be
//! caught mid-flush. None of that should hide a world from a listing, so a
//! failure yields the folder alone with `read: false` and the caller reports
//! that rather than inventing values.

use std::path::Path;

use proto::instance::{Difficulty, GameMode, WorldInfo};
use serde::Deserialize;

const LEVEL_DAT: &str = "level.dat";
const ICON: &str = "icon.png";

/// `level.dat`'s root compound. Only the fields we surface are named; NBT
/// carries far more (the whole player, world-gen settings, gamerules) and
/// `fastnbt` skips what is not asked for.
#[derive(Deserialize)]
struct Level {
    #[serde(rename = "Data")]
    data: LevelData,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LevelData {
    #[serde(rename = "LevelName")]
    level_name: String,
    #[serde(rename = "GameType")]
    game_type: i32,
    #[serde(rename = "Difficulty")]
    difficulty: i8,
    #[serde(rename = "hardcore")]
    hardcore: i8,
    #[serde(rename = "allowCommands")]
    allow_commands: i8,
    /// Milliseconds since the epoch.
    #[serde(rename = "LastPlayed")]
    last_played: i64,
    #[serde(rename = "Version")]
    version: Option<LevelVersion>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LevelVersion {
    #[serde(rename = "Name")]
    name: String,
}

/// Describe one save directory. `folder` is the name under `saves/`; the rest
/// comes from the save itself, or is left at its default when it cannot be read.
pub fn describe(saves_dir: &Path, folder: &str) -> WorldInfo {
    let dir = saves_dir.join(folder);
    let mut world = WorldInfo {
        folder: folder.to_string(),
        name: folder.to_string(),
        size_bytes: crate::usage::dir_size(&dir),
        icon: read_icon(&dir.join(ICON)),
        ..WorldInfo::default()
    };

    let Some(data) = read_level(&dir.join(LEVEL_DAT)) else {
        return world;
    };
    world.read = true;
    if !data.level_name.trim().is_empty() {
        world.name = data.level_name;
    }
    world.game_mode = game_mode(data.game_type);
    world.difficulty = difficulty(data.difficulty);
    world.hardcore = data.hardcore != 0;
    world.cheats = data.allow_commands != 0;
    // A save with no LastPlayed reads 0, which is not "1970" — it is unknown.
    world.last_played_unix = (data.last_played > 0).then_some(data.last_played / 1000);
    world.version = data.version.map(|v| v.name).unwrap_or_default();
    world
}

/// A save's own `icon.png`, base64-encoded; empty when it has none.
pub fn icon(saves_dir: &Path, folder: &str) -> String {
    read_icon(&saves_dir.join(folder).join(ICON))
}

fn read_level(path: &Path) -> Option<LevelData> {
    let bytes = std::fs::read(path).ok()?;
    // level.dat is gzipped, but a hand-unpacked save can be plain NBT; try the
    // decompressed form first and fall back rather than refusing to read it.
    let decoded = gunzip(&bytes);
    let source = decoded.as_deref().unwrap_or(&bytes);
    match fastnbt::from_bytes::<Level>(source) {
        Ok(level) => Some(level.data),
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "cannot read level.dat");
            None
        }
    }
}

fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .ok()
        .map(|_| out)
}

fn read_icon(path: &Path) -> String {
    use base64::Engine;
    match std::fs::read(path) {
        Ok(bytes) => base64::engine::general_purpose::STANDARD.encode(bytes),
        Err(_) => String::new(),
    }
}

fn game_mode(value: i32) -> GameMode {
    match value {
        1 => GameMode::Creative,
        2 => GameMode::Adventure,
        3 => GameMode::Spectator,
        _ => GameMode::Survival,
    }
}

// Pre-1.13 saves stored the same values, so the numbering is stable.
fn difficulty(value: i8) -> Difficulty {
    match value {
        0 => Difficulty::Peaceful,
        1 => Difficulty::Easy,
        3 => Difficulty::Hard,
        _ => Difficulty::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `level.dat`: the root compound with a `Data` compound holding
    /// the fields we read. Written by hand so the test owns the bytes rather
    /// than depending on a fixture save.
    fn level_dat(name: &str, game_type: i32, hardcore: bool) -> Vec<u8> {
        let mut nbt = Vec::new();
        // TAG_Compound "" (root)
        nbt.push(0x0a);
        nbt.extend_from_slice(&0u16.to_be_bytes());
        // TAG_Compound "Data"
        nbt.push(0x0a);
        push_name(&mut nbt, "Data");
        // TAG_String "LevelName"
        nbt.push(0x08);
        push_name(&mut nbt, "LevelName");
        nbt.extend_from_slice(&(name.len() as u16).to_be_bytes());
        nbt.extend_from_slice(name.as_bytes());
        // TAG_Int "GameType"
        nbt.push(0x03);
        push_name(&mut nbt, "GameType");
        nbt.extend_from_slice(&game_type.to_be_bytes());
        // TAG_Byte "hardcore"
        nbt.push(0x01);
        push_name(&mut nbt, "hardcore");
        nbt.push(u8::from(hardcore));
        // TAG_Long "LastPlayed" (millis)
        nbt.push(0x04);
        push_name(&mut nbt, "LastPlayed");
        nbt.extend_from_slice(&1_751_000_000_000i64.to_be_bytes());
        // TAG_Compound "Version" { TAG_String "Name" }
        nbt.push(0x0a);
        push_name(&mut nbt, "Version");
        nbt.push(0x08);
        push_name(&mut nbt, "Name");
        nbt.extend_from_slice(&6u16.to_be_bytes());
        nbt.extend_from_slice(b"1.21.1");
        nbt.push(0x00); // end Version
        nbt.push(0x00); // end Data
        nbt.push(0x00); // end root
        nbt
    }

    fn push_name(out: &mut Vec<u8>, name: &str) {
        out.extend_from_slice(&(name.len() as u16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn temp_saves(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-world-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    #[test]
    fn a_world_is_described_from_its_level_dat() {
        let saves = temp_saves("described");
        let world = saves.path().join("world-folder");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(
            world.join(LEVEL_DAT),
            gzip(&level_dat("My Survival Base", 0, true)),
        )
        .unwrap();
        std::fs::write(world.join(ICON), b"not really a png").unwrap();

        let info = describe(saves.path(), "world-folder");
        assert!(info.read);
        assert_eq!(info.folder, "world-folder");
        assert_eq!(
            info.name, "My Survival Base",
            "the display name, not the folder"
        );
        assert_eq!(info.version, "1.21.1");
        assert_eq!(info.game_mode, GameMode::Survival);
        assert!(info.hardcore);
        assert!(!info.cheats);
        assert_eq!(info.last_played_unix, Some(1_751_000_000));
        assert!(
            !info.icon.is_empty(),
            "the world's own thumbnail rides along"
        );
        assert!(info.size_bytes > 0);
    }

    #[test]
    fn an_unreadable_world_still_lists_as_its_folder() {
        let saves = temp_saves("unreadable");
        // A corrupt save, and one with no level.dat at all: neither may vanish
        // from a listing, and neither may claim values it does not have.
        for (folder, bytes) in [("corrupt", Some(&b"garbage"[..])), ("bare", None)] {
            let dir = saves.path().join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            if let Some(bytes) = bytes {
                std::fs::write(dir.join(LEVEL_DAT), bytes).unwrap();
            }
            let info = describe(saves.path(), folder);
            assert!(!info.read, "{folder} cannot be read");
            assert_eq!(info.name, folder, "falls back to the folder name");
            assert_eq!(info.version, "");
            assert_eq!(info.last_played_unix, None);
        }
    }

    #[test]
    fn plain_nbt_is_read_too() {
        let saves = temp_saves("plain");
        let world = saves.path().join("unpacked");
        std::fs::create_dir_all(&world).unwrap();
        // A hand-unpacked save is not gzipped; read it rather than refuse it.
        std::fs::write(world.join(LEVEL_DAT), level_dat("Flat", 1, false)).unwrap();

        let info = describe(saves.path(), "unpacked");
        assert!(info.read);
        assert_eq!(info.name, "Flat");
        assert_eq!(info.game_mode, GameMode::Creative);
    }
}
