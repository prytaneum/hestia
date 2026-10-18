//! Terminal progress + spinner rendering for long-running commands. A
//! background thread rewrites one stderr line in place on a fixed tick — no
//! viewport, so the shell keeps its scrollback and a fullscreen session can
//! follow cleanly. When stderr is redirected (a pipe, CI) rendering is
//! skipped and callers that want a paper trail degrade to terse per-phase
//! lines.

use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use client::proto::minecraft::{ProvisionPhase, ProvisionProgress};
use ratatui::crossterm::cursor::{Hide, MoveToColumn, Show};
use ratatui::crossterm::style::{Print, Stylize};
use ratatui::crossterm::terminal::{Clear, ClearType};
use ratatui::crossterm::{execute, queue};

use super::render::human_bytes;

const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TICK: Duration = Duration::from_millis(80);
const GAUGE_WIDTH: usize = 30;

/// What the animator paints on the next tick.
enum View {
    Spinner(String),
    Gauge {
        label: &'static str,
        ratio: f64,
        detail: String,
    },
}

/// A background renderer that rewrites the current `View` onto one stderr
/// line every tick until stopped. Created only when stderr is a terminal.
struct Animator {
    view: Arc<Mutex<View>>,
    stop: Arc<AtomicBool>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Animator {
    fn start(initial: View) -> Option<Self> {
        if !io::stderr().is_terminal() {
            return None;
        }
        let view = Arc::new(Mutex::new(initial));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_view, worker_stop) = (view.clone(), stop.clone());
        let handle = thread::spawn(move || run(worker_view, worker_stop));
        Some(Self {
            view,
            stop,
            handle: Mutex::new(Some(handle)),
        })
    }

    fn set(&self, view: View) {
        *self.view.lock().unwrap() = view;
    }

    fn stop(&self) {
        if let Some(handle) = self.handle.lock().unwrap().take() {
            self.stop.store(true, Ordering::Relaxed);
            let _ = handle.join();
        }
    }
}

impl Drop for Animator {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(view: Arc<Mutex<View>>, stop: Arc<AtomicBool>) {
    let mut err = io::stderr();
    let _ = execute!(err, Hide);
    let mut step = 0usize;
    while !stop.load(Ordering::Relaxed) {
        let rendered = {
            let view = view.lock().unwrap();
            line(&view)
        };
        let spin = FRAMES[step % FRAMES.len()].cyan();
        // One buffered frame, flushed once: overwrite from column 0 and erase
        // only the tail. Clear-then-redraw across separate writes lets the
        // terminal paint the blank in between — visible as flicker.
        let mut frame = Vec::new();
        let _ = queue!(
            frame,
            MoveToColumn(0),
            Print(spin),
            Print(" "),
            Print(rendered),
            Clear(ClearType::UntilNewLine)
        );
        let _ = err.write_all(&frame);
        let _ = err.flush();
        step = step.wrapping_add(1);
        thread::sleep(TICK);
    }
    let mut frame = Vec::new();
    let _ = queue!(frame, MoveToColumn(0), Clear(ClearType::CurrentLine), Show);
    let _ = err.write_all(&frame);
    let _ = err.flush();
}

/// The line after the spinner frame: plain text (no escape codes — the line
/// is truncated by chars to the terminal width, and a sliced escape sequence
/// garbles the whole row).
fn line(view: &View) -> String {
    let text = match view {
        View::Spinner(label) => label.clone(),
        View::Gauge {
            label,
            ratio,
            detail,
        } => {
            let filled = ((ratio * GAUGE_WIDTH as f64).round() as usize).min(GAUGE_WIDTH);
            format!(
                "{label} {}{} {:>3.0}% · {detail}",
                "█".repeat(filled),
                "░".repeat(GAUGE_WIDTH - filled),
                ratio * 100.0
            )
        }
    };
    let width = ratatui::crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);
    match text.char_indices().nth(width.saturating_sub(3)) {
        Some((byte, _)) => text[..byte].to_string(),
        None => text,
    }
}

