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

/// How a remote image is drawn.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ImageShape {
    #[default]
    Natural,
    /// A small round picture, like a player head.
    Avatar,
}

/// One page of a [`UiNode::Tabs`]: a label and the nodes shown while it is chosen.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Tab {
    /// Unique among the tabs of one node; the front-end keeps the chosen id.
    pub id: String,
    pub label: String,
    pub children: Vec<UiNode>,
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
    /// Starts one of the plugin's own commands as a job.
    Run {
        command: String,
        #[serde(default)]
        #[cfg_attr(feature = "ts", ts(type = "unknown"))]
        args: Value,
    },
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
    pub badges: Vec<Badge>,
    /// Buttons and copy values shown at the item's end.
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

/// The kind of value a form field takes, and what bounds it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Input {
    Text {
        value: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Number {
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        step: Option<f64>,
    },
    Toggle {
        value: bool,
    },
    Select {
        options: Vec<SelectOption>,
        value: String,
    },
    /// Picks one existing server or instance; submits its id.
    Entry {
        kind: EntryKind,
    },
}

/// One input of a form. Its `name` keys the submitted values.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    pub name: String,
    pub label: String,
    /// A hint shown under the field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(flatten)]
    pub input: Input,
}

/// A short status pill on a list item.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct Badge {
    pub text: String,
    pub tone: Tone,
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
    /// A titled group; `actions` sit beside the title.
    Section {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default)]
        actions: Vec<UiNode>,
        children: Vec<UiNode>,
    },
    /// A value with a copy button; the CLI prints the value.
    Copy {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
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
    /// An image from a host the plugin's `network:connect` grant names. The
    /// daemon fetches it, so the webview never contacts the host.
    RemoteImage {
        /// An `https` URL.
        url: String,
        alt: String,
        #[serde(default)]
        shape: ImageShape,
    },
    Badge {
        text: String,
        #[serde(default)]
        tone: Tone,
    },
    Tabs {
        tabs: Vec<Tab>,
    },
    Table {
        columns: Vec<String>,
        rows: Vec<TableRow>,
        /// Shows a search box that hides the rows whose cells do not contain
        /// what was typed.
        #[serde(default)]
        filter: bool,
        /// The action id sent, with the row's payload, when a row is chosen.
        #[serde(skip_serializing_if = "Option::is_none")]
        row_action: Option<String>,
        /// Shown instead of the rows when there are none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        empty: Option<String>,
    },
    List {
        items: Vec<ListItem>,
        /// Shown instead of the items when there are none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        empty: Option<String>,
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
        /// Asks the user to confirm with this text before the action is sent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confirm: Option<String>,
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

/// The longest URL a remote image may name.
pub const MAX_IMAGE_URL: usize = 2048;

/// The most tabs one node may hold.
pub const MAX_TABS: usize = 12;

/// Why a tree or an effect is refused before a front-end draws it.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum UiError {
    #[error("webview src {0:?} must be a relative .html path under assets/")]
    WebviewSrc(String),
    #[error("webview height {0} must be 1 to {MAX_WEBVIEW_HEIGHT}")]
    WebviewHeight(u32),
    #[error("remote image url {0:?} must be an https URL of at most {MAX_IMAGE_URL} bytes")]
    ImageUrl(String),
    #[error("tabs need 1 to {MAX_TABS} tabs, each with its own non-empty id")]
    Tabs,
    #[error("copy text must not be empty")]
    CopyText,
}

impl UiNode {
    /// Checks every node of the tree; a plugin's output is untrusted.
    pub fn validate(&self) -> Result<(), UiError> {
        match self {
            UiNode::Stack { children, .. } => children.iter().try_for_each(UiNode::validate),
            UiNode::Section {
                actions, children, ..
            } => actions
                .iter()
                .chain(children)
                .try_for_each(UiNode::validate),
            UiNode::Copy { text, .. } if text.is_empty() => Err(UiError::CopyText),
            UiNode::List { items, .. } => items
                .iter()
                .flat_map(|item| &item.actions)
                .try_for_each(UiNode::validate),
            UiNode::RemoteImage { url, .. } => {
                let https = url
                    .get(..8)
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
                if !https || url.len() > MAX_IMAGE_URL || url.len() == 8 {
                    return Err(UiError::ImageUrl(url.clone()));
                }
                Ok(())
            }
            UiNode::Tabs { tabs } => {
                let mut seen = std::collections::HashSet::new();
                let distinct = tabs
                    .iter()
                    .all(|tab| !tab.id.is_empty() && seen.insert(tab.id.as_str()));
                if tabs.is_empty() || tabs.len() > MAX_TABS || !distinct {
                    return Err(UiError::Tabs);
                }
                tabs.iter()
                    .flat_map(|tab| &tab.children)
                    .try_for_each(UiNode::validate)
            }
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
    fn a_webview_src_that_escapes_or_is_not_html_is_refused() {
        assert_eq!(webview("assets/chart/index.html", 300).validate(), Ok(()));
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
    fn a_remote_image_must_name_an_https_url() {
        let image = |url: &str| UiNode::RemoteImage {
            url: url.into(),
            alt: String::new(),
            shape: ImageShape::Avatar,
        };
        assert_eq!(image("https://example.com/a.png").validate(), Ok(()));
        assert_eq!(image("HTTPS://example.com/a.png").validate(), Ok(()));
        for url in [
            "",
            "https://",
            "http://example.com/a.png",
            "data:image/png;base64,AA",
            "assets/a.png",
        ] {
            assert!(image(url).validate().is_err(), "{url}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_IMAGE_URL));
        assert!(image(&long).validate().is_err());
    }

    #[test]
    fn tabs_need_distinct_ids_and_validate_their_children() {
        let tab = |id: &str, children| Tab {
            id: id.into(),
            label: id.into(),
            children,
        };
        let ok = UiNode::Tabs {
            tabs: vec![tab("a", vec![]), tab("b", vec![])],
        };
        assert_eq!(ok.validate(), Ok(()));
        for tabs in [
            vec![],
            vec![tab("a", vec![]), tab("a", vec![])],
            vec![tab("", vec![])],
            vec![tab("a", vec![webview("../x.html", 10)])],
            (0..=MAX_TABS)
                .map(|i| tab(&i.to_string(), vec![]))
                .collect(),
        ] {
            assert!(UiNode::Tabs { tabs }.validate().is_err());
        }
    }

    #[test]
    fn a_nested_node_is_checked() {
        let bad = || webview("../x.html", 10);
        let remote = UiNode::RemoteImage {
            url: "http://example.com/a.png".into(),
            alt: String::new(),
            shape: ImageShape::Natural,
        };
        let section = |actions, children| UiNode::Section {
            title: "t".into(),
            description: None,
            actions,
            children,
        };
        let trees = [
            UiNode::Stack {
                direction: Direction::Vertical,
                children: vec![bad()],
            },
            section(vec![], vec![bad()]),
            section(vec![], vec![remote.clone()]),
            section(vec![bad()], vec![]),
            section(vec![remote], vec![]),
            section(
                vec![],
                vec![UiNode::Copy {
                    text: String::new(),
                    label: None,
                }],
            ),
        ];
        for tree in trees {
            assert!(tree.validate().is_err());
            assert!(Effect::Render { tree }.validate().is_err());
        }
    }
}
