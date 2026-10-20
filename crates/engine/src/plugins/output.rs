use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use tracing::{Level, Span};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamError};

const LINES_PER_SECOND: u32 = 100;
const MAX_LINE_BYTES: usize = 8 * 1024;

struct Window {
    started: Instant,
    lines: u32,
    dropped: u32,
}

/// Where everything a plugin says ends up: its `log` import and its captured
/// stdout and stderr, under one `plugin` span and one shared rate limit.
pub(crate) struct PluginLog {
    span: Span,
    window: Mutex<Window>,
}

impl PluginLog {
    pub(crate) fn new(plugin: &str) -> Arc<Self> {
        Arc::new(PluginLog {
            span: tracing::info_span!(parent: None, "plugin", plugin = %plugin),
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
                self.emit(
                    Level::WARN,
                    &format!("{dropped} log lines dropped (limit {LINES_PER_SECOND}/s)"),
                );
            }
            if allowed {
                self.emit(level, truncate(text));
            }
        }
    }

    fn admit(&self) -> (bool, Option<u32>) {
        let mut window = self.window.lock().unwrap();
        let mut dropped_before = None;
        if window.started.elapsed() >= Duration::from_secs(1) {
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

    fn emit(&self, level: Level, text: &str) {
        self.span.in_scope(|| match level {
            Level::TRACE => tracing::trace!("{text}"),
            Level::DEBUG => tracing::debug!("{text}"),
            Level::INFO => tracing::info!("{text}"),
            Level::WARN => tracing::warn!("{text}"),
            Level::ERROR => tracing::error!("{text}"),
        });
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
    fn async_stream(&self) -> Box<dyn tokio::io::AsyncWrite + Send + Sync> {
        Box::new(tokio::io::sink())
    }

    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(LineStream {
            log: self.log.clone(),
            level: self.level,
            partial: Vec::new(),
        })
    }
}

struct LineStream {
    log: Arc<PluginLog>,
    level: Level,
    partial: Vec<u8>,
}

impl LineStream {
    fn flush_partial(&mut self) {
        if !self.partial.is_empty() {
            let text = String::from_utf8_lossy(&self.partial).into_owned();
            self.partial.clear();
            self.log.line(self.level, &text);
        }
    }
}

#[async_trait]
impl Pollable for LineStream {
    async fn ready(&mut self) {}
}

impl OutputStream for LineStream {
    fn write(&mut self, bytes: Bytes) -> Result<(), StreamError> {
        for byte in bytes {
            if byte == b'\n' {
                self.flush_partial();
            } else {
                self.partial.push(byte);
                if self.partial.len() >= MAX_LINE_BYTES {
                    self.flush_partial();
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        Ok(())
    }

    fn check_write(&mut self) -> Result<usize, StreamError> {
        Ok(MAX_LINE_BYTES)
    }
}

impl Drop for LineStream {
    fn drop(&mut self) {
        self.flush_partial();
    }
}
