//! The one-shot prompt screens behind the `ui` facade — select, multi-select,
//! picker — each a small fullscreen session over the shared
//! components. Borderless: the prompt on top, the body below it, a dim key
//! hint on the bottom row.

use std::convert::Infallible;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::{is_cancel, Flow, Screen};
use crate::ui::components::{Picker, PickerItem, SelectList};

/// Draw the prompt row and hint row, returning the body area between them.
fn chrome(frame: &mut Frame, prompt: &str, hint: &str) -> Rect {
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(
        Paragraph::new(Line::from(prompt)).style(Style::default().fg(Color::Cyan)),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(hint)).style(Style::default().fg(Color::DarkGray)),
        rows[3],
    );
    rows[2]
}

pub struct SelectScreen {
    prompt: String,
    list: SelectList,
}

impl SelectScreen {
    pub fn new(prompt: &str, items: &[String], details: &[String]) -> Self {
        SelectScreen {
            prompt: prompt.to_string(),
            list: SelectList::new(items.to_vec()).with_details(details.to_vec()),
        }
    }
}

impl Screen for SelectScreen {
    type Event = Infallible;
    type Outcome = Option<usize>;

    fn draw(&mut self, frame: &mut Frame) {
        let body = chrome(frame, &self.prompt, "↑/↓ move · enter select · esc cancel");
        self.list.render(frame, body);
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        if is_cancel(&key) {
            return Flow::Done(None);
        }
        if key.code == KeyCode::Enter {
            return Flow::Done(Some(self.list.selected()));
        }
        self.list.on_key(&key);
        Flow::Continue
    }
}

pub struct MultiSelectScreen {
    prompt: String,
    list: SelectList,
}

impl MultiSelectScreen {
    pub fn new(prompt: &str, items: &[String]) -> Self {
        MultiSelectScreen {
            prompt: prompt.to_string(),
            list: SelectList::new(items.to_vec()).with_checkboxes(),
        }
    }
}

impl Screen for MultiSelectScreen {
    type Event = Infallible;
    type Outcome = Option<Vec<usize>>;

    fn draw(&mut self, frame: &mut Frame) {
        let body = chrome(
            frame,
            &self.prompt,
            "↑/↓ move · space toggle · enter confirm · esc cancel",
        );
        self.list.render(frame, body);
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        if is_cancel(&key) {
            return Flow::Done(None);
        }
        if key.code == KeyCode::Enter {
            return Flow::Done(Some(self.list.chosen()));
        }
        self.list.on_key(&key);
        Flow::Continue
    }
}

pub struct PickerScreen {
    prompt: String,
    picker: Picker,
}

impl PickerScreen {
    pub fn new(prompt: &str, items: Vec<PickerItem>) -> Self {
        PickerScreen {
            prompt: prompt.to_string(),
            picker: Picker::new(items),
        }
    }
}

impl Screen for PickerScreen {
    type Event = Infallible;
    type Outcome = Option<usize>;

    fn draw(&mut self, frame: &mut Frame) {
        let rows =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(frame.area());
        self.picker.render(frame, rows[0], &self.prompt);
        frame.render_widget(
            Paragraph::new(Line::from(
                "type to filter · tab all versions · ↑/↓ move · enter select · esc cancel",
            ))
            .style(Style::default().fg(Color::DarkGray)),
            rows[1],
        );
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        match key.code {
            KeyCode::Esc => return Flow::Done(None),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Flow::Done(None)
            }
            KeyCode::Enter => {
                if let Some(index) = self.picker.selected() {
                    return Flow::Done(Some(index));
                }
            }
            _ => {
                self.picker.on_key(&key);
            }
        }
        Flow::Continue
    }
}
