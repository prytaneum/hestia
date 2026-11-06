//! Maps a plugin's declarative [`UiNode`] tree onto the CLI's [`View`]s, and
//! lists the actions a tree offers. The mapping is total: every node becomes
//! views, never a panic.

use client::proto::ui::{Action, FormField, ListItem, Tone, UiNode};

use super::{markdown, View};

const BAR_WIDTH: usize = 20;

/// Something the user can trigger on a page: a button, a list item's button, a
/// chosen table row, or a form's submit. A form carries its `fields`.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub label: String,
    pub action: Action,
    pub fields: Vec<FormField>,
}

pub fn views(node: &UiNode) -> Vec<View> {
    let mut out = Vec::new();
    push_views(node, &mut out);
    out
}

pub fn choices(node: &UiNode) -> Vec<Choice> {
    let mut out = Vec::new();
    push_choices(node, &mut out);
    out
}

fn push_views(node: &UiNode, out: &mut Vec<View>) {
    match node {
        UiNode::Stack { children, .. } => children.iter().for_each(|c| push_views(c, out)),
        UiNode::Heading { text, .. } => out.push(View::line(text)),
        UiNode::Text { text, tone } => out.push(match tone {
            Tone::Muted => View::note(text),
            Tone::Warning | Tone::Error => View::warning(text),
            _ => View::line(text),
        }),
        UiNode::Markdown { source } => {
            let lines: Vec<String> = markdown::render(source)
                .iter()
                .map(ToString::to_string)
                .collect();
            out.push(View::line(lines.join("\n")));
        }
        UiNode::Image { alt, .. } => out.push(View::note(alt)),
        UiNode::Table { columns, rows, .. } => out.push(View::table(
            "",
            columns,
            rows.iter().map(|row| row.cells.clone()).collect(),
        )),
        UiNode::List { items } => out.push(View::table(
            "",
            ["ITEM", "DETAILS"],
            items
                .iter()
                .map(|item| vec![item.title.clone(), item.subtitle.clone().unwrap_or_default()])
                .collect(),
        )),
        UiNode::Detail { pairs } => out.push(View::detail(
            pairs.iter().map(|p| (p.label.clone(), p.value.clone())),
        )),
        UiNode::Form { fields, .. } => out.push(View::detail(fields.iter().map(field_summary))),
        UiNode::Button { label, .. } => out.push(View::note(format!("[ {label} ]"))),
        UiNode::Notice { tone, text } => out.push(match tone {
            Tone::Warning | Tone::Error => View::warning(text),
            _ => View::note(text),
        }),
        UiNode::Progress { fraction, label } => out.push(View::line(bar(*fraction, label))),
    }
}

fn field_summary(field: &FormField) -> (String, String) {
    match field {
        FormField::Text { label, value, .. } | FormField::Select { label, value, .. } => {
            (label.clone(), value.clone())
        }
        FormField::Number { label, value, .. } => (label.clone(), value.to_string()),
        FormField::Toggle { label, value, .. } => (label.clone(), value.to_string()),
        FormField::Entry { label, kind, .. } => (label.clone(), format!("<{kind}>")),
    }
}

fn bar(fraction: Option<f32>, label: &Option<String>) -> String {
    let label = label.as_deref().unwrap_or_default();
    let Some(fraction) = fraction.filter(|f| f.is_finite()) else {
        return format!("working... {label}").trim_end().to_string();
    };
    let fraction = fraction.clamp(0.0, 1.0);
    let filled = (fraction * BAR_WIDTH as f32).round() as usize;
    format!(
        "{}{} {:.0}% {label}",
        "█".repeat(filled),
        "░".repeat(BAR_WIDTH - filled),
        fraction * 100.0
    )
    .trim_end()
    .to_string()
}

fn push_choices(node: &UiNode, out: &mut Vec<Choice>) {
    match node {
        UiNode::Stack { children, .. } => children.iter().for_each(|c| push_choices(c, out)),
        UiNode::Button { label, action, .. } => out.push(Choice {
            label: label.clone(),
            action: action.clone(),
            fields: Vec::new(),
        }),
        UiNode::Form { fields, submit, .. } => out.push(Choice {
            label: submit.label.clone(),
            action: submit.action.clone(),
            fields: fields.clone(),
        }),
        UiNode::Table {
            rows,
            row_action: Some(id),
            ..
        } => out.extend(rows.iter().map(|row| Choice {
            label: row.cells.join(" | "),
            action: Action {
                id: id.clone(),
                payload: row.payload.clone(),
            },
            fields: Vec::new(),
        })),
        UiNode::List { items } => items.iter().for_each(|item| push_item_choices(item, out)),
        _ => {}
    }
}

fn push_item_choices(item: &ListItem, out: &mut Vec<Choice>) {
    for action in &item.actions {
        let before = out.len();
        push_choices(action, out);
        for choice in &mut out[before..] {
            choice.label = format!("{}: {}", item.title, choice.label);
        }
    }
}

#[cfg(test)]
mod tests {
    use client::proto::error::EntryKind;
    use client::proto::ui::{Direction, Pair, SelectOption, Submit, TableRow, Variant};
    use serde_json::json;

    use super::*;

    fn text(text: &str, tone: Tone) -> UiNode {
        UiNode::Text {
            text: text.into(),
            tone,
        }
    }

    fn button(label: &str, id: &str) -> UiNode {
        UiNode::Button {
            label: label.into(),
            action: Action {
                id: id.into(),
                payload: json!(null),
            },
            variant: Variant::Primary,
        }
    }

