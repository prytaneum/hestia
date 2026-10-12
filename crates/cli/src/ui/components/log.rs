//! A scrolling log viewport: kind-styled lines with a capped history,
//! wrap-aware scrolling (`Paragraph::line_count` drives exact row math), and
//! a reading position that stays anchored while output streams in below it.

use std::collections::VecDeque;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

const HISTORY: usize = 2000;

#[derive(Clone, Copy)]
pub enum Kind {
    Output,
    Echo,
    Reply,
    Notice,
}

impl Kind {
    fn style(self) -> Style {
        match self {
            Kind::Output => Style::default(),
            Kind::Echo => Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            Kind::Reply => Style::default().fg(Color::Cyan),
            Kind::Notice => Style::default().fg(Color::Yellow),
        }
    }
}

struct Entry {
    kind: Kind,
    text: String,
}

impl Entry {
    fn line(&self) -> Line<'_> {
        Line::styled(self.text.as_str(), self.kind.style())
    }
}

/// The viewport as measured at the last draw: what scroll clamping and
/// scroll anchoring need between frames.
#[derive(Default, Clone, Copy)]
struct View {
    width: u16,
    height: usize,
    max_scroll: usize,
}

#[derive(Default)]
pub struct LogView {
    lines: VecDeque<Entry>,
    /// Wrapped rows scrolled up from the live tail; 0 follows new output.
    scroll: usize,
    view: View,
}

impl LogView {
    pub fn push(&mut self, kind: Kind, text: String) {
        if let Some((first, rest)) = text.split_once('\n') {
            self.push(kind, first.trim_end_matches('\r').to_string());
            self.push(kind, rest.to_string());
            return;
        }
        if self.lines.len() == HISTORY {
            self.lines.pop_front();
        }
        let entry = Entry { kind, text };
        // Anchor the reading position: rows appended below the tail while
        // scrolled up must not slide the view towards them.
        if self.scroll > 0 {
            self.scroll += wrapped_rows(&entry, self.view.width);
        }
        self.lines.push_back(entry);
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn scroll_up(&mut self, by: usize) {
        self.scroll = (self.scroll + by).min(self.view.max_scroll);
    }

    pub fn scroll_down(&mut self, by: usize) {
        self.scroll = self.scroll.saturating_sub(by);
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.scroll = 0;
    }

    /// Re-follow the live tail.
    pub fn follow(&mut self) {
        self.scroll = 0;
    }

    pub fn page(&self) -> usize {
        self.view.height.max(1)
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let text = Text::from(self.lines.iter().map(Entry::line).collect::<Vec<_>>());
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let total = paragraph.line_count(area.width);
        let height = area.height as usize;
        let max_scroll = total.saturating_sub(height);
        self.scroll = self.scroll.min(max_scroll);
        self.view = View {
            width: area.width,
            height,
            max_scroll,
        };
        let top = max_scroll - self.scroll;
        frame.render_widget(
            paragraph.scroll((top.min(u16::MAX as usize) as u16, 0)),
            area,
        );
    }
}

fn wrapped_rows(entry: &Entry, width: u16) -> usize {
    Paragraph::new(entry.line())
        .wrap(Wrap { trim: false })
        .line_count(width)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_with_width(width: u16) -> LogView {
        let mut log = LogView::default();
        log.view.width = width;
        log.view.max_scroll = usize::MAX;
        log
    }

    #[test]
    fn push_splits_multiline_payloads() {
        let mut log = log_with_width(80);
        log.push(Kind::Reply, "a\r\nb\nc".into());
        let texts: Vec<&str> = log.lines.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
    }

    #[test]
    fn push_caps_history() {
        let mut log = log_with_width(80);
        for i in 0..(HISTORY + 5) {
            log.push(Kind::Output, i.to_string());
        }
        assert_eq!(log.lines.len(), HISTORY);
        assert_eq!(log.lines.front().unwrap().text, "5");
    }

    #[test]
    fn scrolled_view_stays_anchored_as_output_arrives() {
        let mut log = log_with_width(10);
        log.push(Kind::Output, "old".into());
        log.scroll = 3;
        log.push(Kind::Output, "short".into());
        assert_eq!(log.scroll, 4);
        log.push(Kind::Output, "x".repeat(25));
        assert_eq!(log.scroll, 7);
    }

    #[test]
    fn following_tail_ignores_new_output() {
        let mut log = log_with_width(10);
        log.push(Kind::Output, "line".into());
        assert_eq!(log.scroll, 0);
    }
}
