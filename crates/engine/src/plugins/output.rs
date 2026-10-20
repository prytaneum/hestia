use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::AsyncWrite;
use tracing::Level;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};

const LINES_PER_SECOND: u32 = 100;
const MAX_LINE_BYTES: usize = 8 * 1024;
const WINDOW: Duration = Duration::from_secs(1);

struct Window {
    started: Instant,
    lines: u32,
    dropped: u32,
}

/// Where everything a plugin says ends up: its `log` import and its captured
/// stdout and stderr, under one plugin id and one shared rate limit.
pub(crate) struct PluginLog {
    id: String,
    window: Mutex<Window>,
}

impl PluginLog {
    pub(crate) fn new(plugin: &str) -> Arc<Self> {
        Arc::new(PluginLog {
            id: plugin.to_owned(),
            window: Mutex::new(Window {
                started: Instant::now(),
                lines: 0,
                dropped: 0,
            }),
        })
    }

    /// Each `\n`-separated line is rate-limited and truncated on its own, so a
    /// guest cannot forge extra log lines or dodge the limit with one message.
    pub(crate) fn line(&self, level: Level, text: &str) {
        for text in text.lines() {
            let (allowed, dropped_before) = self.admit();
            if let Some(dropped) = dropped_before {
                self.emit_dropped(dropped);
            }
            if allowed {
                self.emit(level, truncate(text));
            }
        }
    }

    /// Reports lines dropped in a window that has since closed, so a plugin
    /// that floods the log and then goes quiet is still noticed.
    pub(crate) fn report_dropped(&self) {
        let dropped = {
            let mut window = self.window.lock().unwrap();
            if window.started.elapsed() < WINDOW || window.dropped == 0 {
                return;
            }
            std::mem::take(&mut window.dropped)
        };
        self.emit_dropped(dropped);
    }

    fn admit(&self) -> (bool, Option<u32>) {
        let mut window = self.window.lock().unwrap();
        let mut dropped_before = None;
        if window.started.elapsed() >= WINDOW {
            if window.dropped > 0 {
                dropped_before = Some(window.dropped);
            }
            *window = Window {
                started: Instant::now(),
                lines: 0,
                dropped: 0,
            };
        }
        if window.lines >= LINES_PER_SECOND {
            window.dropped += 1;
            return (false, dropped_before);
        }
        window.lines += 1;
        (true, dropped_before)
    }

    fn emit_dropped(&self, dropped: u32) {
        self.emit(
            Level::WARN,
            &format!("{dropped} log lines dropped (limit {LINES_PER_SECOND}/s)"),
        );
    }

    fn emit(&self, level: Level, text: &str) {
        let plugin = &self.id;
        match level {
            Level::TRACE => tracing::trace!(%plugin, "{text}"),
            Level::DEBUG => tracing::debug!(%plugin, "{text}"),
            Level::INFO => tracing::info!(%plugin, "{text}"),
            Level::WARN => tracing::warn!(%plugin, "{text}"),
            Level::ERROR => tracing::error!(%plugin, "{text}"),
        }
    }
}

fn truncate(text: &str) -> &str {
    if text.len() <= MAX_LINE_BYTES {
        return text;
    }
    let mut end = MAX_LINE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A guest's stdout or stderr, split into lines for [`PluginLog`].
pub(crate) struct Captured {
    log: Arc<PluginLog>,
    level: Level,
}

impl Captured {
    pub(crate) fn stdout(log: Arc<PluginLog>) -> Self {
        Captured {
            log,
            level: Level::INFO,
        }
    }

    pub(crate) fn stderr(log: Arc<PluginLog>) -> Self {
        Captured {
            log,
            level: Level::WARN,
        }
    }
}

impl IsTerminal for Captured {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for Captured {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(LineSplitter {
            log: self.log.clone(),
            level: self.level,
            partial: Vec::new(),
        })
    }
}

struct LineSplitter {
    log: Arc<PluginLog>,
    level: Level,
    partial: Vec<u8>,
}

impl LineSplitter {
    fn flush_partial(&mut self) {
        if !self.partial.is_empty() {
            let text = String::from_utf8_lossy(&self.partial).into_owned();
            self.partial.clear();
            self.log.line(self.level, &text);
        }
    }
}

impl AsyncWrite for LineSplitter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        for &byte in buf {
            if byte == b'\n' {
                self.flush_partial();
            } else {
                self.partial.push(byte);
                if self.partial.len() >= MAX_LINE_BYTES {
                    self.flush_partial();
                }
            }
        }
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for LineSplitter {
    fn drop(&mut self) {
        self.flush_partial();
    }
}
