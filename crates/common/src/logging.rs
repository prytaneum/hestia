use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use flexi_logger::writers::{FileLogWriter, FileLogWriterHandle};
use flexi_logger::{Age, Cleanup, Criterion, FileSpec, Naming, WriteMode};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::time::LocalTime;

const ENV_FILTER: &str = "HESTIA_LOG";

const CRATES: [&str; 7] = [
    "hestia", "desktop", "common", "client", "engine", "ipc", "proto",
];

const FOREIGN_LEVEL: &str = "warn";

/// Foreign targets held below [`FOREIGN_LEVEL`]. `discord_rich_presence` warns
/// once per failed connect, and no Discord client running is the ordinary case
/// the presence loop polls through — not a fault worth a line every tick.
const QUIET_FOREIGN: [&str; 1] = ["discord_rich_presence=error"];

const LATEST_MAX_BYTES: u64 = 20 * 1024 * 1024;
const LATEST_KEEP_PLAIN: usize = 2;
const LATEST_KEEP_ARCHIVED: usize = 30;

const DEBUG_MAX_BYTES: u64 = 200 * 1024 * 1024;
const DEBUG_KEEP_ARCHIVED: usize = 5;

static CONSOLE_MUTED: AtomicBool = AtomicBool::new(false);

/// Gate the console sink while a fullscreen surface owns the terminal: a log
/// line printed over the alternate screen corrupts it, and the emitter can be
/// any dependency (a renderer's warning), not just our own code. Muted output
/// is dropped — the file sinks, when configured, keep recording.
pub fn set_console_muted(muted: bool) {
    CONSOLE_MUTED.store(muted, Ordering::Relaxed);
}

#[derive(Clone, Copy)]
struct GatedStderr;

struct GatedStderrWriter(Option<io::Stderr>);

impl io::Write for GatedStderrWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.0 {
            Some(stderr) => stderr.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.0 {
            Some(stderr) => stderr.flush(),
            None => Ok(()),
        }
    }
}

impl<'a> MakeWriter<'a> for GatedStderr {
    type Writer = GatedStderrWriter;

    fn make_writer(&'a self) -> Self::Writer {
        GatedStderrWriter((!CONSOLE_MUTED.load(Ordering::Relaxed)).then(io::stderr))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Default for LogLevel {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            LogLevel::Debug
        } else {
            LogLevel::Info
        }
    }
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
            LogLevel::Off => "off",
        }
    }
}

pub struct FileLog {
    logs: PathBuf,
    binary: String,
    level: LogLevel,
    firehose: bool,
}

impl FileLog {
    pub fn for_binary(binary: impl Into<String>, home: Option<&Path>, level: LogLevel) -> Self {
        FileLog {
            logs: crate::paths::log_dir(home),
            binary: binary.into(),
            level,
            firehose: false,
        }
    }

    pub fn with_firehose(mut self) -> Self {
        self.firehose = true;
        self
    }

    fn dir(&self) -> PathBuf {
        self.logs.join(&self.binary)
    }

    fn crash_dir(&self) -> PathBuf {
        self.logs.join("crashes")
    }

    /// The live log path, for callers that surface it (e.g. `daemon.status`).
    pub fn active_path(&self) -> PathBuf {
        self.dir().join("latest.log")
    }
}

