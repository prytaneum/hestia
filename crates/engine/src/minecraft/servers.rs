//! The instance's multiplayer list — `servers.dat`, the file the in-game
//! "Add Server" screen writes.
//!
//! Unlike `level.dat` this one is **uncompressed** NBT, and unlike every other
//! file Hestia keeps, it is not ours: the running game holds the list in memory
//! and writes the whole file back when it exits, so a write made underneath a
//! session is lost. Callers are expected to say so rather than to refuse — the
//! warning is raised where the sessions are known (the engine flow).
//!
//! Reading is best-effort in the same spirit as a world: a missing file is an
//! empty list (a fresh instance has never opened multiplayer), and a file that
//! cannot be parsed is reported as an error only to a caller that is writing,
//! since overwriting a list we failed to understand would discard it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::instance::ServerEntry;
use serde::{Deserialize, Serialize};

pub const FILE: &str = "servers.dat";

pub type Row = HashMap<String, fastnbt::Value>;

#[derive(Serialize, Deserialize, Default)]
struct RawList {
    servers: Vec<Row>,
}

pub fn read_rows(game_dir: &Path) -> Result<Vec<Row>> {
    let Some(bytes) = bytes(&path(game_dir))? else {
        return Ok(Vec::new());
    };
    let list: RawList = fastnbt::from_bytes(&bytes)
        .with_context(|| format!("{} is not a readable server list", path(game_dir).display()))?;
    Ok(list.servers)
}

pub fn write_rows(game_dir: &Path, rows: &[Row]) -> Result<()> {
    let list = RawList {
        servers: rows.to_vec(),
    };
    let bytes = fastnbt::to_bytes(&list).context("cannot encode the server list")?;
    commit(&path(game_dir), &bytes)
}

fn bytes(file: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(file) {
        Ok(bytes) if bytes.is_empty() => Ok(None),
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", file.display())),
    }
}

fn commit(file: &Path, bytes: &[u8]) -> Result<()> {
    common::atomic::write(file, bytes).with_context(|| format!("cannot write {}", file.display()))
}

pub fn path(game_dir: &Path) -> PathBuf {
    game_dir.join(FILE)
}

/// The instance's multiplayer list in the file's own order. A missing file is
/// an empty list; an unreadable one logs and reads as empty, so a corrupt
/// `servers.dat` cannot make the instance itself unlistable.
pub fn read(game_dir: &Path) -> Vec<ServerEntry> {
    match read_rows(game_dir) {
        Ok(rows) => entries(&rows),
        Err(e) => {
            tracing::debug!(instance_dir = %game_dir.display(), error = %e, "cannot read servers.dat");
            Vec::new()
        }
    }
}

pub fn entries(rows: &[Row]) -> Vec<ServerEntry> {
    rows.iter().map(into_proto).collect()
}

/// `row` with the fields Hestia models set from `entry`; every other tag the
/// file carries is left as it was.
pub fn overlay(mut row: Row, entry: &ServerEntry) -> Row {
    let text = |value: &str| fastnbt::Value::String(value.to_string());
    row.insert("name".into(), text(&entry.name));
    row.insert("ip".into(), text(&entry.address));
    if entry.icon.is_empty() {
        row.remove("icon");
    } else {
        row.insert("icon".into(), text(&entry.icon));
    }
    row.insert(
        "acceptTextures".into(),
        fastnbt::Value::Byte(i8::from(entry.accept_textures)),
    );
    if entry.hidden {
        row.insert("hidden".into(), fastnbt::Value::Byte(1));
    } else {
        row.remove("hidden");
    }
    row
}

/// Find an entry by name or by address, case-insensitively — the two things a
/// person has in front of them when naming one.
pub fn find(servers: &[ServerEntry], reference: &str) -> Option<usize> {
    let reference = reference.trim();
    servers
        .iter()
        .position(|s| s.name.eq_ignore_ascii_case(reference))
        .or_else(|| {
            servers
                .iter()
                .position(|s| s.address.eq_ignore_ascii_case(reference))
        })
}

/// The permutation (new slot to old index) that puts the list into the order
/// `order` names, each entry by name or address. The visible entries are dealt back into their own slots in that
/// order; the game's hidden scratch rows are not part of the list anyone
/// arranges, so they stay exactly where they are.
///
/// `None` when `order` does not name each visible entry exactly once — the
/// list moved underneath the caller, and guessing at the intended arrangement
/// would silently reorder rows the caller never saw.
pub fn rearrange(servers: &[ServerEntry], order: &[String]) -> Option<Vec<usize>> {
    let mut slots: Vec<usize> = servers
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.hidden)
        .map(|(index, _)| index)
        .collect();
    if order.len() != slots.len() {
        return None;
    }

    let mut arranged: Vec<usize> = (0..servers.len()).collect();
    let mut taken = vec![false; servers.len()];
    for (slot, reference) in slots.drain(..).zip(order) {
        let found = find_available(servers, reference, &taken)?;
        taken[found] = true;
        arranged[slot] = found;
    }
    Some(arranged)
}

