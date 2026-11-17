//! The daemon's log entries of one plugin, as they are written. A plugin's log
//! goes to the daemon's debug log, not over the socket: that file holds every
//! level, where `latest.log` keeps only what the file level lets through.

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use crate::ui::{self, View};

const POLL: Duration = Duration::from_millis(250);

/// Groups log lines into entries: a line that opens with a timestamp starts
/// one, and the lines that follow it belong to it, as a trap's backtrace does.
#[derive(Default)]
struct Entries {
    open: Option<String>,
}

fn starts_entry(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() > 4 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4] == b'-'
}

impl Entries {
    /// The entry `line` completes, if it opens the next one.
    fn push(&mut self, line: &str) -> Option<String> {
        match (starts_entry(line), self.open.as_mut()) {
            (false, Some(open)) => {
                open.push('\n');
                open.push_str(line);
                None
            }
            _ => self.open.replace(line.to_owned()),
        }
    }

    fn flush(&mut self) -> Option<String> {
        self.open.take()
    }
}

/// Whether `entry` is the plugin's own log line: its target is `plugin`, and
/// its fields, which come after the message, carry the id exactly.
fn is_of(entry: &str, id: &str) -> bool {
    let first = entry.lines().next().unwrap_or_default();
    first.contains(" plugin: ") && entry.contains(&format!("plugin=\"{id}\""))
}

fn show(entry: Option<String>, id: &str) {
    if let Some(entry) = entry.filter(|entry| is_of(entry, id)) {
        let _ = ui::show(View::line(entry));
    }
}

pub async fn follow(id: String) {
    let path = common::paths::log_dir(None)
        .join("hestiad")
        .join("debug")
        .join("latest.log");
    let mut offset = std::fs::metadata(&path).map_or(0, |meta| meta.len());
    let mut entries = Entries::default();
    loop {
        tokio::time::sleep(POLL).await;
        let Ok(mut file) = std::fs::File::open(&path) else {
            continue;
        };
        let length = file.metadata().map_or(0, |meta| meta.len());
        if length < offset {
            offset = 0;
        }
        let mut text = String::new();
        if file.seek(SeekFrom::Start(offset)).is_err() || file.read_to_string(&mut text).is_err() {
            continue;
        }
        let complete = text.rfind('\n').map_or(0, |end| end + 1);
        if complete == 0 {
            show(entries.flush(), &id);
            continue;
        }
        offset += complete as u64;
        for line in text[..complete].lines() {
            show(entries.push(line), &id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_trap_and_a_log_line_are_found_among_entries_by_the_real_format() {
        let sink = Sink::default();
        let writer = sink.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(true)
            .with_max_level(tracing::Level::TRACE)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "kernel::registry", plugin = "tour", "plugin removed");
            tracing::debug!(target: "plugin", plugin = "tour", "{:?}", "debugging");
            tracing::warn!(target: "plugin", plugin = "tour-2", "{:?}", "another plugin");
            tracing::warn!(target: "plugin", plugin = "tour", "trapped\n{:?}", "wasm backtrace");
        });
        let written = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let mut entries = Entries::default();
        let mut found: Vec<String> = written
            .lines()
            .filter_map(|line| entries.push(line))
            .collect();
        found.extend(entries.flush());
        found.retain(|entry| is_of(entry, "tour"));

        assert_eq!(found.len(), 2, "{found:#?}");
        assert!(found[0].contains("debugging"), "{found:#?}");
        assert!(
            found[1].contains("trapped") && found[1].contains("wasm backtrace"),
            "the target is on the first line and the id on the last: {found:#?}"
        );
    }
}
