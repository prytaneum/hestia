//! File tailing: live `process.output` events come from polling the process's
//! log file (its own latest.log or the supervisor's capture file), and
//! `process.logs` reads the file's tail on demand.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use proto::process::{LogStream, ProcessLogLine, ProcessOutputEvent};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use super::{emit, ProcessEvents};

const POLL: Duration = Duration::from_millis(250);
const CHUNK: usize = 64 * 1024;

/// Cap on lines per `process.output` event. A pass coalesces every line it read
/// into one event (below), but a large initial backlog — the first tail of a
/// long file — would otherwise become one unbounded event: a single giant
/// deserialize + render spike on a subscriber. Splitting the outlier into a few
/// bounded events keeps the steady-state one-event-per-pass while never handing
/// a subscriber more than this many lines at once.
const MAX_LINES_PER_EVENT: usize = 500;

pub struct Tailer {
    done: Arc<AtomicBool>,
    wake: Arc<Notify>,
    handle: JoinHandle<()>,
}

impl Tailer {
    /// Must complete before the exit event is published, or subscribers miss
    /// trailing lines.
    pub async fn finish(self) {
        self.done.store(true, Ordering::SeqCst);
        self.wake.notify_one();
        let _ = self.handle.await;
    }
}

pub fn spawn(
    path: PathBuf,
    from: u64,
    id: String,
    events: Option<Arc<dyn ProcessEvents>>,
) -> Tailer {
    let done = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(Notify::new());
    let handle = tokio::spawn(run(path, from, id, events, done.clone(), wake.clone()));
    Tailer { done, wake, handle }
}

async fn run(
    path: PathBuf,
    mut offset: u64,
    id: String,
    events: Option<Arc<dyn ProcessEvents>>,
    done: Arc<AtomicBool>,
    wake: Arc<Notify>,
) {
    let mut pending = Vec::new();
    loop {
        let finishing = done.load(Ordering::SeqCst);
        // Coalesce every line read this pass into one event: a chatty startup
        // log would otherwise emit thousands of per-line events and saturate a
        // subscriber's main thread (the desktop re-renders per event). The
        // batch is capped at MAX_LINES_PER_EVENT so an initial backlog degrades
        // to a few bounded events instead of one huge one.
        let mut batch = Vec::new();
        drain(&path, &mut offset, &mut pending, |offset, line| {
            batch.push(ProcessLogLine {
                stream: LogStream::Stdout,
                line,
                offset,
            });
            if batch.len() >= MAX_LINES_PER_EVENT {
                publish(events.as_ref(), &id, std::mem::take(&mut batch));
            }
        });
        if !batch.is_empty() {
            publish(events.as_ref(), &id, batch);
        }
        if finishing {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(POLL) => {}
            _ = wake.notified() => {}
        }
    }
}

fn publish(events: Option<&Arc<dyn ProcessEvents>>, id: &str, lines: Vec<ProcessLogLine>) {
    emit(
        events,
        &ProcessOutputEvent {
            id: id.to_string(),
            lines,
        },
    );
}

fn drain(path: &Path, offset: &mut u64, pending: &mut Vec<u8>, mut emit: impl FnMut(u64, String)) {
    let Ok(mut file) = File::open(path) else {
        return;
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len < *offset {
        // Rotated or truncated (log4j archives latest.log): start over.
        *offset = 0;
        pending.clear();
    }
    if len == *offset || file.seek(SeekFrom::Start(*offset)).is_err() {
        return;
    }
    let mut remaining = (len - *offset) as usize;
    let mut chunk = vec![0u8; CHUNK.min(remaining)];
    while remaining > 0 {
        let want = CHUNK.min(remaining);
        let Ok(read) = file.read(&mut chunk[..want]) else {
            break;
        };
        if read == 0 {
            break;
        }
        *offset += read as u64;
        remaining -= read;
        pending.extend_from_slice(&chunk[..read]);
        while let Some(pos) = pending.iter().position(|&b| b == b'\n') {
            let start = *offset - pending.len() as u64;
            let line: Vec<u8> = pending.drain(..=pos).collect();
            emit(start, text(&line[..pos]));
        }
    }
}

fn text(line: &[u8]) -> String {
    String::from_utf8_lossy(line.strip_suffix(b"\r").unwrap_or(line)).into_owned()
}

/// The last `limit` lines of `path` with their start offsets; empty when the file is missing.
pub fn read_last_lines(path: &Path, limit: usize) -> Vec<(u64, String)> {
    if limit == 0 {
        return Vec::new();
    }
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return Vec::new();
    };

    let mut tail: Vec<u8> = Vec::new();
    let mut pos = len;
    while pos > 0 {
        let start = pos.saturating_sub(CHUNK as u64);
        let mut chunk = vec![0u8; (pos - start) as usize];
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut chunk).is_err() {
            break;
        }
        chunk.extend_from_slice(&tail);
        tail = chunk;
        pos = start;
        if tail.iter().filter(|&&b| b == b'\n').count() > limit {
            break;
        }
    }

    let mut lines = Vec::new();
    let mut start = 0;
    for line in tail.split_inclusive(|&b| b == b'\n') {
        let ended = line.strip_suffix(b"\n").unwrap_or(line);
        lines.push((pos + start as u64, text(ended)));
        start += line.len();
    }
    if pos > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    let skip = lines.len().saturating_sub(limit);
    lines.drain(..skip);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &str) -> tempfile::NamedTempFile {
        let path = tempfile::Builder::new()
            .prefix(&format!("hestia-tail-{name}-"))
            .tempfile()
            .expect("temp file");
        let mut f = File::create(path.path()).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn reads_the_last_lines() {
        let file = temp_file("last", "one\ntwo\nthree\n");
        let path = file.path();
        let line = |offset, text: &str| (offset, text.to_string());
        assert_eq!(read_last_lines(path, 2), [line(4, "two"), line(8, "three")]);
        assert_eq!(read_last_lines(path, 10).len(), 3);
        assert!(read_last_lines(path, 0).is_empty());
    }

    #[test]
    fn a_line_has_the_same_offset_tailed_or_read_back() {
        let file = temp_file("same", "one\r\ntwø\n\nfour\n");
        let mut tailed = Vec::new();
        drain(file.path(), &mut 0, &mut Vec::new(), |at, l| {
            tailed.push((at, l))
        });
        assert_eq!(read_last_lines(file.path(), 10), tailed);
        assert_eq!(tailed[3], (11, "four".to_string()));
    }

    #[test]
    fn missing_file_reads_empty() {
        assert!(read_last_lines(Path::new("/nonexistent/hestia.log"), 5).is_empty());
    }

    #[test]
    fn drain_emits_new_complete_lines_and_handles_truncation() {
        let file = temp_file("drain", "first\nsecond\npart");
        let path = file.path();
        let mut offset = 0;
        let mut pending = Vec::new();
        let mut lines = Vec::new();
        drain(path, &mut offset, &mut pending, |at, l| lines.push((at, l)));
        let line = |offset, text: &str| (offset, text.to_string());
        assert_eq!(lines, [line(0, "first"), line(6, "second")]);

        std::fs::write(path, "fresh\n").unwrap();
        drain(path, &mut offset, &mut pending, |at, l| lines.push((at, l)));
        assert_eq!(lines[2], line(0, "fresh"));
    }
}
