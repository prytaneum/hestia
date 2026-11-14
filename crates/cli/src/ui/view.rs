//! The presentation-agnostic output model. Commands build `View`s and hand them
//! to `ui::show`; the renderer decides how to present them (ratatui on a
//! terminal, plain text when piped). The future TUI consumes the same `View`s.

use super::cell::Cell;

pub enum View {
    /// A primary line of output.
    Line(String),
    /// A secondary line (dimmed on a terminal): empty-state notes, hints.
    Note(String),
    /// A degraded outcome of an operation that otherwise succeeded. Deliberately
    /// heavier than a `Note`: the command worked, but not the way the user asked.
    Warning(String),
    /// A key/value block, keys aligned.
    Detail(Vec<(String, Cell)>),
    /// One thing's state followed by its details: a line and a block for a
    /// person, one object with a leading `state` under `--json`.
    Status {
        state: String,
        rows: Vec<(String, Cell)>,
    },
    /// A titled table; long tables page interactively on a terminal. `keys` name
    /// the columns for `--json`, one per header.
    Table {
        title: String,
        headers: Vec<String>,
        keys: Vec<String>,
        rows: Vec<Vec<Cell>>,
    },
    /// A list with nothing in it: the note a person reads, and `[]` under
    /// `--json`, so a script never has to tell an empty list from a message.
    Empty(String),
    /// A structured value a command already holds, printed as-is under
    /// `--json` and pretty-printed otherwise.
    Json(serde_json::Value),
}

/// The machine key for a display label: lowercased, every run of other
/// characters becoming one `_`.
pub fn key_of(label: &str) -> String {
    label
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("_")
}

fn rows_of<K: Into<String>, V: Into<Cell>>(
    rows: impl IntoIterator<Item = (K, V)>,
) -> Vec<(String, Cell)> {
    rows.into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

impl View {
    pub fn line(text: impl Into<String>) -> View {
        View::Line(text.into())
    }

    pub fn note(text: impl Into<String>) -> View {
        View::Note(text.into())
    }

    pub fn empty(text: impl Into<String>) -> View {
        View::Empty(text.into())
    }

    pub fn warning(text: impl Into<String>) -> View {
        View::Warning(text.into())
    }

    pub fn detail<K: Into<String>, V: Into<Cell>>(rows: impl IntoIterator<Item = (K, V)>) -> View {
        View::Detail(rows_of(rows))
    }

    pub fn status<K: Into<String>, V: Into<Cell>>(
        state: impl Into<String>,
        rows: impl IntoIterator<Item = (K, V)>,
    ) -> View {
        View::Status {
            state: state.into(),
            rows: rows_of(rows),
        }
    }

    pub fn table<C: Into<Cell>>(
        title: impl Into<String>,
        headers: impl IntoIterator<Item = impl Into<String>>,
        rows: Vec<Vec<C>>,
    ) -> View {
        let headers: Vec<String> = headers.into_iter().map(Into::into).collect();
        View::Table {
            title: title.into(),
            keys: headers.iter().map(|header| key_of(header)).collect(),
            headers,
            rows: rows
                .into_iter()
                .map(|row| row.into_iter().map(Into::into).collect())
                .collect(),
        }
    }

    /// Name a table's `--json` keys where the snake_case of its headers would
    /// read badly to a script; the displayed headers stay as they are.
    pub fn keyed<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> View {
        if let View::Table { keys, .. } = &mut self {
            *keys = names.into_iter().map(str::to_owned).collect();
        }
        self
    }

    pub fn json(value: serde_json::Value) -> View {
        View::Json(value)
    }
}
