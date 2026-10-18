//! The game's own Quick Play log: what a session last joined.
//!
//! Given `--quickPlayPath`, a 1.20+ client rewrites that file every time it
//! enters a world, a server or a Realm — whether or not the launch asked for
//! one. It holds only the latest join (a one-element array), so it reads as
//! "what this session is in", never as a history; [`crate::recent`] keeps that.

use std::path::{Path, PathBuf};

use proto::instance::{GameMode, QuickPlay};
use serde::Deserialize;

use crate::recent::Play;

pub(crate) const ARGUMENT: &str = "--quickPlayPath";

const SUFFIX: &str = ".quickplay.json";

#[derive(Deserialize)]
struct Entry {
    #[serde(rename = "type")]
    kind: Kind,
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    gamemode: GameMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Singleplayer,
    Multiplayer,
    Realms,
}

/// One file per session, beside its log: concurrent sessions of an instance
/// would otherwise overwrite each other's latest join.
pub(crate) fn log_path(session_dir: &Path, session_seq: u32) -> PathBuf {
    session_dir.join(format!("session-{session_seq}{SUFFIX}"))
}

/// The latest join of every session that has written a log under
/// `session_dir`. A log caught mid-rewrite, or one this build cannot read, is
/// skipped: the next read sees it whole.
pub(crate) fn joins(session_dir: &Path) -> Vec<Play> {
    std::fs::read_dir(session_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(SUFFIX))
        })
        .filter_map(|path| read(&path))
        .collect()
}

/// The log's own `lastPlayedTime` is ignored in favour of the file's mtime:
/// they are the same moment, and one needs no date parser.
fn read(path: &Path) -> Option<Play> {
    let played_unix = common::modified_unix(&std::fs::metadata(path).ok()?)?;
    let bytes = std::fs::read(path).ok()?;
    let entry = match serde_json::from_slice::<Vec<Entry>>(&bytes) {
        Ok(entries) => entries.into_iter().next_back()?,
        Err(e) => {
            tracing::debug!(path = %path.display(), error = %e, "cannot read the quick play log");
            return None;
        }
    };
    Some(Play {
        target: match entry.kind {
            Kind::Singleplayer => QuickPlay::World(entry.id),
            Kind::Multiplayer => QuickPlay::Server(entry.id),
            Kind::Realms => QuickPlay::Realm(entry.id),
        },
        name: entry.name,
        game_mode: entry.gamemode,
        played_unix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_dir(logs: &[(u32, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for (seq, body) in logs {
            std::fs::write(log_path(dir.path(), *seq), body).expect("write the log");
        }
        dir
    }

    #[test]
    fn each_kind_of_join_reads_as_the_target_that_returns_to_it() {
        let dir = session_dir(&[
            (
                1,
                r#"[{"type":"singleplayer","id":"New World (2)","name":"Home","lastPlayedTime":"2026-10-06T10:00:00.000000Z","gamemode":"creative"}]"#,
            ),
            (
                2,
                r#"[{"type":"multiplayer","id":"smp.example.net:25566","name":"SMP","lastPlayedTime":"2026-10-06T10:00:00.000000Z","gamemode":"survival"}]"#,
            ),
            (
                3,
                r#"[{"type":"realms","id":"1234567","name":"Our Realm","lastPlayedTime":"2026-10-06T10:00:00.000000Z","gamemode":"adventure"}]"#,
            ),
        ]);

        let mut joins = joins(dir.path());
        joins.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(joins[0].target, QuickPlay::World("New World (2)".into()));
        assert_eq!(joins[0].game_mode, GameMode::Creative);
        assert_eq!(joins[1].target, QuickPlay::Realm("1234567".into()));
        assert_eq!(
            joins[2].target,
            QuickPlay::Server("smp.example.net:25566".into())
        );
        assert!(joins.iter().all(|join| join.played_unix > 0));
    }

    #[test]
    fn a_half_written_log_and_an_unrelated_file_are_skipped() {
        let dir = session_dir(&[(1, ""), (2, r#"[{"type":"lan","id":"x"}]"#), (3, "[]")]);
        std::fs::write(dir.path().join("session-1.log"), "noise").expect("write");

        assert!(joins(dir.path()).is_empty());
    }
}