    #[test]
    fn text_and_heading_map_to_lines_and_notes() {
        let tree = UiNode::Stack {
            direction: Direction::Vertical,
            children: vec![
                UiNode::Heading {
                    text: "Title".into(),
                    level: 1,
                },
                text("plain", Tone::Neutral),
                text("quiet", Tone::Muted),
                text("bad", Tone::Error),
            ],
        };
        let views = views(&tree);
        assert!(matches!(&views[0], View::Line(t) if t == "Title"));
        assert!(matches!(&views[1], View::Line(t) if t == "plain"));
        assert!(matches!(&views[2], View::Note(t) if t == "quiet"));
        assert!(matches!(&views[3], View::Warning(t) if t == "bad"));
    }

    #[test]
    fn tables_lists_and_details_map_to_their_views() {
        let table = UiNode::Table {
            columns: vec!["A".into()],
            rows: vec![TableRow {
                cells: vec!["1".into()],
                payload: json!(7),
            }],
            row_action: Some("pick".into()),
        };
        assert!(matches!(&views(&table)[0], View::Table { headers, rows, .. }
            if headers == &["A"] && rows == &[vec!["1".to_string()]]));
        let list = UiNode::List {
            items: vec![ListItem {
                title: "t".into(),
                subtitle: Some("s".into()),
                actions: vec![button("Go", "go")],
                ..ListItem::default()
            }],
        };
        assert!(matches!(&views(&list)[0], View::Table { rows, .. }
            if rows == &[vec!["t".to_string(), "s".to_string()]]));
        let detail = UiNode::Detail {
            pairs: vec![Pair {
                label: "k".into(),
                value: "v".into(),
            }],
        };
        assert!(matches!(&views(&detail)[0], View::Detail(rows)
            if rows == &[("k".to_string(), "v".to_string())]));
    }

    #[test]
    fn markdown_image_notice_and_button_map_to_text() {
        let md = UiNode::Markdown {
            source: "# Hi\n\nbody".into(),
        };
        assert!(matches!(&views(&md)[0], View::Line(t) if t.contains("Hi") && t.contains("body")));
        let image = UiNode::Image {
            src: "a.png".into(),
            alt: "a picture".into(),
        };
        assert!(matches!(&views(&image)[0], View::Note(t) if t == "a picture"));
        let notice = UiNode::Notice {
            tone: Tone::Warning,
            text: "careful".into(),
        };
        assert!(matches!(&views(&notice)[0], View::Warning(t) if t == "careful"));
        let info = UiNode::Notice {
            tone: Tone::Info,
            text: "fyi".into(),
        };
        assert!(matches!(&views(&info)[0], View::Note(t) if t == "fyi"));
        assert!(matches!(&views(&button("Go", "go"))[0], View::Note(t) if t == "[ Go ]"));
    }

    #[test]
    fn progress_clamps_and_survives_nonsense() {
        let half = UiNode::Progress {
            fraction: Some(0.5),
            label: Some("half".into()),
        };
        assert!(
            matches!(&views(&half)[0], View::Line(t) if t.contains("50%") && t.ends_with("half"))
        );
        for fraction in [Some(f32::NAN), Some(9.0), Some(-1.0), None] {
            let node = UiNode::Progress {
                fraction,
                label: None,
            };
            assert_eq!(views(&node).len(), 1);
        }
        let over = UiNode::Progress {
            fraction: Some(9.0),
            label: None,
        };
        assert!(matches!(&views(&over)[0], View::Line(t) if t.contains("100%")));
    }

    #[test]
    fn an_empty_stack_is_no_views() {
        let tree = UiNode::Stack {
            direction: Direction::Horizontal,
            children: vec![],
        };
        assert!(views(&tree).is_empty());
        assert!(choices(&tree).is_empty());
    }

    #[test]
    fn a_form_shows_its_fields_and_offers_its_submit() {
        let form = UiNode::Form {
            id: "f".into(),
            fields: vec![
                FormField::Select {
                    name: "s".into(),
                    label: "Pick".into(),
                    options: vec![SelectOption {
                        value: "a".into(),
                        label: "A".into(),
                    }],
                    value: "a".into(),
                },
                FormField::Entry {
                    name: "e".into(),
                    label: "Server".into(),
                    kind: EntryKind::Server,
                },
            ],
            submit: Submit {
                label: "Save".into(),
                action: Action {
                    id: "save".into(),
                    payload: json!(null),
                },
            },
        };
        assert!(matches!(&views(&form)[0], View::Detail(rows) if rows.len() == 2));
        let found = choices(&form);
        assert_eq!(found[0].label, "Save");
        assert_eq!(found[0].action.id, "save");
        assert_eq!(found[0].fields.len(), 2);
    }

    #[test]
    fn buttons_rows_and_item_buttons_are_choices() {
        let tree = UiNode::Stack {
            direction: Direction::Vertical,
            children: vec![
                button("Refresh", "refresh"),
                UiNode::Table {
                    columns: vec!["A".into()],
                    rows: vec![TableRow {
                        cells: vec!["x".into(), "y".into()],
                        payload: json!({"n": 1}),
                    }],
                    row_action: Some("open".into()),
                },
                UiNode::List {
                    items: vec![ListItem {
                        title: "Item".into(),
                        actions: vec![button("Del", "del")],
                        ..ListItem::default()
                    }],
                },
            ],
        };
        let found = choices(&tree);
        let labels: Vec<&str> = found.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Refresh", "x | y", "Item: Del"]);
        assert_eq!(found[1].action.payload, json!({"n": 1}));
    }
}