/// An animated wait indicator held for the lifetime of a daemon round-trip:
/// `let _spinner = Spinner::start("…");` clears itself on drop. A no-op when
/// stderr is not a terminal.
pub struct Spinner {
    animator: Option<Animator>,
}

impl Spinner {
    pub fn start(label: impl Into<String>) -> Self {
        Self {
            animator: Animator::start(View::Spinner(label.into())),
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if let Some(animator) = &self.animator {
            animator.stop();
        }
    }
}

/// A byte gauge with optional spinner phases: live on a terminal, one line per new label when redirected.
pub struct ByteReporter {
    animator: Option<Animator>,
    rate: Mutex<RateMeter>,
    last_label: Mutex<Option<&'static str>>,
}

impl ByteReporter {
    pub fn new(label: &'static str) -> Self {
        let reporter = Self {
            animator: Animator::start(View::Spinner(label.into())),
            rate: Mutex::new(RateMeter::default()),
            last_label: Mutex::new(None),
        };
        reporter.announce(label);
        reporter
    }

    fn announce(&self, label: &'static str) {
        let mut last = self.last_label.lock().unwrap();
        if self.animator.is_none() && *last != Some(label) {
            eprintln!("{label}");
        }
        *last = Some(label);
    }

    pub fn phase(&self, label: &'static str) {
        self.announce(label);
        if let Some(animator) = &self.animator {
            animator.set(View::Spinner(label.into()));
        }
    }

    pub fn bytes(&self, label: &'static str, current: u64, total: u64) {
        self.announce(label);
        let Some(animator) = &self.animator else {
            return;
        };
        let rate = self.rate.lock().unwrap().observe(current);
        animator.set(View::Gauge {
            label,
            ratio: count_ratio(current, total),
            detail: size_detail(current, total, rate),
        });
    }

    /// Clear the gauge so a following message prints on a clean line.
    pub fn finish(&self) {
        if let Some(animator) = &self.animator {
            animator.stop();
        }
    }
}

pub type OnProgress = Box<dyn Fn(&ProvisionProgress) + Send + Sync>;

/// Reports provisioning progress (server create / instance launch): a live
/// gauge on a terminal, one line per phase when stderr is redirected. Byte
/// phases (java, jars) show sizes and throughput; count phases (libraries,
/// assets) show completed/total.
pub struct ProvisionReporter {
    animator: Option<Animator>,
    rate: Mutex<RateMeter>,
    last_phase: Mutex<Option<ProvisionPhase>>,
    last_detail: Mutex<String>,
}

impl ProvisionReporter {
    fn new() -> Self {
        Self {
            animator: Animator::start(View::Spinner(
                provision_label(ProvisionPhase::Resolving).into(),
            )),
            rate: Mutex::new(RateMeter::default()),
            last_phase: Mutex::new(None),
            last_detail: Mutex::new(String::new()),
        }
    }

    /// Shows progress while `work` runs and clears it afterwards, success or
    /// not. `initial` seeds the display before the daemon's first event.
    pub async fn run<T>(
        initial: Option<ProvisionProgress>,
        work: impl AsyncFnOnce(OnProgress) -> T,
    ) -> T {
        let reporter = Arc::new(Self::new());
        if let Some(initial) = &initial {
            reporter.update(initial);
        }
        let progress = reporter.clone();
        let result = work(Box::new(move |p| progress.update(p))).await;
        reporter.finish();
        result
    }

    /// Redirected: print each new thing a long step says, not just its phase.
    fn echo_narration(&self, progress: &ProvisionProgress) {
        if progress.total > 0 || progress.detail.is_empty() {
            return;
        }
        let mut last = self.last_detail.lock().unwrap();
        if *last != progress.detail {
            last.clone_from(&progress.detail);
            eprintln!("  {}", progress.detail);
        }
    }

