use std::fmt;
use std::fs::Metadata;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, Timelike};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

const FORMAT: &str = "%Y-%m-%d %H:%M:%S%.3f";

const FILE_FORMAT: &str = "%Y%m%d-%H%M%S%3f";

const UTC_STAMP: &str = "%Y%m%d-%H%M%S";

#[derive(Clone, Copy, Default)]
pub struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut Writer<'_>) -> fmt::Result {
        write!(w, "{}", Local::now().format(FORMAT))
    }
}

pub fn now_stamp() -> String {
    Local::now().format(FORMAT).to_string()
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// `unix` as `YYYYMMDD-HHMMSS` in UTC: what names a backup archive and an
/// exported instance, anything the disk registry sorts by time.
pub fn utc_stamp(unix: i64) -> String {
    DateTime::from_timestamp(unix, 0)
        .unwrap_or_default()
        .format(UTC_STAMP)
        .to_string()
}

/// The unix time of an HTTP date header (`Wed, 21 Oct 2015 07:28:00 GMT`).
pub fn http_date_unix(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|date| date.timestamp())
}

pub fn modified_unix(meta: &Metadata) -> Option<i64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(since.as_secs() as i64)
}

/// The unix time today began at in the machine's zone — what a "daily at
/// 04:30" schedule is measured from.
pub fn local_midnight_unix() -> i64 {
    let now = Local::now();
    let since_midnight = i64::from(now.time().num_seconds_from_midnight());
    now.timestamp() - since_midnight
}

pub fn now_file_stamp() -> String {
    Local::now().format(FILE_FORMAT).to_string()
}
