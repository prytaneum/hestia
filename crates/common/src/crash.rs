use std::fs;
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const LOG_TAIL_BYTES: u64 = 64 * 1024;

const KEEP: usize = 20;

struct Target {
    dir: PathBuf,
    log: Option<PathBuf>,
    binary: String,
}

static TARGET: OnceLock<Target> = OnceLock::new();

pub(crate) fn install(dir: Option<PathBuf>, log: Option<PathBuf>, binary: &str) {
    if let Some(dir) = dir {
        let _ = TARGET.set(Target {
            dir,
            log,
            binary: binary.to_string(),
        });
    }

    std::panic::set_hook(Box::new(|info| {
        let message = payload(info);
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".to_string());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();

        tracing::error!(
            panic = %message,
            location = %location,
            "process panicked"
        );

        match write_report("panic", &message, &location, &backtrace) {
            Some(path) => eprintln!(
                "hestia: panicked at {location}: {message}\n  crash report: {}",
                path.display()
            ),
            None => eprintln!("hestia: panicked at {location}: {message}"),
        }
    }));
}

pub fn record(kind: &str, message: &str, location: &str, detail: &str) -> Option<PathBuf> {
    tracing::error!(kind, %message, location, "crash reported");
    write_report(kind, message, location, detail)
}

fn write_report(kind: &str, message: &str, location: &str, detail: &str) -> Option<PathBuf> {
    let target = TARGET.get()?;
    fs::create_dir_all(&target.dir).ok()?;

    let path = target
        .dir
        .join(format!("crash-{}.log", crate::time::now_file_stamp()));
    let report = format!(
        "Hestia crash report\n\
         ===================\n\
         time:     {time}\n\
         binary:   {binary} {version}\n\
         platform: {os} {arch}\n\
         pid:      {pid}\n\
         kind:     {kind}\n\
         \n\
         message:\n{message}\n\
         \n\
         location:\n{location}\n\
         \n\
         detail:\n{detail}\n\
         \n\
         recent log:\n{log}\n",
        time = crate::time::now_stamp(),
        binary = target.binary,
        version = crate::app::VERSION_LABEL,
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        pid = std::process::id(),
        log = log_tail(target.log.as_deref()).unwrap_or_else(|| "(unavailable)".to_string()),
    );

    fs::write(&path, report).ok()?;
    prune(&target.dir);
    Some(path)
}

fn log_tail(log: Option<&Path>) -> Option<String> {
    let mut file = fs::File::open(log?).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL_BYTES)))
        .ok()?;
    let mut tail = String::new();
    io::Read::read_to_string(&mut file, &mut tail).ok()?;
    if len > LOG_TAIL_BYTES {
        if let Some(newline) = tail.find('\n') {
            tail.drain(..=newline);
        }
    }
    Some(tail)
}

fn prune(dir: &Path) {
    let mut reports: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "log"))
        .collect();
    if reports.len() <= KEEP {
        return;
    }
    reports.sort_unstable();
    for old in &reports[..reports.len() - KEEP] {
        let _ = fs::remove_file(old);
    }
}

fn payload(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-string payload".to_string()
    }
}
