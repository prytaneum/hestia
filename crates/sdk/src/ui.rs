//! Builders for the trees a page returns from [`Pages::on`](crate::pages::Pages::on).
//! They produce the same [`UiNode`] values the front-ends draw, so anything
//! the builders do not cover can be written as the enum directly.
//!
//! A button or a form carries an action: the plugin's own enum, tagged
//! `#[serde(tag = "id", content = "payload")]` so it has the wire form of an
//! [`Action`], which [`Pages::on`](crate::pages::Pages::on) reads back typed.
//!
//! ```
//! use hestia::ui;
//!
//! #[derive(serde::Serialize, serde::Deserialize)]
//! #[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
//! enum Act {
//!     Backup,
//! }
//!
//! let page = ui::stack([
//!     ui::heading("Backups", 1),
//!     ui::table(["World", "Size"], [ui::row(["smp", "1.2 GB"])]),
//!     ui::button("Back up now", Act::Backup),
//! ]);
//! ```

use proto::ui::{Direction, Pair, Submit, TableRow, Variant};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use proto::ui::{Action, Effect, FormField, Tone, UiNode};

/// The [`Action`] a button or form sends, written from the plugin's own enum.
/// It must serialize as an object with an `id` and, optionally, a `payload`,
/// which `#[serde(tag = "id", content = "payload")]` gives an enum; a value of
/// any other shape becomes an action no handler can read, so the click answers
/// [`Error::Invalid`](crate::Error::Invalid).
///
/// ```
/// use hestia::ui;
///
/// #[derive(serde::Serialize)]
/// #[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
/// enum Act {
///     Remove(String),
/// }
///
/// let action = ui::action(Act::Remove("smp".into()));
/// assert_eq!(action.id, "remove");
/// assert_eq!(action.payload, hestia::json!("smp"));
/// ```
pub fn action(action: impl Serialize) -> Action {
    #[derive(Deserialize)]
    struct Wire {
        id: String,
        #[serde(default)]
        payload: Value,
    }
    match serde_json::to_value(action).and_then(serde_json::from_value::<Wire>) {
        Ok(Wire { id, payload }) => Action { id, payload },
        Err(error) => Action {
            id: format!("invalid action: {error}"),
            payload: Value::Null,
        },
    }
}

pub fn stack(children: impl IntoIterator<Item = UiNode>) -> UiNode {
    UiNode::Stack {
        direction: Direction::Vertical,
        children: children.into_iter().collect(),
    }
}

pub fn row_of(children: impl IntoIterator<Item = UiNode>) -> UiNode {
    UiNode::Stack {
        direction: Direction::Horizontal,
        children: children.into_iter().collect(),
    }
}

/// `level` is 1 to 3.
pub fn heading(text: impl Into<String>, level: u8) -> UiNode {
    UiNode::Heading {
        text: text.into(),
        level,
    }
}

pub fn text(text: impl Into<String>) -> UiNode {
    UiNode::Text {
        text: text.into(),
        tone: Tone::Neutral,
    }
}

pub fn markdown(source: impl Into<String>) -> UiNode {
    UiNode::Markdown {
        source: source.into(),
    }
}

pub fn notice(tone: Tone, text: impl Into<String>) -> UiNode {
    UiNode::Notice {
        tone,
        text: text.into(),
    }
}

/// `src` is a path inside the plugin's package.
pub fn image(src: impl Into<String>, alt: impl Into<String>) -> UiNode {
    UiNode::Image {
        src: src.into(),
        alt: alt.into(),
    }
}

/// `src` is an `.html` file under the plugin's `assets/`; `height` is in pixels.
pub fn webview(src: impl Into<String>, height: u32) -> UiNode {
    UiNode::Webview {
        src: src.into(),
        height,
    }
}

pub fn row<S: Into<String>>(cells: impl IntoIterator<Item = S>) -> TableRow {
    TableRow {
        cells: cells.into_iter().map(Into::into).collect(),
        payload: Value::Null,
    }
}

pub fn table<S: Into<String>>(
    columns: impl IntoIterator<Item = S>,
    rows: impl IntoIterator<Item = TableRow>,
) -> UiNode {
    UiNode::Table {
        columns: columns.into_iter().map(Into::into).collect(),
        rows: rows.into_iter().collect(),
        row_action: None,
    }
}

pub fn detail<L: Into<String>, V: Into<String>>(pairs: impl IntoIterator<Item = (L, V)>) -> UiNode {
    UiNode::Detail {
        pairs: pairs
            .into_iter()
            .map(|(label, value)| Pair {
                label: label.into(),
                value: value.into(),
            })
            .collect(),
    }
}

pub fn button(label: impl Into<String>, on_click: impl Serialize) -> UiNode {
    UiNode::Button {
        label: label.into(),
        action: action(on_click),
        variant: Variant::Primary,
    }
}

pub fn form(
    id: impl Into<String>,
    fields: impl IntoIterator<Item = FormField>,
    submit_label: impl Into<String>,
    on_submit: impl Serialize,
) -> UiNode {
    UiNode::Form {
        id: id.into(),
        fields: fields.into_iter().collect(),
        submit: Submit {
            label: submit_label.into(),
            action: action(on_submit),
        },
    }
}

/// `fraction` is 0 to 1.
pub fn progress(fraction: f32) -> UiNode {
    UiNode::Progress {
        fraction: Some(fraction),
        label: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    #[serde(tag = "id", content = "payload")]
    enum Act {
        Create,
    }

    #[test]
    fn an_action_takes_its_id_and_payload_from_the_enum() {
        let created = action(Act::Create);
        assert_eq!(
            (created.id.as_str(), &created.payload),
            ("Create", &Value::Null)
        );
        assert!(action("plain").id.starts_with("invalid action"));
    }

    #[test]
    fn a_built_page_round_trips_through_the_wire_form() {
        let page = stack([
            heading("Backups", 1),
            table(["World"], [row(["smp"])]),
            form(
                "new",
                [FormField::Text {
                    name: "name".into(),
                    label: "Name".into(),
                    value: String::new(),
                }],
                "Create",
                Act::Create,
            ),
        ]);
        let wire = serde_json::to_string(&page).unwrap();
        assert_eq!(serde_json::from_str::<UiNode>(&wire).unwrap(), page);
    }
}
