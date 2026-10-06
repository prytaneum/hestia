//! What an instance's sessions joined lately — the history behind the
//! cross-instance "recently played" list.
//!
//! The game reports only its latest join ([`crate::minecraft::quickplay`]), so
//! each one is folded in here as it is seen. The record sits in the entry root:
//! it follows the instance through a rename and goes when the instance does.

use std::cmp::Reverse;
use std::path::Path;

use anyhow::Result;
use proto::instance::{GameMode, QuickPlay};
use serde::{Deserialize, Serialize};

use crate::registry;
use crate::schema::Document;

/// How many targets are kept, per instance and in the list across all of them —
/// the official launcher's five slots.
pub(crate) const LIMIT: usize = 5;

pub(crate) const FILE: &str = "recent.json";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Play {
    pub target: QuickPlay,
    pub name: String,
    pub game_mode: GameMode,
    pub played_unix: i64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Recent {
    plays: Vec<Play>,
}

impl Document for Recent {
    const NAME: &'static str = FILE;
}

/// Most recent first.
pub(crate) fn load(entry_dir: &Path) -> Vec<Play> {
    registry::read_record::<Recent>(entry_dir)
        .map(|recent| recent.plays)
        .unwrap_or_default()
}

/// Fold `joins` into the history: a target already there moves up to its newer
/// time, a new one pushes the oldest out. Idempotent, so a log read twice (a
/// session still in the same world) writes nothing.
pub(crate) fn record(entry_dir: &Path, joins: Vec<Play>) -> Result<()> {
    let mut plays = load(entry_dir);
    let mut changed = false;
    for join in joins {
        match plays.iter_mut().find(|play| play.target == join.target) {
            Some(play) if play.played_unix >= join.played_unix => {}
            Some(play) => {
                *play = join;
                changed = true;
            }
            None => {
                plays.push(join);
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(());
    }
    plays.sort_by_key(|play| Reverse(play.played_unix));
    plays.truncate(LIMIT);
    registry::write_record(entry_dir, &Recent { plays })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(folder: &str, played_unix: i64) -> Play {
        Play {
            target: QuickPlay::World(folder.into()),
            name: folder.into(),
            game_mode: GameMode::Survival,
            played_unix,
        }
    }

    #[test]
    fn a_rejoined_target_moves_up_rather_than_appearing_twice() {
        let dir = tempfile::tempdir().expect("temp dir");
        record(dir.path(), vec![play("a", 10), play("b", 20)]).expect("record");
        record(dir.path(), vec![play("a", 30)]).expect("record");

        let folders: Vec<_> = load(dir.path()).into_iter().map(|p| p.name).collect();
        assert_eq!(folders, ["a", "b"]);
    }

    #[test]
    fn an_older_reading_of_a_known_target_changes_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        record(dir.path(), vec![play("a", 30)]).expect("record");
        record(dir.path(), vec![play("a", 10)]).expect("record");

        assert_eq!(load(dir.path()), [play("a", 30)]);
    }

    #[test]
    fn the_oldest_target_falls_off_past_the_limit() {
        let dir = tempfile::tempdir().expect("temp dir");
        let joins = (0..=LIMIT as i64)
            .map(|n| play(&format!("w{n}"), n + 1))
            .collect();
        record(dir.path(), joins).expect("record");

        let plays = load(dir.path());
        assert_eq!(plays.len(), LIMIT);
        assert!(plays.iter().all(|p| p.name != "w0"));
    }
}
