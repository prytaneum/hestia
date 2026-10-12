//! The fullscreen session runtime. Every interactive surface is a [`Screen`]
//! run in the alternate screen: the runtime owns the terminal lifecycle (raw
//! mode, alternate screen, restore-on-drop), the event loop (50 ms input
//! poll, drain-before-redraw, dirty-flag drawing), resize re-wrapping, and
//! the 80×24 minimum-size notice — a screen only draws its state and reacts
//! to keys, mouse, ticks, and injected app events.

pub mod console;
pub mod logs;
pub mod monitor;
pub mod pager;
pub mod prompt;
mod terminal;

use std::io;
use std::time::Duration;

use anyhow::Result;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::UnboundedReceiver;

pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

const POLL: Duration = Duration::from_millis(50);

/// A screen's answer to an input or event: keep running, or resolve the
/// session with an outcome.
pub enum Flow<T> {
    Continue,
    Done(T),
}

/// One fullscreen session's state and behavior. `Event` is the app-event type
/// an async driver injects ([`std::convert::Infallible`] for pure-key
/// screens); `Outcome` is what the session resolves to.
pub trait Screen {
    type Event;
    type Outcome;

    fn draw(&mut self, frame: &mut Frame);

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome>;

    fn on_mouse(&mut self, _mouse: MouseEvent) -> Flow<Self::Outcome> {
        Flow::Continue
    }

    fn on_event(&mut self, _event: Self::Event) -> Flow<Self::Outcome> {
        Flow::Continue
    }

    /// Advance time-based state (debounce deadlines, animations) once per
    /// poll interval; `true` requests a redraw.
    fn tick(&mut self) -> bool {
        false
    }

    /// Whether the session captures mouse events (wheel scrolling).
    fn wants_mouse(&self) -> bool {
        false
    }
}

/// Run a screen to its outcome, owning the whole terminal for the duration.
/// Blocking — call [`run_async`] from async code that must keep pumping the
/// event channel.
pub fn run<S: Screen>(
    mut screen: S,
    events: Option<UnboundedReceiver<S::Event>>,
) -> Result<S::Outcome> {
    let _guard = terminal::TerminalGuard::acquire(screen.wants_mouse())?;
    drive(&mut screen, events)
}

/// [`run`] off the async runtime, for sessions whose event channel is fed by
/// a driver future the caller must keep polling concurrently.
pub async fn run_async<S>(
    screen: S,
    events: Option<UnboundedReceiver<S::Event>>,
) -> Result<S::Outcome>
where
    S: Screen + Send + 'static,
    S::Event: Send + 'static,
    S::Outcome: Send + 'static,
{
    tokio::task::spawn_blocking(move || run(screen, events)).await?
}

fn drive<S: Screen>(
    screen: &mut S,
    mut events: Option<UnboundedReceiver<S::Event>>,
) -> Result<S::Outcome> {
    // Stderr is unbuffered and the backend queues one write per changed cell
    // run; buffering makes each frame reach the terminal as a single flush
    // instead of a visible cell-by-cell repaint. 64 KiB holds a worst-case
    // full styled redraw, so no frame splits mid-buffer.
    let writer = io::BufWriter::with_capacity(64 * 1024, io::stderr());
    let mut terminal = Terminal::new(CrosstermBackend::new(writer))?;
    let mut dirty = true;
    loop {
        let mut disconnected = false;
        if let Some(receiver) = events.as_mut() {
            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        dirty = true;
                        if let Flow::Done(outcome) = screen.on_event(event) {
                            return Ok(outcome);
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if disconnected {
            events = None;
        }
        if screen.tick() {
            dirty = true;
        }
        if dirty {
            terminal.draw(|frame| {
                let area = frame.area();
                if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
                    draw_too_small(frame, area);
                } else {
                    screen.draw(frame);
                }
            })?;
            dirty = false;
        }
        if !event::poll(POLL)? {
            continue;
        }
        // Drain every pending event before redrawing, so a paste or held key
        // costs one frame, not one frame per keystroke.
        loop {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Flow::Done(outcome) = screen.on_key(key) {
                        return Ok(outcome);
                    }
                }
                Event::Mouse(mouse) => {
                    if let Flow::Done(outcome) = screen.on_mouse(mouse) {
                        return Ok(outcome);
                    }
                }
                _ => {}
            }
            dirty = true;
            if !event::poll(Duration::ZERO)? {
                break;
            }
        }
    }
}

/// Whether a key is the shared cancel gesture for list-driven screens
/// (Esc / q / Ctrl-C). Screens that take text input handle Esc themselves.
pub fn is_cancel(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
}

/// The one-line header the log sessions share: a cyan title on the left and a
/// dim right-aligned hint that flips to the scroll position while scrolled up
/// from the tail, else shows the screen's own key hints.
pub(super) fn log_header(frame: &mut Frame, area: Rect, title: &str, scroll: usize, actions: &str) {
    let hint = if scroll > 0 {
        format!("scrolled ↑{scroll} · ↓ follows")
    } else {
        actions.to_string()
    };
    let hint = Span::styled(hint, Style::default().fg(Color::DarkGray));
    let [title_area, hint_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(hint.width() as u16)])
            .areas(area);
    frame.render_widget(
        Paragraph::new(Span::styled(
            title.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        title_area,
    );
    frame.render_widget(Paragraph::new(hint), hint_area);
}

fn draw_too_small(frame: &mut Frame, area: Rect) {
    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(Line::styled(
            format!("terminal too small — need {MIN_WIDTH}×{MIN_HEIGHT}"),
            Style::default().fg(Color::Yellow),
        ))
        .centered(),
        rows[1],
    );
}
