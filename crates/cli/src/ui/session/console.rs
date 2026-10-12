//! The attach console: live server output above an input line, as a session
//! screen. Detaching never touches the server — stopping a workload is always
//! an explicit act, not a side effect of closing a view.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{log_header, Flow, Screen};
use crate::ui::components::log::{Kind, LogView};
use crate::ui::components::TextInput;

const WHEEL: usize = 3;

pub enum ConsoleEvent {
    Output(String),
    Reply(String),
    Notice(String),
    /// Ends the console; the message is handed back to the caller.
    Closed(String),
}

#[derive(Default)]
pub struct Completions {
    pub line: String,
    pub completions: Vec<String>,
    pub arguments: Vec<String>,
}

pub struct ConsoleLink {
    pub commands: UnboundedSender<String>,
    pub lines: UnboundedSender<String>,
    pub completions: UnboundedReceiver<Completions>,
}

pub struct ConsoleScreen {
    title: String,
    log: LogView,
    input: TextInput,
    link: ConsoleLink,
    offered: Completions,
    cycle: Option<usize>,
}

impl ConsoleScreen {
    pub fn new(title: &str, backfill: Vec<String>, link: ConsoleLink) -> Self {
        let mut log = LogView::default();
        for line in backfill {
            log.push(Kind::Output, line);
        }
        ConsoleScreen {
            title: title.to_string(),
            log,
            input: TextInput::default(),
            link,
            offered: Completions::default(),
            cycle: None,
        }
    }

    fn edited(&mut self) {
        self.cycle = None;
        self.offered = Completions::default();
        if !self.input.is_empty() {
            let _ = self.link.lines.send(self.input.text().to_string());
        }
    }

    fn cycle(&mut self, forward: bool) {
        let count = self.offered.completions.len();
        if count == 0 {
            return;
        }
        let next = match (self.cycle, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(at), true) => (at + 1) % count,
            (Some(at), false) => (at + count - 1) % count,
        };
        let text = self.input.text();
        let word = text
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(at, c)| at + c.len_utf8());
        let completed = format!("{}{}", &text[..word], self.offered.completions[next]);
        self.input = TextInput::with_text(&completed);
        self.cycle = Some(next);
    }

    fn send(&mut self) -> bool {
        let command = self.input.take().trim().to_string();
        self.edited();
        if command.is_empty() {
            return true;
        }
        self.log.push(Kind::Echo, format!("» {command}"));
        self.log.follow();
        self.link.commands.send(command).is_ok()
    }
}

impl Screen for ConsoleScreen {
    type Event = ConsoleEvent;
    type Outcome = Option<String>;

    fn wants_mouse(&self) -> bool {
        true
    }

    fn draw(&mut self, frame: &mut Frame) {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());
        log_header(
            frame,
            rows[0],
            &self.title,
            self.log.scroll(),
            "Enter send · Tab complete · Esc detach · ↑/↓ scroll",
        );
        self.log.render(frame, rows[1]);
        draw_offered(frame, rows[2], &self.offered, self.cycle);
        draw_input(frame, rows[3], &self.input);
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        match key.code {
            KeyCode::Esc => return Flow::Done(None),
            KeyCode::Char('c' | 'd') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Flow::Done(None)
            }
            KeyCode::Enter => {
                if !self.send() {
                    return Flow::Done(Some("console closed".into()));
                }
            }
            KeyCode::Tab => self.cycle(true),
            KeyCode::BackTab => self.cycle(false),
            KeyCode::Up => self.log.scroll_up(1),
            KeyCode::Down => self.log.scroll_down(1),
            KeyCode::PageUp => self.log.scroll_up(self.log.page()),
            KeyCode::PageDown => self.log.scroll_down(self.log.page()),
            _ => {
                let before = self.input.text().to_string();
                self.input.on_key(&key);
                if self.input.text() != before {
                    self.edited();
                }
            }
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

    fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Ok(offered) = self.link.completions.try_recv() {
            if self.cycle.is_none() && offered.line == self.input.text() {
                self.offered = offered;
                changed = true;
            }
        }
        changed
    }
}

fn draw_offered(frame: &mut Frame, area: Rect, offered: &Completions, cycle: Option<usize>) {
    let dim = Style::default().fg(Color::DarkGray);
    let chosen = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let first = cycle.unwrap_or(0);
    let completions = offered
        .completions
        .iter()
        .enumerate()
        .skip(first)
        .map(|(at, word)| (word, if cycle == Some(at) { chosen } else { dim }));
    let arguments = offered
        .arguments
        .iter()
        .map(|argument| (argument, dim.add_modifier(Modifier::ITALIC)));
    let spans: Vec<Span> = completions
        .chain(arguments)
        .flat_map(|(word, style)| [Span::styled(word.as_str(), style), Span::raw("  ")])
        .collect();
    let [_, row] = Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

fn draw_input(frame: &mut Frame, area: Rect, input: &TextInput) {
    let [prompt_area, input_area] =
        Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(Span::styled("> ", Style::default().fg(Color::Cyan))),
        prompt_area,
    );
    input.render(frame, input_area);
}
