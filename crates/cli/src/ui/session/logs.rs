//! The read-only log session: a fullscreen follower over a process's output —
//! the attach console minus the input line. Detaching never touches the
//! workload; Esc/q/Ctrl-C hand the terminal back and leave it running.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;

use super::console::ConsoleEvent;
use super::{log_header, Flow, Screen};
use crate::ui::components::log::{Kind, LogView};

const WHEEL: usize = 3;

pub struct LogScreen {
    title: String,
    log: LogView,
}

impl LogScreen {
    pub fn new(title: &str, backfill: Vec<String>) -> Self {
        let mut log = LogView::default();
        for line in backfill {
            log.push(Kind::Output, line);
        }
        LogScreen {
            title: title.to_string(),
            log,
        }
    }
}

impl Screen for LogScreen {
    type Event = ConsoleEvent;
    type Outcome = Option<String>;

    fn wants_mouse(&self) -> bool {
        true
    }

    fn draw(&mut self, frame: &mut Frame) {
        let rows =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(frame.area());
        log_header(
            frame,
            rows[0],
            &self.title,
            self.log.scroll(),
            "Esc detach · ↑/↓ scroll",
        );
        self.log.render(frame, rows[1]);
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Flow::Done(None),
            KeyCode::Char('c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Flow::Done(None)
            }
            KeyCode::Up => self.log.scroll_up(1),
            KeyCode::Down => self.log.scroll_down(1),
            KeyCode::PageUp => self.log.scroll_up(self.log.page()),
            KeyCode::PageDown => self.log.scroll_down(self.log.page()),
            KeyCode::End => self.log.follow(),
            _ => {}
        }
        Flow::Continue
    }

    fn on_mouse(&mut self, mouse: MouseEvent) -> Flow<Self::Outcome> {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.log.scroll_up(WHEEL),
            MouseEventKind::ScrollDown => self.log.scroll_down(WHEEL),
            _ => {}
        }
        Flow::Continue
    }

    fn on_event(&mut self, event: ConsoleEvent) -> Flow<Self::Outcome> {
        match event {
            ConsoleEvent::Output(text) => self.log.push(Kind::Output, text),
            ConsoleEvent::Reply(text) => self.log.push(Kind::Reply, text),
            ConsoleEvent::Notice(text) => self.log.push(Kind::Notice, text),
            ConsoleEvent::Closed(message) => return Flow::Done(Some(message)),
        }
        Flow::Continue
    }
}