    fn update(&self, progress: &ProvisionProgress) {
        let phase_changed = {
            let mut last = self.last_phase.lock().unwrap();
            let changed = *last != Some(progress.phase);
            *last = Some(progress.phase);
            changed
        };
        let Some(animator) = &self.animator else {
            if phase_changed {
                eprintln!("{}", provision_label(progress.phase));
            }
            self.echo_narration(progress);
            return;
        };
        if phase_changed {
            *self.rate.lock().unwrap() = RateMeter::default();
        }
        let view = match progress.phase {
            _ if progress.total == 0 && !progress.detail.is_empty() => {
                View::Spinner(progress.detail.clone())
            }
            ProvisionPhase::Java
            | ProvisionPhase::Server
            | ProvisionPhase::Client
            | ProvisionPhase::Content => {
                let rate = self.rate.lock().unwrap().observe(progress.current);
                View::Gauge {
                    label: gauge_label(progress.phase),
                    ratio: overall_ratio(progress),
                    detail: bytes_detail(progress, rate),
                }
            }
            ProvisionPhase::Libraries
            | ProvisionPhase::Assets
            | ProvisionPhase::Backup
            | ProvisionPhase::Extract
                if progress.total > 0 =>
            {
                View::Gauge {
                    label: gauge_label(progress.phase),
                    ratio: count_ratio(progress.current, progress.total),
                    detail: format!(
                        "{} · {}/{}",
                        provision_noun(progress.phase),
                        progress.current,
                        progress.total
                    ),
                }
            }
            phase => View::Spinner(provision_label(phase).into()),
        };
        animator.set(view);
    }

    /// Clear the gauge so a following message prints on a clean line.
    fn finish(&self) {
        if let Some(animator) = &self.animator {
            animator.stop();
        }
    }
}

fn count_ratio(current: u64, total: u64) -> f64 {
    if total > 0 {
        (current as f64 / total as f64).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The gauge ratio for a progress event. A multi-unit phase (`items > 0`)
/// fills monotonically across the whole batch — completed units plus the
/// current unit's byte fraction — so cached or instant units still advance
/// the bar instead of leaving it stuck at a per-file reset.
pub(crate) fn overall_ratio(progress: &ProvisionProgress) -> f64 {
    progress.ratio()
}

fn bytes_detail(progress: &ProvisionProgress, rate: f64) -> String {
    let mut parts = Vec::new();
    if progress.items > 0 {
        parts.push(format!("{}/{}", progress.item, progress.items));
    }
    if progress.detail.is_empty() {
        parts.push(provision_noun(progress.phase).to_string());
    } else {
        parts.push(progress.detail.clone());
    }
    parts.push(size_detail(progress.current, progress.total, rate));
    parts.join(" · ")
}

fn size_detail(current: u64, total: u64, rate: f64) -> String {
    let total = if total > 0 {
        human_bytes(total)
    } else {
        "?".to_string()
    };
    let size = format!("{} / {total}", human_bytes(current));
    if rate > 0.0 {
        format!("{size} · {}/s", human_bytes(rate as u64))
    } else {
        size
    }
}

/// The word before a phase's gauge (`backing up ███…`, not `downloading` for
/// everything).
fn gauge_label(phase: ProvisionPhase) -> &'static str {
    match phase {
        ProvisionPhase::Backup => "backing up",
        ProvisionPhase::Libraries => "libraries",
        ProvisionPhase::Assets => "assets",
        ProvisionPhase::Overrides => "pack files",
        ProvisionPhase::Archive => "files",
        ProvisionPhase::Extract => "files",
        _ => "downloading",
    }
}

fn provision_label(phase: ProvisionPhase) -> &'static str {
    match phase {
        ProvisionPhase::Resolving => "resolving…",
        ProvisionPhase::Backup => "backing up…",
        ProvisionPhase::Java => "java runtime…",
        ProvisionPhase::Server => "server jar…",
        ProvisionPhase::Client => "client jar…",
        ProvisionPhase::Libraries => "libraries…",
        ProvisionPhase::Assets => "assets…",
        ProvisionPhase::Content => "downloading…",
        ProvisionPhase::Overrides => "pack files…",
        ProvisionPhase::Archive => "archiving…",
        ProvisionPhase::Extract => "extracting…",
    }
}

fn provision_noun(phase: ProvisionPhase) -> &'static str {
    match phase {
        ProvisionPhase::Resolving => "profile",
        ProvisionPhase::Backup => "files",
        ProvisionPhase::Java => "java",
        ProvisionPhase::Server => "server jar",
        ProvisionPhase::Client => "client jar",
        ProvisionPhase::Libraries => "libraries",
        ProvisionPhase::Assets => "assets",
        ProvisionPhase::Content => "content",
        ProvisionPhase::Overrides => "pack files",
        ProvisionPhase::Archive => "files",
        ProvisionPhase::Extract => "files",
    }
}

