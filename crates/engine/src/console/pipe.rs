//! A console over a process's stdin: the server answers into its captured log,
//! so a reply is whatever lands there after the command goes in.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use proto::error::{EntryKind, ErrorInfo, Field, Reason};
use proto::naming::server_process_id;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::process::{ProcessSupervisor, StdinError};

const POLL: Duration = Duration::from_millis(25);
pub(super) const QUIET: Duration = Duration::from_millis(250);
pub(super) const DEADLINE: Duration = Duration::from_secs(2);
pub(super) const MAX_REPLY_BYTES: usize = 64 * 1024;

pub(super) async fn command(
    processes: &Arc<ProcessSupervisor>,
    server: &str,
    command: &str,
) -> Result<String> {
    let process = server_process_id(server);
    let log = processes
        .log_path(&process)
        .ok_or_else(|| not_running(server))?;
    let offset = tokio::fs::metadata(&log)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);
    processes
        .write_line(&process, command)
        .await
        .map_err(|e| stdin_error(server, e))?;
    let reply = capture(&log, offset, QUIET, DEADLINE, MAX_REPLY_BYTES).await?;
    Ok(strip_prefixes(&reply))
}

pub(super) async fn capture(
    log: &Path,
    offset: u64,
    quiet: Duration,
    deadline: Duration,
    max_bytes: usize,
) -> Result<String> {
    let start = Instant::now();
    let mut last_growth = start;
    let mut collected: Vec<u8> = Vec::new();
    loop {
        tokio::time::sleep(POLL).await;
        let before = collected.len();
        if let Ok(mut file) = tokio::fs::File::open(log).await {
            file.seek(std::io::SeekFrom::Start(offset + before as u64))
                .await
                .context("cannot seek the server log")?;
            let room = max_bytes - before;
            file.take(room as u64)
                .read_to_end(&mut collected)
                .await
                .context("cannot read the server log")?;
        }
        if collected.len() > before {
            last_growth = Instant::now();
        }
        if collected.len() >= max_bytes
            || last_growth.elapsed() >= quiet
            || start.elapsed() >= deadline
        {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&collected).trim_end().to_string())
}

/// BDS prefixes each line with `[2026-01-02 03:04:05:678 INFO] `.
fn strip_prefixes(reply: &str) -> String {
    reply
        .lines()
        .map(
            |line| match line.strip_prefix('[').and_then(|r| r.split_once("] ")) {
                Some((stamp, rest)) if stamp.starts_with(|c: char| c.is_ascii_digit()) => rest,
                _ => line,
            },
        )
        .collect::<Vec<_>>()
        .join("\n")
}

fn not_running(server: &str) -> anyhow::Error {
    ErrorInfo::NotRunning {
        entry: EntryKind::Server,
        name: server.to_string(),
    }
    .into()
}

fn stdin_error(server: &str, error: StdinError) -> anyhow::Error {
    match error {
        StdinError::InvalidLine(_) => ErrorInfo::InvalidValue {
            field: Field::Command,
            reason: Reason::ConsoleLine,
        }
        .into(),
        StdinError::NotRunning(_) => not_running(server),
        StdinError::NoEndpoint(_) => {
            anyhow::anyhow!("this server has no console yet (restart it to enable one)")
        }
        StdinError::Io(e) => anyhow::Error::new(e).context("cannot reach the server's console"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[tokio::test]
    async fn returns_only_lines_after_the_offset() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("output.log");
        std::fs::write(&log, "old\n").unwrap();
        let writer_log = log.clone();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(ms(60)).await;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(writer_log)
                .unwrap();
            file.write_all(b"one\ntwo\n").unwrap();
        });
        let reply = capture(&log, 4, ms(150), Duration::from_secs(2), 1024)
            .await
            .unwrap();
        writer.await.unwrap();
        assert_eq!(reply, "one\ntwo");
    }

    #[tokio::test]
    async fn stops_at_the_quiet_window() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("output.log");
        std::fs::write(&log, "").unwrap();
        let started = Instant::now();
        let reply = capture(&log, 0, ms(100), Duration::from_secs(2), 1024)
            .await
            .unwrap();
        assert_eq!(reply, "");
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn respects_the_byte_cap() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("output.log");
        std::fs::write(&log, "x".repeat(500)).unwrap();
        let reply = capture(&log, 0, ms(100), Duration::from_secs(2), 100)
            .await
            .unwrap();
        assert_eq!(reply.len(), 100);
    }

    #[test]
    fn strips_the_bds_prefix() {
        let reply = "[2026-01-02 03:04:05:678 INFO] There are 0/10 players online:\nplain";
        assert_eq!(
            strip_prefixes(reply),
            "There are 0/10 players online:\nplain"
        );
    }
}
