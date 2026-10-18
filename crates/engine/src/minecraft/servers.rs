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

/// The file's root compound. `servers` is the only tag the game writes.
#[derive(Serialize, Deserialize, Default)]
struct ServerList {
    servers: Vec<Entry>,
}

/// One list row in the file's own spelling. Every field is optional in
/// practice: entries written by older versions and by third-party editors omit
/// what they do not set, and an omission must not hide the row.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Entry {
    name: String,
    ip: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    icon: String,
    #[serde(rename = "acceptTextures", skip_serializing_if = "Option::is_none")]
    accept_textures: Option<i8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hidden: Option<i8>,
}

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
    match parse(&path(game_dir)) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!(instance_dir = %game_dir.display(), error = %e, "cannot read servers.dat");
            Vec::new()
        }
    }
}

/// The same read, but a parse failure is an error rather than an empty list —
/// the form a write must use, since rewriting the file from an empty list would
/// throw away a list we merely failed to decode.
pub fn read_strict(game_dir: &Path) -> Result<Vec<ServerEntry>> {
    parse(&path(game_dir))
}

fn parse(file: &Path) -> Result<Vec<ServerEntry>> {
    let Some(bytes) = bytes(file)? else {
        return Ok(Vec::new());
    };
    let list: ServerList = fastnbt::from_bytes(&bytes)
        .with_context(|| format!("{} is not a readable server list", file.display()))?;
    Ok(list.servers.into_iter().map(into_proto).collect())
}

pub fn write(game_dir: &Path, servers: &[ServerEntry]) -> Result<()> {
    let list = ServerList {
        servers: servers.iter().map(from_proto).collect(),
    };
    let bytes = fastnbt::to_bytes(&list).context("cannot encode the server list")?;
    commit(&path(game_dir), &bytes)
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

/// Rewrite the list into the order `order` names, each entry by name or
/// address. The visible entries are dealt back into their own slots in that
/// order; the game's hidden scratch rows are not part of the list anyone
/// arranges, so they stay exactly where they are.
///
/// `None` when `order` does not name each visible entry exactly once — the
/// list moved underneath the caller, and guessing at the intended arrangement
/// would silently reorder rows the caller never saw.
pub fn rearrange(servers: &[ServerEntry], order: &[String]) -> Option<Vec<ServerEntry>> {
    let mut slots: Vec<usize> = servers
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.hidden)
        .map(|(index, _)| index)
        .collect();
    if order.len() != slots.len() {
        return None;
    }

    let mut arranged = servers.to_vec();
    let mut taken = vec![false; servers.len()];
    for (slot, reference) in slots.drain(..).zip(order) {
        let found = find_available(servers, reference, &taken)?;
        taken[found] = true;
        arranged[slot] = servers[found].clone();
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

fn into_proto(entry: Entry) -> ServerEntry {
    ServerEntry {
        name: entry.name,
        address: entry.ip,
        icon: entry.icon,
        accept_textures: entry.accept_textures.unwrap_or(0) != 0,
        hidden: entry.hidden.unwrap_or(0) != 0,
    }
}

fn from_proto(entry: &ServerEntry) -> Entry {
    Entry {
        name: entry.name.clone(),
        ip: entry.address.clone(),
        icon: entry.icon.clone(),
        accept_textures: Some(i8::from(entry.accept_textures)),
        hidden: entry.hidden.then_some(1),
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
        write(dir.path(), &servers).unwrap();
        assert_eq!(read(dir.path()), servers);
    }

    #[test]
    fn a_missing_file_is_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path()).is_empty());
        assert!(read_strict(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_file_reads_empty_but_refuses_a_write_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(dir.path()), b"not nbt at all").unwrap();
        assert!(read(dir.path()).is_empty());
        assert!(read_strict(dir.path()).is_err());
    }

    #[test]
    fn a_list_takes_the_order_it_is_given() {
        let servers = vec![
            entry("A", "a.net"),
            entry("B", "b.net"),
            entry("C", "c.net"),
        ];
        let order = ["C".to_string(), "a.net".to_string(), "B".to_string()];
        assert_eq!(
            names(&rearrange(&servers, &order).unwrap()),
            ["C", "A", "B"]
        );
    }

    #[test]
    fn a_hidden_row_keeps_its_slot_and_is_never_named() {
        let hidden = ServerEntry {
            hidden: true,
            ..entry("scratch", "direct.example.net")
        };
        let servers = vec![entry("A", "a.net"), hidden, entry("B", "b.net")];
        let arranged =
            rearrange(&servers, &["B".to_string(), "A".to_string()]).expect("visible order");
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
        let arranged = rearrange(&servers, &order).expect("both placed");
        assert_eq!(
            arranged
                .iter()
                .map(|s| s.address.as_str())
                .collect::<Vec<_>>(),
            ["one.example.net", "two.example.net"]
        );
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
}
