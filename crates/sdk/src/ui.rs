//! Builders for the trees a page returns from [`Plugin::render`](crate::Plugin::render).
//! They produce the same [`UiNode`] values the front-ends draw, so anything
//! the builders do not cover can be written as the enum directly.
//!
//! ```
//! use hestia::ui;
//!
//! let page = ui::stack([
//!     ui::heading("Backups", 1),
//!     ui::table(["World", "Size"], [ui::row(["smp", "1.2 GB"])]),
//!     ui::button("Back up now", "backup"),
//! ]);
//! ```

use proto::ui::{Direction, Pair, Submit, TableRow, Variant};
use serde_json::Value;

pub use proto::ui::{Action, Effect, FormField, Tone, UiNode};

/// A click or submit that carries no payload.
pub fn action(id: impl Into<String>) -> Action {
    Action {
        id: id.into(),
        payload: Value::Null,
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

pub fn button(label: impl Into<String>, action_id: impl Into<String>) -> UiNode {
    UiNode::Button {
        label: label.into(),
        action: action(action_id),
        variant: Variant::Primary,
    }
}

pub fn form(
    id: impl Into<String>,
    fields: impl IntoIterator<Item = FormField>,
    submit_label: impl Into<String>,
    action_id: impl Into<String>,
) -> UiNode {
    UiNode::Form {
        id: id.into(),
        fields: fields.into_iter().collect(),
        submit: Submit {
            label: submit_label.into(),
            action: action(action_id),
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
                "create",
            ),
        ]);
        let wire = serde_json::to_string(&page).unwrap();
        assert_eq!(serde_json::from_str::<UiNode>(&wire).unwrap(), page);
    }
}