/// Byte rate measured over fixed minimum windows. Progress events arrive over
/// the socket in bursts (one per chunk, several within microseconds), so a
/// per-event instantaneous rate is dominated by intra-burst spikes and wildly
/// overstates throughput; averaging each ≥`RATE_WINDOW` span weights fast and
/// stalled periods by their real duration. A counter that goes backwards is a
/// new stream: the previous stream's rate is discarded, and nothing shows
/// until the new one has sustained a full window — a file served instantly
/// from the local cache never displays a speed at all.
#[derive(Default)]
struct RateMeter {
    window: Option<(Instant, u64)>,
    per_second: f64,
}

const RATE_WINDOW: Duration = Duration::from_millis(500);

impl RateMeter {
    fn observe(&mut self, current: u64) -> f64 {
        self.observe_at(Instant::now(), current)
    }

    fn observe_at(&mut self, now: Instant, current: u64) -> f64 {
        match self.window {
            Some((start, count)) if current >= count => {
                let elapsed = now.duration_since(start);
                if elapsed >= RATE_WINDOW {
                    let rate = (current - count) as f64 / elapsed.as_secs_f64();
                    self.per_second = if self.per_second == 0.0 {
                        rate
                    } else {
                        0.5 * self.per_second + 0.5 * rate
                    };
                    self.window = Some((now, current));
                }
            }
            _ => {
                self.per_second = 0.0;
                self.window = Some((now, current));
            }
        }
        self.per_second
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_ignores_intra_burst_spikes() {
        let start = Instant::now();
        let mut meter = RateMeter::default();
        // 10 KB/s real throughput, delivered as micro-bursts of 5 events.
        let mut rate = 0.0;
        for second in 0..4u64 {
            for burst in 0..5u64 {
                let t = start + Duration::from_secs(second) + Duration::from_micros(burst * 50);
                rate = meter.observe_at(t, second * 10_000 + burst * 2_000);
            }
        }
        assert!(
            (5_000.0..20_000.0).contains(&rate),
            "expected ~10 KB/s, got {rate} B/s"
        );
    }

    #[test]
    fn rate_resets_when_the_counter_goes_backwards() {
        let start = Instant::now();
        let mut meter = RateMeter::default();
        meter.observe_at(start, 100_000);
        let flowing = meter.observe_at(start + Duration::from_secs(1), 200_000);
        assert!(flowing > 0.0);
        let rate = meter.observe_at(start + Duration::from_secs(2), 1_000);
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn instant_transfer_never_reports_a_rate() {
        let start = Instant::now();
        let mut meter = RateMeter::default();
        meter.observe_at(start, 0);
        let rate = meter.observe_at(start + Duration::from_millis(40), 5_000_000);
        assert_eq!(rate, 0.0);
    }
}
