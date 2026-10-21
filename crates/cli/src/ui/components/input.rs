//! A single-line text field: char-indexed cursor editing, rendered with
//! horizontal scrolling and a real terminal cursor at the caret.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::Frame;

#[derive(Default)]
pub struct TextInput {
    text: String,
    cursor: usize,
}

impl TextInput {
    pub fn with_text(text: &str) -> Self {
        TextInput {
            text: text.to_string(),
            cursor: text.chars().count(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(byte_of(&self.text, self.cursor), c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.text.remove(byte_of(&self.text, self.cursor));
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.chars().count() {
            self.text.remove(byte_of(&self.text, self.cursor));
        }
    }

    /// Apply an editing or caret-movement key; `true` when consumed.
    pub fn on_key(&mut self, key: &KeyEvent) -> bool {
        match key.code {
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => self.insert(c),
            _ => return false,
        }
        true
    }

    /// Draw the field into `area`, scrolled so the caret stays visible, and
    /// place the terminal cursor at the caret.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        self.render_focused(frame, area, true);
    }

    /// [`render`], but only claim the terminal cursor when `focused` — a field
    /// sharing a screen with other focus targets stays visible without the
    /// caret pretending to own the keys.
    pub fn render_focused(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let caret = Span::raw(&self.text[..byte_of(&self.text, self.cursor)]).width() as u16;
        let overflow = caret.saturating_sub(area.width.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(self.text.as_str()).scroll((0, overflow)),
            area,
        );
        if focused {
            frame.set_cursor_position(Position::new(area.x + caret - overflow, area.y));
        }
    }
}

fn byte_of(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .nth(cursor)
        .map_or(text.len(), |(byte, _)| byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_at_char_cursor() {
        let mut input = TextInput::default();
        for c in "héllo".chars() {
            input.insert(c);
        }
        input.cursor = 1;
        input.delete();
        assert_eq!(input.text, "hllo");
        input.insert('a');
        assert_eq!(input.text, "hallo");
        input.backspace();
        assert_eq!(input.text, "hllo");
        assert_eq!(input.cursor, 1);
    }
}
