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
    /// A page of the plugin's own, drawn in a sandboxed frame with no access to
    /// the app. It talks to the host only through the frame's message bridge.
    Webview {
        /// An `.html` file under the plugin's `assets/`.
        src: String,
        /// In pixels, 1 to [`MAX_WEBVIEW_HEIGHT`].
        height: u32,
    },
}

/// The tallest frame a plugin may ask for.
pub const MAX_WEBVIEW_HEIGHT: u32 = 2000;

/// Why a tree or an effect is refused before a front-end draws it.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum UiError {
    #[error("webview src {0:?} must be a relative .html path under assets/")]
    WebviewSrc(String),
    #[error("webview height {0} must be 1 to {MAX_WEBVIEW_HEIGHT}")]
    WebviewHeight(u32),
}

impl UiNode {
    /// Checks every node of the tree; a plugin's output is untrusted.
    pub fn validate(&self) -> Result<(), UiError> {
        match self {
            UiNode::Stack { children, .. } => children.iter().try_for_each(UiNode::validate),
            UiNode::List { items } => items
                .iter()
                .flat_map(|item| &item.actions)
                .try_for_each(UiNode::validate),
            UiNode::Webview { src, height } => {
                let under_assets = src
                    .strip_prefix("assets/")
                    .is_some_and(crate::plugin::stays_inside);
                if !under_assets || !src.ends_with(".html") {
                    return Err(UiError::WebviewSrc(src.clone()));
                }
                if !(1..=MAX_WEBVIEW_HEIGHT).contains(height) {
                    return Err(UiError::WebviewHeight(*height));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Effect {
    pub fn validate(&self) -> Result<(), UiError> {
        match self {
            Effect::Render { tree } => tree.validate(),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn webview(src: &str, height: u32) -> UiNode {
        UiNode::Webview {
            src: src.into(),
            height,
        }
    }

    #[test]
    fn a_webview_under_assets_is_accepted() {
        assert_eq!(webview("assets/chart/index.html", 300).validate(), Ok(()));
    }

    #[test]
    fn a_webview_src_that_escapes_or_is_not_html_is_refused() {
        for src in [
            "",
            "assets/",
            "assets/../plugin.toml",
            "assets/a/../../x.html",
            "/assets/a.html",
            "themes/a.html",
            "assets/a.png",
            "assets\\a.html",
            "https://example.com/a.html",
            "assets//a.html",
        ] {
            assert!(webview(src, 300).validate().is_err(), "{src}");
        }
    }

    #[test]
    fn a_webview_height_is_bounded() {
        assert_eq!(
            webview("assets/a.html", 0).validate(),
            Err(UiError::WebviewHeight(0))
        );
        assert!(webview("assets/a.html", MAX_WEBVIEW_HEIGHT + 1)
            .validate()
            .is_err());
    }

    #[test]
    fn a_nested_webview_is_checked() {
        let tree = UiNode::Stack {
            direction: Direction::Vertical,
            children: vec![webview("../x.html", 10)],
        };
        assert!(tree.validate().is_err());
        assert!(Effect::Render { tree }.validate().is_err());
    }
}