#[must_use = "dropping the guard stops file logging"]
pub struct LogGuard(#[allow(dead_code)] Vec<FileLogWriterHandle>);

pub fn init_logging(console_level: LogLevel, file: Option<FileLog>) -> LogGuard {
    let mut layers = Vec::new();
    let mut handles = Vec::new();
    let firehose = file.as_ref().is_some_and(|f| f.firehose);

    if console_level != LogLevel::Off {
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(GatedStderr)
            .with_timer(LocalTime)
            .with_ansi(io::stderr().is_terminal())
            .with_target(true)
            .with_filter(app_filter(console_level));
        layers.push(layer.boxed());
    }

    if let Some(file) = &file {
        if file.level != LogLevel::Off {
            match rotating_writer(
                &file.dir(),
                Criterion::AgeOrSize(Age::Day, LATEST_MAX_BYTES),
                Cleanup::KeepLogAndCompressedFiles(LATEST_KEEP_PLAIN, LATEST_KEEP_ARCHIVED),
            ) {
                Ok((writer, handle)) => {
                    handles.push(handle);
                    layers.push(
                        file_layer(writer)
                            .with_filter(app_filter(file.level))
                            .boxed(),
                    );
                }
                Err(e) => eprintln!("hestia: file logging disabled: {e}"),
            }
        }

        if file.firehose {
            match rotating_writer(
                &file.dir().join("debug"),
                Criterion::Size(DEBUG_MAX_BYTES),
                Cleanup::KeepCompressedFiles(DEBUG_KEEP_ARCHIVED),
            ) {
                Ok((writer, handle)) => {
                    handles.push(handle);
                    layers.push(
                        file_layer(writer)
                            .with_file(true)
                            .with_line_number(true)
                            .boxed(),
                    );
                }
                Err(e) => eprintln!("hestia: debug logging disabled: {e}"),
            }
        }
    }

    let global = if firehose {
        EnvFilter::new("trace")
    } else {
        app_filter(console_level.max(file.as_ref().map_or(LogLevel::Off, |f| f.level)))
    };
    tracing_subscriber::registry()
        .with(global)
        .with(layers)
        .init();

    let (crash_dir, log_path, binary) = match &file {
        Some(file) => (
            Some(file.crash_dir()),
            Some(file.active_path()),
            file.binary.clone(),
        ),
        None => (None, None, crate::app::NAME.to_lowercase()),
    };
    crate::crash::install(crash_dir, log_path, &binary);

    tracing::info!(
        version = crate::app::VERSION_LABEL,
        pid = std::process::id(),
        "process starting"
    );

    LogGuard(handles)
}

fn file_layer<S, W>(
    writer: W,
) -> tracing_subscriber::fmt::Layer<
    S,
    tracing_subscriber::fmt::format::DefaultFields,
    tracing_subscriber::fmt::format::Format<tracing_subscriber::fmt::format::Full, LocalTime>,
    W,
>
where
    W: for<'a> MakeWriter<'a> + 'static,
{
    tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_timer(LocalTime)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
}

fn rotating_writer(
    dir: &Path,
    criterion: Criterion,
    cleanup: Cleanup,
) -> Result<
    (impl for<'a> MakeWriter<'a> + 'static, FileLogWriterHandle),
    flexi_logger::FlexiLoggerError,
> {
    let spec = FileSpec::default()
        .directory(dir)
        .suppress_basename()
        .suppress_timestamp()
        .suffix("log");

    let (writer, handle) = FileLogWriter::builder(spec)
        .rotate(
            criterion,
            Naming::TimestampsCustomFormat {
                current_infix: Some("latest"),
                format: "%Y-%m-%d",
            },
            cleanup,
        )
        .append()
        .use_utc()
        .write_mode(WriteMode::Direct)
        .try_build_with_handle()?;

    Ok((move || writer.clone(), handle))
}

fn app_filter(level: LogLevel) -> EnvFilter {
    if let Some(spec) = env_override() {
        return EnvFilter::new(spec);
    }
    let mut filter = EnvFilter::new(FOREIGN_LEVEL);
    for target in CRATES {
        if let Ok(directive) = format!("{target}={}", level.as_str()).parse() {
            filter = filter.add_directive(directive);
        }
    }
    for quiet in QUIET_FOREIGN {
        if let Ok(directive) = quiet.parse() {
            filter = filter.add_directive(directive);
        }
    }
    filter
}

fn env_override() -> Option<String> {
    std::env::var(ENV_FILTER).ok().filter(|s| !s.is_empty())
}
