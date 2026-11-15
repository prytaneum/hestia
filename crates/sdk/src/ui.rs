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

use proto::ui::{Badge, Direction, ImageShape, Pair, SelectOption, Submit, Tab, TableRow, Variant};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use proto::error::EntryKind;
pub use proto::ui::{Action, Effect, FormField, Input, ListItem, Tone, UiNode};

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

/// A picture from `url`, an `https` address on a host the plugin's
/// `network:connect` grant names. The daemon fetches it; a host the grant does
/// not name shows no picture.
pub fn remote_image(url: impl Into<String>, alt: impl Into<String>) -> UiNode {
    UiNode::RemoteImage {
        url: url.into(),
        alt: alt.into(),
        shape: ImageShape::Natural,
    }
}

/// A small round picture, like a player head; the same fetch rules as
/// [`remote_image`].
pub fn avatar(url: impl Into<String>, alt: impl Into<String>) -> UiNode {
    UiNode::RemoteImage {
        url: url.into(),
        alt: alt.into(),
        shape: ImageShape::Avatar,
    }
}

pub fn badge(tone: Tone, text: impl Into<String>) -> UiNode {
    UiNode::Badge {
        text: text.into(),
        tone,
    }
}

/// One tab of [`tabs`]; `id` is unique among them.
pub fn tab(
    id: impl Into<String>,
    label: impl Into<String>,
    children: impl IntoIterator<Item = UiNode>,
) -> Tab {
    Tab {
        id: id.into(),
        label: label.into(),
        children: children.into_iter().collect(),
    }
}

