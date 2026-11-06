//! The plugin UI vocabulary: a closed set of declarative nodes a plugin returns
//! and every front-end renders with its own components. The desktop and the CLI
//! draw the same tree, so a plugin page is written once. A node is added only
//! when a real plugin needs it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::EntryKind;

/// Colour intent shared by text, notices and toasts.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Neutral,
    Muted,
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    #[default]
    Primary,
    Secondary,
    Danger,
}

/// What a click or a submit sends back to the plugin: the `id` it declared and
/// the `payload` it attached. A form adds its field values separately.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Action {
    pub id: String,
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub payload: Value,
}

/// What a plugin asks the host to do after an action.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Effect {
    /// Replace the page with a new tree.
    Render {
        tree: UiNode,
    },
    Toast {
        tone: Tone,
        text: String,
    },
    /// Open another page of the same plugin.
    Navigate {
        page: String,
    },
    /// Ask the plugin to render the current page again.
    Refresh,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct TableRow {
    pub cells: Vec<String>,
    /// Sent as the payload of the table's `rowAction` when the row is chosen.
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub payload: Value,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ListItem {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    /// A name from the app's icon set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Buttons shown on the item.
    pub actions: Vec<UiNode>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Pair {
    pub label: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

/// A form's submit button and the action it sends.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Submit {
    pub label: String,
    pub action: Action,
}

/// One input of a form. Its `name` keys the submitted values.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FormField {
    Text {
        name: String,
        label: String,
        value: String,
    },
    Number {
        name: String,
        label: String,
        value: f64,
    },
    Toggle {
        name: String,
        label: String,
        value: bool,
    },
    Select {
        name: String,
        label: String,
        options: Vec<SelectOption>,
        value: String,
    },
    /// Picks one existing server or instance; submits its id.
    Entry {
        name: String,
        label: String,
        kind: EntryKind,
    },
}

/// One node of a plugin page.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum UiNode {
    Stack {
        #[serde(default)]
        direction: Direction,
        children: Vec<UiNode>,
    },
    Heading {
        text: String,
        /// 1 to 3.
        level: u8,
    },
    Text {
        text: String,
        #[serde(default)]
        tone: Tone,
    },
    /// Front-ends render this without raw HTML; embedded markup is shown as text.
    Markdown {
        source: String,
    },
    Image {
        /// A path inside the plugin's own assets, relative to its root.
        src: String,
        alt: String,
    },
    Table {
        columns: Vec<String>,
        rows: Vec<TableRow>,
        /// The action id sent, with the row's payload, when a row is chosen.
        #[serde(skip_serializing_if = "Option::is_none")]
        row_action: Option<String>,
    },
    List {
        items: Vec<ListItem>,
    },
    Detail {
        pairs: Vec<Pair>,
    },
    Form {
        id: String,
        fields: Vec<FormField>,
        submit: Submit,
    },
    Button {
        label: String,
        action: Action,
        #[serde(default)]
        variant: Variant,
    },
    Notice {
        #[serde(default)]
        tone: Tone,
        text: String,
    },
    Progress {
        /// 0 to 1; absent when the length of the work is unknown.
        #[serde(skip_serializing_if = "Option::is_none")]
        fraction: Option<f32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}
