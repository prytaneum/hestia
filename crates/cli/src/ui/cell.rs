//! One value of a table or detail, with two faces: the text a person reads and
//! the typed value `--json` carries. A plain string is both, so a call site
//! with nothing better to offer passes its string.

use chrono::{DateTime, SecondsFormat};
use serde::Serialize;
use serde_json::{Number, Value};

use super::human_bytes;

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    text: String,
    value: Value,
    shown: bool,
}

impl Cell {
    fn new(text: impl Into<String>, value: Value) -> Cell {
        Cell {
            text: text.into(),
            value,
            shown: true,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// A count, an id-like number or any other whole number.
    pub fn count(number: impl Into<Number>) -> Cell {
        let number = number.into();
        Cell::new(number.to_string(), Value::Number(number))
    }

    /// A size: the text in human units, the value in bytes.
    pub fn bytes(bytes: u64) -> Cell {
        Cell::new(human_bytes(bytes), bytes.into())
    }

    /// A duration: `text` as a person reads it, the value in seconds.
    pub fn seconds(seconds: u64, text: impl Into<String>) -> Cell {
        Cell::new(text, seconds.into())
    }

    /// A point in time: `text` as a person reads it (often relative), the
    /// value an RFC 3339 string in UTC. A stamp outside the representable
    /// range is absent.
    pub fn time(unix_seconds: i64, text: impl Into<String>) -> Cell {
        let value = DateTime::from_timestamp(unix_seconds, 0).map_or(Value::Null, |at| {
            Value::String(at.to_rfc3339_opts(SecondsFormat::Secs, true))
        });
        Cell::new(text, value)
    }

    pub fn flag(flag: bool, text: impl Into<String>) -> Cell {
        Cell::new(text, Value::Bool(flag))
    }

    /// A state or kind: `text` as a person reads it, the value its wire token.
    pub fn token(text: impl Into<String>, token: &impl Serialize) -> Cell {
        Cell::new(text, serde_json::to_value(token).unwrap_or(Value::Null))
    }

    /// A list shown as `text`, an array of its items (strings or wire tokens)
    /// under `--json`.
    pub fn list<I>(text: impl Into<String>, items: I) -> Cell
    where
        I: IntoIterator,
        I::Item: Serialize,
    {
        let items = items
            .into_iter()
            .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
            .collect();
        Cell::new(text, Value::Array(items))
    }

    /// Nothing to show: `text` is the placeholder a person sees, the value null.
    pub fn absent(text: impl Into<String>) -> Cell {
        Cell::new(text, Value::Null)
    }

    /// A value only `--json` carries; the text rendering leaves it out.
    pub fn json_only(value: impl Serialize) -> Cell {
        Cell {
            text: String::new(),
            value: serde_json::to_value(value).unwrap_or(Value::Null),
            shown: false,
        }
    }

    /// Any serializable value, shown as `text`, for what no other constructor
    /// fits (a pair of numbers, a small object).
    pub fn typed(text: impl Into<String>, value: impl Serialize) -> Cell {
        Cell::new(text, serde_json::to_value(value).unwrap_or(Value::Null))
    }

    /// A string that may be missing: absent, shown as `placeholder`, when it is.
    pub fn maybe(text: Option<impl Into<String>>, placeholder: &str) -> Cell {
        text.map_or_else(|| Cell::absent(placeholder), |text| Cell::from(text.into()))
    }

    /// A JSON value the command already holds: a string reads as itself, any
    /// other value as its JSON text, and `--json` carries it untouched.
    pub fn raw(value: Value) -> Cell {
        let text = match &value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        Cell::new(text, value)
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Cell {
        let value = Value::String(text.clone());
        Cell::new(text, value)
    }
}

impl From<&str> for Cell {
    fn from(text: &str) -> Cell {
        Cell::from(text.to_string())
    }
}

impl From<&String> for Cell {
    fn from(text: &String) -> Cell {
        Cell::from(text.clone())
    }
}