pub fn tabs(tabs: impl IntoIterator<Item = Tab>) -> UiNode {
    UiNode::Tabs {
        tabs: tabs.into_iter().collect(),
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
    Table::new(columns, rows).into()
}

/// A [`table`] with a search box above it that hides the rows whose cells do
/// not contain what was typed.
pub fn filtered_table<S: Into<String>>(
    columns: impl IntoIterator<Item = S>,
    rows: impl IntoIterator<Item = TableRow>,
) -> UiNode {
    Table::new(columns, rows).filter().into()
}

/// A table; converts into a [`UiNode`].
///
/// ```
/// use hestia::ui;
///
/// let node: ui::UiNode = ui::Table::new(["World"], [])
///     .filter()
///     .row_action("open")
///     .empty("No worlds yet.")
///     .into();
/// assert!(matches!(node, ui::UiNode::Table { filter: true, empty: Some(_), .. }));
/// ```
pub struct Table(UiNode);

impl Table {
    pub fn new<S: Into<String>>(
        columns: impl IntoIterator<Item = S>,
        rows: impl IntoIterator<Item = TableRow>,
    ) -> Self {
        Table(UiNode::Table {
            columns: columns.into_iter().map(Into::into).collect(),
            rows: rows.into_iter().collect(),
            filter: false,
            row_action: None,
            empty: None,
        })
    }

    /// A search box above the table that hides the rows whose cells do not
    /// contain what was typed.
    pub fn filter(mut self) -> Self {
        if let UiNode::Table { filter, .. } = &mut self.0 {
            *filter = true;
        }
        self
    }

    /// The action id sent, with the row's payload, when a row is chosen.
    pub fn row_action(mut self, id: impl Into<String>) -> Self {
        if let UiNode::Table { row_action, .. } = &mut self.0 {
            *row_action = Some(id.into());
        }
        self
    }

    /// Shown instead of the rows when there are none.
    pub fn empty(mut self, text: impl Into<String>) -> Self {
        if let UiNode::Table { empty, .. } = &mut self.0 {
            *empty = Some(text.into());
        }
        self
    }
}

impl From<Table> for UiNode {
    fn from(table: Table) -> UiNode {
        table.0
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

/// A [`Button`] in its default look.
pub fn button(label: impl Into<String>, on_click: impl Serialize) -> UiNode {
    Button::new(label, on_click).into()
}

/// A [`button`] that asks `prompt` before its action is sent, for what cannot
/// be undone.
pub fn button_confirming(
    label: impl Into<String>,
    on_click: impl Serialize,
    prompt: impl Into<String>,
) -> UiNode {
    Button::new(label, on_click).danger().confirm(prompt).into()
}

/// A button; converts into a [`UiNode`]. Primary until told otherwise.
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
/// let node: ui::UiNode = ui::Button::new("Remove", Act::Remove("smp".into()))
///     .danger()
///     .confirm("Remove smp?")
///     .into();
/// assert!(matches!(node, ui::UiNode::Button { confirm: Some(_), .. }));
/// ```
pub struct Button(UiNode);

impl Button {
    pub fn new(label: impl Into<String>, on_click: impl Serialize) -> Self {
        Button(UiNode::Button {
            label: label.into(),
            action: action(on_click),
            variant: Variant::Primary,
            confirm: None,
        })
    }

    /// The quieter look, for a button beside a primary one.
    pub fn secondary(self) -> Self {
        self.variant(Variant::Secondary)
    }

    /// The look for what destroys something.
    pub fn danger(self) -> Self {
        self.variant(Variant::Danger)
    }

    /// Asks `prompt` before the action is sent.
    pub fn confirm(mut self, prompt: impl Into<String>) -> Self {
        if let UiNode::Button { confirm, .. } = &mut self.0 {
            *confirm = Some(prompt.into());
        }
        self
    }

    fn variant(mut self, look: Variant) -> Self {
        if let UiNode::Button { variant, .. } = &mut self.0 {
            *variant = look;
        }
        self
    }
}

impl From<Button> for UiNode {
    fn from(button: Button) -> UiNode {
        button.0
    }
}

/// A value with a copy button. The CLI prints the value.
pub fn copy(text: impl Into<String>) -> UiNode {
    UiNode::Copy {
        text: text.into(),
        label: None,
    }
}

/// A [`copy`] value with a label before it.
pub fn copy_labelled(label: impl Into<String>, text: impl Into<String>) -> UiNode {
    UiNode::Copy {
        text: text.into(),
        label: Some(label.into()),
    }
}

/// A titled group; converts into a [`UiNode`]. Header actions sit beside the
/// title.
///
/// ```
/// use hestia::ui;
///
/// #[derive(serde::Serialize)]
/// #[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
/// enum Act {
///     Add,
/// }
///
/// let items = [ui::Item::new("voice").badge(ui::Tone::Success, "Online")];
/// let section: ui::UiNode = ui::Section::new("Tunnels")
///     .description("Up whenever the server runs.")
///     .action(ui::Button::new("Add", Act::Add).secondary())
///     .children([ui::List::new(items).empty("No tunnels yet.").into()])
///     .into();
///
/// let wire = serde_json::to_string(&section).unwrap();
/// assert_eq!(serde_json::from_str::<ui::UiNode>(&wire).unwrap(), section);
/// ```
pub struct Section {
    title: String,
    description: Option<String>,
    actions: Vec<UiNode>,
    children: Vec<UiNode>,
}

impl Section {
    pub fn new(title: impl Into<String>) -> Self {
        Section {
            title: title.into(),
            description: None,
            actions: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a header action: a button or a copy value.
    pub fn action(mut self, action: impl Into<UiNode>) -> Self {
        self.actions.push(action.into());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = UiNode>) -> Self {
        self.children.extend(children);
        self
    }
}

impl From<Section> for UiNode {
    fn from(section: Section) -> UiNode {
        UiNode::Section {
            title: section.title,
            description: section.description,
            actions: section.actions,
            children: section.children,
        }
    }
}

/// One row of a [`List`]; converts into a [`ListItem`].
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
/// let remove = ui::Button::new("Remove", Act::Remove("voice".into()))
///     .danger()
///     .confirm("Remove voice?");
/// let item: ui::ListItem = ui::Item::new("voice")
///     .subtitle("UDP · local 24454")
///     .badge(ui::Tone::Success, "Online")
///     .action(ui::copy("abc.gl.joinmc.link:12345"))
///     .action(remove)
///     .into();
/// assert_eq!((item.badges.len(), item.actions.len()), (1, 2));
/// ```
pub struct Item(ListItem);

impl Item {
    pub fn new(title: impl Into<String>) -> Self {
        Item(ListItem {
            title: title.into(),
            ..ListItem::default()
        })
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.0.subtitle = Some(subtitle.into());
        self
    }

    /// A name from the app's icon set.
    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.0.icon = Some(icon.into());
        self
    }

    pub fn badge(mut self, tone: Tone, text: impl Into<String>) -> Self {
        self.0.badges.push(Badge {
            text: text.into(),
            tone,
        });
        self
    }

    /// Adds an action at the item's end: a button or a copy value.
    pub fn action(mut self, action: impl Into<UiNode>) -> Self {
        self.0.actions.push(action.into());
        self
    }
}

impl From<Item> for ListItem {
    fn from(item: Item) -> ListItem {
        item.0
    }
}

/// A list of [`Item`]s; converts into a [`UiNode`].
pub struct List {
    items: Vec<ListItem>,
    empty: Option<String>,
}

impl List {
    pub fn new<I: Into<ListItem>>(items: impl IntoIterator<Item = I>) -> Self {
        List {
            items: items.into_iter().map(Into::into).collect(),
            empty: None,
        }
    }

    /// Shown instead of the items when there are none.
    pub fn empty(mut self, text: impl Into<String>) -> Self {
        self.empty = Some(text.into());
        self
    }
}

impl From<List> for UiNode {
    fn from(list: List) -> UiNode {
        UiNode::List {
            items: list.items,
            empty: list.empty,
        }
    }
}

/// One input of a [`form`]; converts into a [`FormField`]. `placeholder`
/// applies to a text field, `min`, `max` and `step` to a number field, and are
/// ignored on any other.
///
/// ```
/// use hestia::ui;
///
/// let port: ui::FormField = ui::Field::number("port", "Local port", 24454.0)
///     .min(1.0)
///     .max(65535.0)
///     .help("The port the server listens on.")
///     .into();
/// let name: ui::FormField = ui::Field::text("name", "Name", "").placeholder("voice").into();
/// assert_eq!(port.help.as_deref(), Some("The port the server listens on."));
/// assert!(matches!(name.input, ui::Input::Text { placeholder: Some(_), .. }));
/// ```
pub struct Field(FormField);

impl Field {
    fn of(name: impl Into<String>, label: impl Into<String>, input: Input) -> Self {
        Field(FormField {
            name: name.into(),
            label: label.into(),
            help: None,
            input,
        })
    }

    pub fn text(
        name: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self::of(
            name,
            label,
            Input::Text {
                value: value.into(),
                placeholder: None,
            },
        )
    }

    pub fn number(name: impl Into<String>, label: impl Into<String>, value: f64) -> Self {
        Self::of(
            name,
            label,
            Input::Number {
                value,
                min: None,
                max: None,
                step: None,
            },
        )
    }

    pub fn toggle(name: impl Into<String>, label: impl Into<String>, value: bool) -> Self {
        Self::of(name, label, Input::Toggle { value })
    }

    /// `options` are `(value, label)` pairs; `value` is the selected one.
    pub fn select<V: Into<String>, L: Into<String>>(
        name: impl Into<String>,
        label: impl Into<String>,
        options: impl IntoIterator<Item = (V, L)>,
        value: impl Into<String>,
    ) -> Self {
        Self::of(
            name,
            label,
            Input::Select {
                options: options
                    .into_iter()
                    .map(|(value, label)| SelectOption {
                        value: value.into(),
                        label: label.into(),
                    })
                    .collect(),
                value: value.into(),
            },
        )
    }

    /// Picks one existing server or instance; submits its id.
    pub fn entry(name: impl Into<String>, label: impl Into<String>, kind: EntryKind) -> Self {
        Self::of(name, label, Input::Entry { kind })
    }

    /// A hint shown under the field.
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.0.help = Some(help.into());
        self
    }

    pub fn placeholder(mut self, hint: impl Into<String>) -> Self {
        if let Input::Text { placeholder, .. } = &mut self.0.input {
            *placeholder = Some(hint.into());
        }
        self
    }

    pub fn min(mut self, bound: f64) -> Self {
        if let Input::Number { min, .. } = &mut self.0.input {
            *min = Some(bound);
        }
        self
    }

    pub fn max(mut self, bound: f64) -> Self {
        if let Input::Number { max, .. } = &mut self.0.input {
            *max = Some(bound);
        }
        self
    }

    pub fn step(mut self, increment: f64) -> Self {
        if let Input::Number { step, .. } = &mut self.0.input {
            *step = Some(increment);
        }
        self
    }
}

impl From<Field> for FormField {
    fn from(field: Field) -> FormField {
        field.0
    }
}

pub fn form(
    id: impl Into<String>,
    fields: impl IntoIterator<Item = impl Into<FormField>>,
    submit_label: impl Into<String>,
    on_submit: impl Serialize,
) -> UiNode {
    UiNode::Form {
        id: id.into(),
        fields: fields.into_iter().map(Into::into).collect(),
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
}
