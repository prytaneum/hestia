//! Bedrock Dedicated Server's hot-backup protocol: `save hold`, poll
//! `save query` until the files are ready, copy each file truncated to the
//! length the server lists (LevelDB keeps appending while held), `save resume`.
//! This module parses the `save query` reply; the orchestration lives in the
//! backup flow.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

pub type Truncation = HashMap<PathBuf, u64>;

const READY: &str = "Data saved. Files are now ready to be copied.";
const WORLDS: &str = "worlds";

#[derive(Debug, thiserror::Error)]
pub enum HoldError {
    #[error("the server did not finish saving within {0} seconds")]
    NotReady(u64),
    #[error("the server's file list is malformed: {0:?}")]
    Malformed(String),
    #[error("the server listed an unsafe path: {0:?}")]
    UnsafePath(String),
}

/// `Ok(None)` while the server has not finished saving. Listed paths are
/// relative to the server's `worlds/` directory and come back prefixed with
/// `worlds/`, so they key against `data/`. Each is checked at the edge: only
/// plain relative components.
pub fn parse_query(reply: &str) -> Result<Option<Truncation>, HoldError> {
    let mut lines = reply.lines();
    let Some(marker) = lines.by_ref().find(|line| line.contains(READY)) else {
        return Ok(None);
    };
    let tail = marker.split_once(READY).map_or("", |(_, rest)| rest.trim());
    let list = if tail.is_empty() {
        lines
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("")
    } else {
        tail
    };
    if list.is_empty() {
        return Err(HoldError::Malformed(reply.to_string()));
    }
    list.split(", ")
        .map(parse_entry)
        .collect::<Result<_, _>>()
        .map(Some)
}

fn parse_entry(entry: &str) -> Result<(PathBuf, u64), HoldError> {
    let entry = entry.trim();
    let (path, length) = entry
        .rsplit_once(':')
        .ok_or_else(|| HoldError::Malformed(entry.to_string()))?;
    let length = length
        .trim()
        .parse()
        .map_err(|_| HoldError::Malformed(entry.to_string()))?;
    let relative = Path::new(path);
    let plain = !path.is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if !plain {
        return Err(HoldError::UnsafePath(path.to_string()));
    }
    Ok((Path::new(WORLDS).join(relative), length))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: &str = "Bedrock level/db/000005.ldb:1234, Bedrock level/level.dat:2911";

    fn expected() -> Truncation {
        Truncation::from([
            (PathBuf::from("worlds/Bedrock level/db/000005.ldb"), 1234),
            (PathBuf::from("worlds/Bedrock level/level.dat"), 2911),
        ])
    }

    #[test]
    fn parses_a_two_line_reply() {
        let reply = format!("Data saved. Files are now ready to be copied.\n{FILES}");
        assert_eq!(parse_query(&reply).unwrap(), Some(expected()));
    }

    #[test]
    fn tolerates_stripped_timestamps_and_trailing_whitespace() {
        let reply = format!("Data saved. Files are now ready to be copied. \r\n  {FILES}  \r\n");
        assert_eq!(parse_query(&reply).unwrap(), Some(expected()));
    }

    #[test]
    fn a_reply_before_the_files_are_ready_is_none() {
        assert_eq!(
            parse_query("A previous save has not been completed.").unwrap(),
            None
        );
        assert_eq!(parse_query("").unwrap(), None);
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for bad in ["../x:1", "/etc/passwd:1", "a/../../x:1"] {
            let reply = format!("Data saved. Files are now ready to be copied.\n{bad}");
            assert!(
                matches!(parse_query(&reply), Err(HoldError::UnsafePath(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_ready_reply_without_a_list_is_malformed() {
        assert!(matches!(
            parse_query("Data saved. Files are now ready to be copied."),
            Err(HoldError::Malformed(_))
        ));
    }
}
