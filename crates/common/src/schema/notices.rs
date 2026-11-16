//! Where a quarantined document goes so that somebody other than the log finds
//! out about it.
//!
//! A quarantine is not the outcome of the operation that happened to trigger it
//! — a `server.list` does not *cause* one — so it cannot ride out on that
//! result the way an ordinary degraded outcome does. What it does instead is
//! disappear: the entry whose record was set aside stops being listed,
//! silently, which is the exact failure a warning riding on the result exists
//! to prevent.
//!
//! So quarantines accumulate here, in one append-only sink per process, and are
//! read back through the surfaces where the user is already looking: the daemon
//! status, and the result of an import or a restore that produced one. The sink
//! is process-global rather than an engine member because a document is loaded
//! from free functions several layers below anything holding the aggregate, and
//! threading a handle through every store constructor to carry diagnostics would
//! cost more than it explains. `mark`/`since` scope a read to one operation
//! without the sink having to know operations exist.

use std::sync::{Mutex, OnceLock};

/// A document set aside because this build could not read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quarantine {
    pub document: String,
    /// Where it was set aside to, or where it still is if the rename failed.
    pub path: String,
    pub detail: String,
}

/// How many are kept. A data home full of unreadable files must not grow this
/// without bound; the oldest are dropped, and the log still has all of them.
const LIMIT: usize = 64;

/// A position in the sink. Opaque and monotonic, so it stays meaningful after
/// the oldest entries have been dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark(u64);

struct Sink {
    next: u64,
    entries: Vec<(u64, Quarantine)>,
}

fn sink() -> &'static Mutex<Sink> {
    static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();
    SINK.get_or_init(|| {
        Mutex::new(Sink {
            next: 0,
            entries: Vec::new(),
        })
    })
}

pub fn record(quarantine: Quarantine) {
    let mut sink = sink().lock().unwrap();
    let seq = sink.next;
    sink.next += 1;
    sink.entries.push((seq, quarantine));
    if sink.entries.len() > LIMIT {
        sink.entries.remove(0);
    }
}

/// Where the sink is now, for pairing with [`since`] around one operation.
pub fn mark() -> Mark {
    Mark(sink().lock().unwrap().next)
}

/// Everything recorded since `mark` — what one operation produced.
pub fn since(mark: Mark) -> Vec<Quarantine> {
    sink()
        .lock()
        .unwrap()
        .entries
        .iter()
        .filter(|(seq, _)| *seq >= mark.0)
        .map(|(_, quarantine)| quarantine.clone())
        .collect()
}

/// Everything this daemon has quarantined since it started.
pub fn all() -> Vec<Quarantine> {
    sink()
        .lock()
        .unwrap()
        .entries
        .iter()
        .map(|(_, quarantine)| quarantine.clone())
        .collect()
}
