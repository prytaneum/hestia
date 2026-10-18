//! On-disk records of supervised processes. `<dir>/<id>/record.json` is the
//! "re-adopt me" marker the next daemon recovers from. It stays after the process
//! ends, gaining an `ended` section, so a finished process's directory is
//! labelled and its logs are never mistaken for a stray: a directory with no
//! readable record is the only thing the startup sweep deletes.

use std::fs;
use std::path::Path;

use proto::process::{ProcessInfo, ProcessSpec, ProcessState};
use serde::{Deserialize, Serialize};

use super::{identity, secrets};

const FILE: &str = "record.json";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProcessRecord {
    pub id: String,
    pub pid: u32,
    pub pid_started: u64,
    pub spec: ProcessSpec,
    pub started_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ended>,
}

/// How a process ended, kept beside its logs so a post-mortem survives the
/// daemon that watched it.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Ended {
    pub state: ProcessState,
    /// `None` for an adopted process — the exit code of a process we did not
    /// spawn is unknowable, a documented gap rather than a missing field.
    pub exit_code: Option<i32>,
    pub ended_unix: i64,
}

impl ProcessRecord {
    /// The spec is stored with its credentials masked; the real one only lives
    /// in memory long enough to spawn.
    pub fn for_spawn(spec: &ProcessSpec, pid: u32, started_unix: i64) -> Self {
        ProcessRecord {
            id: spec.id.clone(),
            pid,
            pid_started: identity::identify(pid).unwrap_or(0),
            spec: ProcessSpec {
                args: secrets::redact(&spec.args),
                ..spec.clone()
            },
            started_unix,
            ended: None,
        }
    }

    pub fn end(self, state: ProcessState, exit_code: Option<i32>, ended_unix: i64) -> Self {
        ProcessRecord {
            ended: Some(Ended {
                state,
                exit_code,
                ended_unix,
            }),
            ..self
        }
    }

    pub fn info(&self) -> ProcessInfo {
        let (state, exit_code) = self
            .ended
            .as_ref()
            .map_or((ProcessState::Running, None), |e| (e.state, e.exit_code));
        ProcessInfo {
            id: self.id.clone(),
            pid: self.pid,
            program: self.spec.program.clone(),
            args: self.spec.args.clone(),
            state,
            exit_code,
            started_unix: self.started_unix,
        }
    }
}

pub fn save(proc_dir: &Path, record: &ProcessRecord) {
    let path = proc_dir.join(FILE);
    let result = serde_json::to_vec_pretty(record)
        .map_err(std::io::Error::other)
        .and_then(|json| common::atomic::write_private(&path, &json));
    if let Err(e) = result {
        tracing::warn!(id = %record.id, "cannot persist process record: {e}");
    }
}

pub fn load(proc_dir: &Path) -> Option<ProcessRecord> {
    let path = proc_dir.join(FILE);
    let contents = fs::read(&path).ok()?;
    match serde_json::from_slice(&contents) {
        Ok(record) => Some(record),
        Err(e) => {
            tracing::warn!(path = %path.display(), "discarding malformed record: {e}");
            None
        }
    }
}

pub fn scan(dir: &Path) -> Vec<ProcessRecord> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| load(&entry.path()))
        .collect()
}

/// Every finished process under `dir`, newest first.
pub fn scan_finished(dir: &Path) -> Vec<ProcessRecord> {
    let mut finished: Vec<ProcessRecord> = scan(dir)
        .into_iter()
        .filter(|record| record.ended.is_some())
        .collect();
    finished.sort_by_key(|record| std::cmp::Reverse(record.ended.as_ref().map(|e| e.ended_unix)));
    finished
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-records-{name}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn spec(id: &str) -> ProcessSpec {
        ProcessSpec {
            id: id.into(),
            program: "java".into(),
            ..Default::default()
        }
    }

    #[test]
    fn save_scan_round_trip_keeps_credentials_off_disk() {
        let dir = temp_dir("round-trip");
        let proc_dir = dir.path().join("server-x");
        fs::create_dir_all(&proc_dir).unwrap();
        let spec = ProcessSpec {
            args: vec!["--accessToken".into(), "ey.a.real.jwt".into()],
            ..spec("server-x")
        };
        save(
            &proc_dir,
            &ProcessRecord::for_spawn(&spec, std::process::id(), 42),
        );

        let on_disk = fs::read_to_string(proc_dir.join(FILE)).unwrap();
        assert!(!on_disk.contains("ey.a.real.jwt"));

        let scanned = scan(dir.path());
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].id, "server-x");
        assert_eq!(scanned[0].started_unix, 42);
        assert!(scanned[0].ended.is_none());
        assert!(identity::is_same(scanned[0].pid, scanned[0].pid_started));
    }

    #[test]
    fn scan_skips_malformed_records() {
        let dir = temp_dir("malformed");
        let proc_dir = dir.path().join("broken");
        fs::create_dir_all(&proc_dir).unwrap();
        fs::write(proc_dir.join(FILE), b"not json").unwrap();
        assert!(scan(dir.path()).is_empty());
    }

    #[test]
    fn ending_a_record_keeps_it_known_and_its_spec_readable() {
        let dir = temp_dir("ended");
        let proc_dir = dir.path().join("server-x");
        fs::create_dir_all(&proc_dir).unwrap();

        let live = ProcessRecord::for_spawn(&spec("server-x"), 1234, 100);
        save(&proc_dir, &live);
        assert!(scan_finished(dir.path()).is_empty());

        save(&proc_dir, &live.end(ProcessState::Killed, Some(143), 200));
        let finished = scan_finished(dir.path());
        assert_eq!(finished.len(), 1);
        let info = finished[0].info();
        assert_eq!(info.state, ProcessState::Killed);
        assert_eq!(info.exit_code, Some(143));
        assert_eq!(finished[0].spec.program, "java");
    }

    #[test]
    fn finished_scan_is_newest_first() {
        let dir = temp_dir("order");
        for (id, ended) in [("a", 300), ("b", 100), ("c", 200)] {
            let proc_dir = dir.path().join(id);
            fs::create_dir_all(&proc_dir).unwrap();
            let record = ProcessRecord::for_spawn(&spec(id), 1, ended - 60);
            save(&proc_dir, &record.end(ProcessState::Exited, Some(0), ended));
        }
        let ids: Vec<String> = scan_finished(dir.path())
            .into_iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, ["a", "c", "b"]);
    }
}