/// [`find`] over the visible entries not yet placed — two entries may carry the
/// same name, and each reference in an arrangement claims one of them.
fn find_available(servers: &[ServerEntry], reference: &str, taken: &[bool]) -> Option<usize> {
    let reference = reference.trim();
    let available =
        |index: usize, entry: &ServerEntry| !taken[index] && !entry.hidden && !reference.is_empty();
    servers
        .iter()
        .enumerate()
        .position(|(index, entry)| {
            available(index, entry) && entry.name.eq_ignore_ascii_case(reference)
        })
        .or_else(|| {
            servers.iter().enumerate().position(|(index, entry)| {
                available(index, entry) && entry.address.eq_ignore_ascii_case(reference)
            })
        })
}

fn into_proto(row: &Row) -> ServerEntry {
    let text = |key: &str| match row.get(key) {
        Some(fastnbt::Value::String(value)) => value.clone(),
        _ => String::new(),
    };
    let flag = |key: &str| matches!(row.get(key), Some(fastnbt::Value::Byte(byte)) if *byte != 0);
    ServerEntry {
        name: text("name"),
        address: text("ip"),
        icon: text("icon"),
        accept_textures: flag("acceptTextures"),
        hidden: flag("hidden"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, address: &str) -> ServerEntry {
        ServerEntry {
            name: name.into(),
            address: address.into(),
            ..ServerEntry::default()
        }
    }

    #[test]
    fn a_list_round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let servers = vec![
            ServerEntry {
                accept_textures: true,
                icon: "aWNvbg==".into(),
                ..entry("Hermitcraft", "smp.example.net:25565")
            },
            entry("LAN box", "192.168.1.10"),
        ];
        let rows: Vec<Row> = servers.iter().map(|s| overlay(Row::new(), s)).collect();
        write_rows(dir.path(), &rows).unwrap();
        assert_eq!(read(dir.path()), servers);
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_empty());
        assert!(read_rows(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_file_reads_empty_but_refuses_a_write_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), b"not nbt at all").unwrap();
        assert!(read(dir.path()).is_empty());
        assert!(read_rows(dir.path()).is_err());
    }

    #[test]
    fn a_list_takes_the_order_it_is_given() {
        let servers = vec![
            entry("A", "a.net"),
            entry("B", "b.net"),
            entry("C", "c.net"),
        ];
        let order = ["C".to_string(), "a.net".to_string(), "B".to_string()];
        assert_eq!(names(&arrange(&servers, &order).unwrap()), ["C", "A", "B"]);
    }

    #[test]
    fn a_hidden_row_keeps_its_slot_and_is_never_named() {
        let hidden = ServerEntry {
            hidden: true,
            ..entry("scratch", "direct.example.net")
        };
        let servers = vec![entry("A", "a.net"), hidden, entry("B", "b.net")];
        let arranged =
            arrange(&servers, &["B".to_string(), "A".to_string()]).expect("visible order");
        assert_eq!(names(&arranged), ["B", "scratch", "A"]);
        assert!(arranged[1].hidden);
        assert!(rearrange(&servers, &["scratch".to_string(), "A".to_string()]).is_none());
    }

    #[test]
    fn an_order_naming_a_different_set_is_refused() {
        let servers = vec![entry("A", "a.net"), entry("B", "b.net")];
        for order in [
            vec!["A".to_string()],
            vec!["A".to_string(), "A".to_string()],
            vec!["A".to_string(), "B".to_string(), "C".to_string()],
            vec!["A".to_string(), "gone".to_string()],
        ] {
            assert!(rearrange(&servers, &order).is_none(), "{order:?}");
        }
    }

    #[test]
    fn two_entries_sharing_a_name_each_claim_one_slot() {
        let servers = vec![
            entry("SMP", "one.example.net"),
            entry("SMP", "two.example.net"),
        ];
        let order = ["SMP".to_string(), "two.example.net".to_string()];
        let arranged = arrange(&servers, &order).expect("both placed");
        assert_eq!(
            arranged
                .iter()
                .map(|s| s.address.as_str())
                .collect::<Vec<_>>(),
            ["one.example.net", "two.example.net"]
        );
    }

    fn arrange(servers: &[ServerEntry], order: &[String]) -> Option<Vec<ServerEntry>> {
        let permutation = rearrange(servers, order)?;
        Some(
            permutation
                .into_iter()
                .map(|i| servers[i].clone())
                .collect(),
        )
    }

    fn names(servers: &[ServerEntry]) -> Vec<&str> {
        servers.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn an_entry_is_found_by_name_or_address_ignoring_case() {
        let servers = vec![entry("Hermitcraft", "smp.example.net")];
        assert_eq!(find(&servers, "hermitcraft"), Some(0));
        assert_eq!(find(&servers, "SMP.example.net"), Some(0));
        assert_eq!(find(&servers, "nothing"), None);
    }

    #[test]
    fn edits_keep_the_tags_this_build_does_not_model() {
        let mut carried = Row::new();
        carried.insert("acceptTextures".into(), fastnbt::Value::Byte(1));
        carried.insert("previewsChat".into(), fastnbt::Value::Byte(1));
        let kept = overlay(carried, &entry("Renamed", "a.net"));
        assert_eq!(kept.get("previewsChat"), Some(&fastnbt::Value::Byte(1)));
        assert_eq!(kept.get("acceptTextures"), Some(&fastnbt::Value::Byte(0)));
        assert_eq!(into_proto(&kept).name, "Renamed");
    }
}
