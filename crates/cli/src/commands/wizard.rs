//! The fullscreen create wizard shared by `server create` and
//! `instance create`: step pages under a breadcrumb — flavor → version →
//! name → settings → confirm — with Esc walking back, given arguments
//! prefilling (and skipping) steps, and provisioning progress rendered in the
//! same session, ending on the created entry's summary. It opens whenever a
//! create is interactive and flavor, version, name or (server) EULA is missing.

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::Result;
use client::proto::error::EntryKind;
use client::proto::instance::InstanceInfo;
use client::proto::keys::KeyKind;
use client::proto::minecraft::{
    ConfigEntry, Edition, EditionCatalogue, GameVersion, ProvisionProgress, VersionKind,
};
use client::proto::server::{ServerCreateParams, ServerDone};
use client::Client;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use ratatui::Frame;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::commands::mc::{edition_label, flavor_detail, flavor_label, kind_label};
use crate::ui::components::working::draw_working;
use crate::ui::components::{Picker, PickerItem, SelectList, TextInput};
use crate::ui::session::{self, Flow, Screen};

const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

/// How a settings field edits: free text, or cycling through fixed choices.
#[derive(Clone)]
pub enum FieldKind {
    Text,
    Number,
    Choice(Vec<String>),
}

/// The seed's slot for a server's listed name; the chosen edition's
/// `nameKey` replaces it, and it is dropped when the edition has none.
pub const NAME_FIELD: &str = "listed-name";

/// One settings row, prefilled from the command's flags. An empty value is
/// not sent — the entry keeps whatever its own default is; `hint` says where
/// that default comes from (dim, in place of a value). Defaults the game owns
/// are deliberately not spelled out here: they vary per version and only
/// exist once that version's server generates its own file.
#[derive(Clone)]
pub struct Field {
    pub key: Cow<'static, str>,
    pub label: &'static str,
    pub hint: &'static str,
    pub kind: FieldKind,
    pub value: String,
}

impl Field {
    pub fn text(
        key: &'static str,
        label: &'static str,
        hint: &'static str,
        value: Option<String>,
    ) -> Field {
        Field {
            key: key.into(),
            label,
            hint,
            kind: FieldKind::Text,
            value: value.unwrap_or_default(),
        }
    }

    pub fn number(
        key: &'static str,
        label: &'static str,
        hint: &'static str,
        value: Option<String>,
    ) -> Field {
        Field {
            key: key.into(),
            label,
            hint,
            kind: FieldKind::Number,
            value: value.unwrap_or_default(),
        }
    }

    pub fn choice(
        key: &'static str,
        label: &'static str,
        hint: &'static str,
        options: &[&str],
        value: Option<String>,
    ) -> Field {
        Field {
            key: key.into(),
            label,
            hint,
            kind: FieldKind::Choice(options.iter().map(|o| o.to_string()).collect()),
            value: value.unwrap_or_default(),
        }
    }
}

/// Everything the wizard starts from: the per-edition catalogues, the argument
/// prefills, and the settings schema for this entry kind. `edition` is
/// prefilled when the command named one; with several catalogues and none
/// named, the wizard asks.
pub struct WizardSeed {
    pub kind: EntryKind,
    pub catalogues: Vec<EditionCatalogue>,
    pub edition: Option<Edition>,
    pub flavor: Option<String>,
    pub version: Option<String>,
    pub name: Option<String>,
    pub loader: Option<String>,
    pub eula: bool,
    pub fields: Vec<Field>,
    /// `--prop KEY=VALUE` passthrough, applied after the settings fields.
    pub extra: Vec<ConfigEntry>,
}

pub enum WizardOutcome {
    /// The whole create result, warnings included — the wizard's caller reports
    /// them exactly as the flag path does.
    Server(Box<ServerDone>),
    Instance(Box<InstanceInfo>),
}

enum Request {
    Versions { edition: Edition, flavor: String },
    CreateServer(Box<ServerCreateParams>),
    CreateInstance(Box<InstanceCreate>),
}

struct InstanceCreate {
    name: String,
    flavor: String,
    version: String,
    loader: Option<String>,
    config: Vec<ConfigEntry>,
}

enum AppEvent {
    Versions {
        edition: Edition,
        flavor: String,
        versions: Vec<GameVersion>,
    },
    Progress(ProvisionProgress),
    ServerCreated(Box<ServerDone>),
    InstanceCreated(Box<InstanceInfo>),
    Failed {
        message: String,
    },
}

/// Run the create wizard; `None` when the user backed out.
pub async fn run(client: &Client, seed: WizardSeed) -> Result<Option<WizardOutcome>> {
    let kind = seed.kind;
    let (request_tx, request_rx) = unbounded_channel();
    let (event_tx, event_rx) = unbounded_channel();
    let screen = WizardScreen::new(seed, request_tx);
    let driver = drive(client, kind, request_rx, event_tx);
    let (outcome, ()) = tokio::join!(session::run_async(screen, Some(event_rx)), driver);
    outcome
}

async fn drive(
    client: &Client,
    kind: EntryKind,
    mut requests: UnboundedReceiver<Request>,
    events: UnboundedSender<AppEvent>,
) {
    while let Some(request) = requests.recv().await {
        let event = match request {
            Request::Versions { edition, flavor } => {
                match client.entry().versions(kind, edition, &flavor).await {
                    Ok(versions) => AppEvent::Versions {
                        edition,
                        flavor,
                        versions,
                    },
                    Err(e) => AppEvent::Failed {
                        message: e.to_string(),
                    },
                }
            }
            Request::CreateServer(params) => {
                let progress = events.clone();
                match client
                    .server()
                    .create(*params, move |p| {
                        let _ = progress.send(AppEvent::Progress(p.clone()));
                    })
                    .await
                {
                    Ok(info) => AppEvent::ServerCreated(Box::new(info)),
                    Err(e) => AppEvent::Failed {
                        message: e.to_string(),
                    },
                }
            }
            Request::CreateInstance(create) => {
                match client
                    .instance()
                    .create(
                        &create.name,
                        &create.flavor,
                        &create.version,
                        create.loader.clone(),
                        create.config.clone(),
                    )
                    .await
                {
                    Ok(info) => AppEvent::InstanceCreated(Box::new(info)),
                    Err(e) => AppEvent::Failed {
                        message: e.to_string(),
                    },
                }
            }
        };
        if events.send(event).is_err() {
            break;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Edition,
    Flavor,
    Version,
    Name,
    Settings,
    Confirm,
    Working,
}

const STEPS: [Step; 6] = [
    Step::Edition,
    Step::Flavor,
    Step::Version,
    Step::Name,
    Step::Settings,
    Step::Confirm,
];

fn step_label(step: Step) -> &'static str {
    match step {
        Step::Edition => "edition",
        Step::Flavor => "flavor",
        Step::Version => "version",
        Step::Name => "name",
        Step::Settings => "settings",
        Step::Confirm => "confirm",
        Step::Working => "",
    }
}

struct WizardScreen {
    seed: WizardSeed,
    requests: UnboundedSender<Request>,

    step: usize,
    first_step: usize,
    edition: Edition,
    edition_list: SelectList,
    flavor_list: SelectList,
    flavor: String,
    versions: HashMap<(&'static str, String), Vec<GameVersion>>,
    picker: Option<Picker>,
    version: String,
    name: TextInput,
    fields: Vec<Field>,
    settings_cursor: usize,
    editing: Option<TextInput>,
    eula: bool,
    status: Option<String>,
    progress: Option<ProvisionProgress>,
}

impl WizardScreen {
    fn new(seed: WizardSeed, requests: UnboundedSender<Request>) -> Self {
        let edition = seed
            .edition
            .or_else(|| seed.catalogues.first().map(|c| c.edition))
            .unwrap_or_default();
        let edition_labels = seed
            .catalogues
            .iter()
            .map(|c| edition_label(c.edition).to_string())
            .collect();
        let mut screen = WizardScreen {
            edition,
            edition_list: SelectList::new(edition_labels),
            flavor_list: SelectList::new(Vec::new()),
            flavor: seed.flavor.clone().unwrap_or_default(),
            version: seed.version.clone().unwrap_or_default(),
            name: TextInput::with_text(seed.name.as_deref().unwrap_or("")),
            fields: seed.fields.clone(),
            eula: seed.eula || seed.kind == EntryKind::Instance,
            seed,
            requests,
            step: 0,
            first_step: 0,
            versions: HashMap::new(),
            picker: None,
            settings_cursor: 0,
            editing: None,
            status: None,
            progress: None,
        };
        screen.flavor_list = screen.flavor_select();
        let mut first = 0;
        if screen.seed.edition.is_some() || !screen.asks_edition() {
            first = 1;
            if !screen.flavor.is_empty() {
                first = 2;
                if !screen.version.is_empty() {
                    first = 3;
                }
            }
        }
        screen.fields = screen.applicable_fields();
        screen.step = first;
        screen.first_step = first;
        screen.entered_step();
        screen
    }

    fn current(&self) -> Step {
        match self.step {
            i if i < STEPS.len() => STEPS[i],
            _ => Step::Working,
        }
    }

    fn asks_edition(&self) -> bool {
        self.seed.catalogues.len() > 1
    }

    fn catalogue(&self) -> Option<&EditionCatalogue> {
        self.seed
            .catalogues
            .iter()
            .find(|c| c.edition == self.edition)
    }

    fn flavor_select(&self) -> SelectList {
        let flavors = self
            .catalogue()
            .map(|c| c.flavors.as_slice())
            .unwrap_or(&[]);
        SelectList::new(flavors.iter().map(flavor_label).collect())
            .with_details(flavors.iter().map(flavor_detail).collect())
    }

    fn version_key(&self) -> (&'static str, String) {
        (edition_label(self.edition), self.flavor.clone())
    }

    /// The seed's fields held to the chosen edition's rules: a key the edition
    /// has no rule for is dropped when it is a JVM setting, and a choice takes
    /// the edition's own values.
    fn applicable_fields(&self) -> Vec<Field> {
        let catalogue = self.catalogue();
        let rules = catalogue.map(|c| c.rules.as_slice()).unwrap_or(&[]);
        let name_key = catalogue.and_then(|c| c.name_key.as_deref());
        let rule = |key: &str| rules.iter().find(|r| r.key == key);
        self.seed
            .fields
            .iter()
            .filter(|f| f.key != "memory" || rule("memory").is_some())
            .filter(|f| f.key != NAME_FIELD || name_key.is_some())
            .map(|f| {
                let mut field = f.clone();
                if field.key == NAME_FIELD {
                    if let Some(key) = name_key {
                        field.key = Cow::Owned(key.to_string());
                    }
                }
                if let (FieldKind::Choice(options), Some(KeyKind::Choice { values })) =
                    (&mut field.kind, rule(&field.key).map(|r| &r.kind))
                {
                    *options = values.clone();
                }
                field
            })
            .collect()
    }

    fn default_name(&self) -> String {
        format!("{}-{}", self.flavor, self.version)
    }

    /// Per-step setup when navigation lands on it.
    fn entered_step(&mut self) {
        self.status = None;
        if self.current() == Step::Version {
            match self.versions.get(&self.version_key()) {
                Some(versions) => self.picker = Some(version_picker(versions)),
                None => {
                    self.picker = None;
                    let _ = self.requests.send(Request::Versions {
                        edition: self.edition,
                        flavor: self.flavor.clone(),
                    });
                }
            }
        }
    }

    fn advance(&mut self) {
        if self.step + 1 < STEPS.len() {
            self.step += 1;
            self.entered_step();
        }
    }

    /// Esc: cancel an edit, else one step back; backing past the first
    /// unprefilled step cancels the wizard.
    fn back(&mut self) -> Flow<Option<WizardOutcome>> {
        if self.editing.take().is_some() {
            return Flow::Continue;
        }
        if self.step == self.first_step {
            return Flow::Done(None);
        }
        self.step -= 1;
        self.entered_step();
        Flow::Continue
    }

    /// Parse-check the settings values; an error keeps the step with a note.
    fn validate_settings(&self) -> Result<(), String> {
        for field in &self.fields {
            if field.value.is_empty() {
                continue;
            }
            match &field.kind {
                FieldKind::Number => {
                    if field.value.parse::<u32>().is_err() {
                        return Err(format!("{} must be a number", field.label));
                    }
                }
                FieldKind::Choice(options) => {
                    if !options.contains(&field.value) {
                        return Err(format!(
                            "{} must be one of {}",
                            field.label,
                            options.join(", ")
                        ));
                    }
                }
                FieldKind::Text => {}
            }
        }
        Ok(())
    }

    fn field_value(&self, key: &str) -> String {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.clone())
            .unwrap_or_default()
    }

    fn config_entries(&self) -> Vec<ConfigEntry> {
        let mut config = Vec::new();
        for field in &self.fields {
            if field.key == "port" || field.value.is_empty() {
                continue;
            }
            config.push(ConfigEntry {
                key: field.key.to_string(),
                value: field.value.clone(),
            });
        }
        config.extend(self.seed.extra.iter().cloned());
        config
    }

    fn create(&mut self) {
        let name = if self.name.is_empty() {
            self.default_name()
        } else {
            self.name.text().trim().to_string()
        };
        match self.seed.kind {
            EntryKind::Server => {
                let port = self.field_value("port").parse::<u16>().ok();
                let params = ServerCreateParams {
                    name,
                    edition: self.edition,
                    flavor: self.flavor.clone(),
                    version: self.version.clone(),
                    loader_version: self.seed.loader.clone(),
                    eula: true,
                    port,
                    config: self.config_entries(),
                    id: String::new(),
                };
                let _ = self.requests.send(Request::CreateServer(Box::new(params)));
            }
            EntryKind::Instance => {
                let _ = self
                    .requests
                    .send(Request::CreateInstance(Box::new(InstanceCreate {
                        name,
                        flavor: self.flavor.clone(),
                        version: self.version.clone(),
                        loader: self.seed.loader.clone(),
                        config: self.config_entries(),
                    })));
            }
        }
        self.progress = None;
        self.step = STEPS.len();
    }

    fn on_key_settings(&mut self, key: KeyEvent) {
        if self.editing.is_some() {
            if key.code == KeyCode::Enter {
                let value = self.editing.take().expect("editing").take();
                self.fields[self.settings_cursor].value = value.trim().to_string();
            } else if let Some(editing) = self.editing.as_mut() {
                editing.on_key(&key);
            }
            return;
        }
        let rows = self.fields.len() + 1;
        match key.code {
            KeyCode::Up => self.settings_cursor = self.settings_cursor.saturating_sub(1),
            KeyCode::Down => self.settings_cursor = (self.settings_cursor + 1).min(rows - 1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.settings_cursor == self.fields.len() {
                    if key.code == KeyCode::Enter {
                        match self.validate_settings() {
                            Ok(()) => self.advance(),
                            Err(message) => self.status = Some(message),
                        }
                    }
                    return;
                }
                let field = &mut self.fields[self.settings_cursor];
                match &field.kind {
                    FieldKind::Choice(options) => {
                        let next = match options.iter().position(|o| *o == field.value) {
                            Some(i) if i + 1 < options.len() => options[i + 1].clone(),
                            Some(_) => String::new(),
                            None => options[0].clone(),
                        };
                        field.value = next;
                    }
                    FieldKind::Text | FieldKind::Number => {
                        if key.code == KeyCode::Enter {
                            self.editing = Some(TextInput::with_text(&field.value));
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

impl Screen for WizardScreen {
    type Event = AppEvent;
    type Outcome = Option<WizardOutcome>;

    fn on_event(&mut self, event: AppEvent) -> Flow<Self::Outcome> {
        match event {
            AppEvent::Versions {
                edition,
                flavor,
                versions,
            } => {
                let key = (edition_label(edition), flavor);
                if self.current() == Step::Version && key == self.version_key() {
                    self.picker = Some(version_picker(&versions));
                }
                self.versions.insert(key, versions);
            }
            AppEvent::Progress(progress) => {
                if self.current() == Step::Working {
                    self.progress = Some(progress);
                }
            }
            AppEvent::ServerCreated(info) => {
                return Flow::Done(Some(WizardOutcome::Server(info)));
            }
            AppEvent::InstanceCreated(info) => {
                return Flow::Done(Some(WizardOutcome::Instance(info)));
            }
            AppEvent::Failed { message } => match self.current() {
                Step::Working => {
                    self.status = Some(message);
                    self.step = STEPS.len() - 1;
                }
                _ => self.status = Some(message),
            },
        }
        Flow::Continue
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return match self.current() {
                Step::Working => Flow::Continue,
                _ => Flow::Done(None),
            };
        }
        match self.current() {
            Step::Edition => match key.code {
                KeyCode::Esc => return self.back(),
                KeyCode::Enter => {
                    if let Some(catalogue) = self.seed.catalogues.get(self.edition_list.selected())
                    {
                        self.edition = catalogue.edition;
                        if !catalogue.flavors.iter().any(|f| f.id == self.flavor) {
                            self.flavor.clear();
                            self.version.clear();
                        }
                        self.flavor_list = self.flavor_select();
                        self.fields = self.applicable_fields();
                        self.advance();
                    }
                }
                _ => {
                    self.edition_list.on_key(&key);
                }
            },
            Step::Flavor => match key.code {
                KeyCode::Esc => return self.back(),
                KeyCode::Enter => {
                    let flavors = self
                        .catalogue()
                        .map(|c| c.flavors.as_slice())
                        .unwrap_or(&[]);
                    if let Some(flavor) = flavors.get(self.flavor_list.selected()) {
                        self.flavor = flavor.id.clone();
                        self.advance();
                    }
                }
                _ => {
                    self.flavor_list.on_key(&key);
                }
            },
            Step::Version => match key.code {
                KeyCode::Esc => return self.back(),
                KeyCode::Enter => {
                    if let Some(picker) = &self.picker {
                        if let Some(index) = picker.selected() {
                            self.version = self.versions[&self.version_key()][index].id.clone();
                            self.advance();
                        }
                    }
                }
                _ => {
                    if let Some(picker) = self.picker.as_mut() {
                        picker.on_key(&key);
                    }
                }
            },
            Step::Name => match key.code {
                KeyCode::Esc => return self.back(),
                KeyCode::Enter => self.advance(),
                _ => {
                    self.name.on_key(&key);
                }
            },
            Step::Settings => match key.code {
                KeyCode::Esc => return self.back(),
                _ => self.on_key_settings(key),
            },
            Step::Confirm => match key.code {
                KeyCode::Esc => return self.back(),
                KeyCode::Char(' ') if self.seed.kind == EntryKind::Server => {
                    self.eula = !self.eula;
                }
                KeyCode::Enter => {
                    if self.eula {
                        self.create();
                    } else {
                        self.status =
                            Some("accept the Minecraft EULA to continue (space toggles)".into());
                    }
                }
                _ => {}
            },
            Step::Working => {}
        }
        Flow::Continue
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        self.draw_breadcrumb(frame, header);

        match self.current() {
            Step::Edition => {
                self.edition_list.render(frame, body);
                hint(frame, footer, "↑/↓ move · enter next · esc cancel");
            }
            Step::Flavor => {
                self.flavor_list.render(frame, body);
                hint(frame, footer, "↑/↓ move · enter next · esc cancel");
            }
            Step::Version => {
                match self.picker.as_mut() {
                    Some(picker) => picker.render(frame, body, "version"),
                    None => frame.render_widget(
                        Paragraph::new(Line::styled(
                            "fetching versions…",
                            Style::default().fg(Color::DarkGray),
                        )),
                        body,
                    ),
                }
                hint(
                    frame,
                    footer,
                    "type to filter · tab snapshots · enter next · esc back",
                );
            }
            Step::Name => {
                let [row, _] =
                    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(body);
                let label = Span::styled("name: ", Style::default().fg(Color::Cyan));
                let [label_area, field_area] = Layout::horizontal([
                    Constraint::Length(label.width() as u16),
                    Constraint::Min(0),
                ])
                .areas(row);
                frame.render_widget(Paragraph::new(label), label_area);
                self.name.render(frame, field_area);
                if self.name.is_empty() {
                    frame.render_widget(
                        Paragraph::new(self.default_name())
                            .style(Style::default().fg(Color::DarkGray)),
                        field_area,
                    );
                }
                hint(frame, footer, "enter next · esc back");
            }
            Step::Settings => {
                self.draw_settings(frame, body);
                let text = match self.editing.is_some() {
                    true => "enter accept · esc cancel edit",
                    false => "↑/↓ move · enter edit/next · space cycle · esc back",
                };
                hint(frame, footer, text);
            }
            Step::Confirm => {
                self.draw_confirm(frame, body);
                let text = match self.seed.kind {
                    EntryKind::Server => "enter create · space toggle eula · esc back",
                    EntryKind::Instance => "enter create · esc back",
                };
                hint(frame, footer, text);
            }
            Step::Working => {
                let label = match self.seed.kind {
                    EntryKind::Server => "provisioning",
                    EntryKind::Instance => "creating",
                };
                draw_working(frame, label, self.progress.as_ref());
            }
        }
    }
}

impl WizardScreen {
    fn draw_breadcrumb(&self, frame: &mut Frame, area: Rect) {
        let mut spans = vec![
            Span::styled(
                format!("create {}", self.seed.kind),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" · ", Style::default().fg(Color::DarkGray)),
        ];
        for (i, step) in STEPS.iter().enumerate() {
            if *step == Step::Edition && !self.asks_edition() {
                continue;
            }
            if spans.len() > 2 {
                spans.push(Span::styled(" ▸ ", Style::default().fg(Color::DarkGray)));
            }
            let style = match i.cmp(&self.step.min(STEPS.len() - 1)) {
                std::cmp::Ordering::Less => Style::default().fg(Color::DarkGray),
                std::cmp::Ordering::Equal => Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                std::cmp::Ordering::Greater => Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            };
            spans.push(Span::styled(step_label(*step), style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        if let Some(status) = &self.status {
            let [_, status_row] =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
            frame.render_widget(
                Paragraph::new(Line::styled(
                    status.clone(),
                    Style::default().fg(Color::Yellow),
                )),
                status_row,
            );
        }
    }

    fn draw_settings(&mut self, frame: &mut Frame, area: Rect) {
        let width = self.fields.iter().map(|f| f.label.len()).max().unwrap_or(0);
        let mut rows: Vec<ListItem> = Vec::new();
        for (i, field) in self.fields.iter().enumerate() {
            let mut spans = vec![Span::styled(
                format!("{:width$}  ", field.label),
                Style::default().fg(Color::DarkGray),
            )];
            if let Some(editing) = self.editing.as_ref().filter(|_| i == self.settings_cursor) {
                spans.push(Span::raw(format!("{}▏", editing.text())));
            } else if field.value.is_empty() {
                spans.push(Span::styled(
                    field.hint,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ));
            } else {
                spans.push(Span::raw(field.value.clone()));
            }
            rows.push(ListItem::new(Line::from(spans)));
        }
        rows.push(ListItem::new(Line::from("continue")));
        let mut state = ListState::default();
        state.select(Some(self.settings_cursor));
        let list = List::new(rows).highlight_symbol("> ").highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn confirm_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        if self.asks_edition() {
            rows.push(("edition".to_string(), edition_label(self.edition).into()));
        }
        rows.extend([
            ("flavor".to_string(), self.flavor.clone()),
            ("version".to_string(), self.version.clone()),
            (
                "name".to_string(),
                if self.name.is_empty() {
                    self.default_name()
                } else {
                    self.name.text().trim().to_string()
                },
            ),
        ]);
        if let Some(loader) = &self.seed.loader {
            rows.push(("loader".to_string(), loader.clone()));
        }
        for field in &self.fields {
            if !field.value.is_empty() {
                rows.push((field.label.to_string(), field.value.clone()));
            }
        }
        for entry in &self.seed.extra {
            rows.push((entry.key.clone(), entry.value.clone()));
        }
        rows
    }

    fn draw_confirm(&self, frame: &mut Frame, area: Rect) {
        let rows = self.confirm_rows();
        let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        let dim = Style::default().fg(Color::DarkGray);
        let mut lines: Vec<Line> = rows
            .iter()
            .map(|(key, value)| {
                Line::from(vec![
                    Span::styled(format!("{key:width$}  "), dim),
                    Span::raw(value.clone()),
                ])
            })
            .collect();
        if self.seed.kind == EntryKind::Server {
            lines.push(Line::raw(""));
            let mark = if self.eula { "[x]" } else { "[ ]" };
            lines.push(Line::styled(
                format!("{mark} accept the Minecraft EULA ({EULA_URL})"),
                match self.eula {
                    true => Style::default().fg(Color::Green),
                    false => Style::default().fg(Color::Yellow),
                },
            ));
        }
        frame.render_widget(Paragraph::new(lines), area);
    }
}

fn hint(frame: &mut Frame, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(Line::from(text)).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn version_picker(versions: &[GameVersion]) -> Picker {
    Picker::new(
        versions
            .iter()
            .map(|v| PickerItem {
                label: v.id.clone(),
                tag: kind_label(v.kind).to_string(),
                stable: v.kind == VersionKind::Release,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use client::proto::keys::KeyRule;
    use client::proto::minecraft::Flavor;

    fn catalogue(edition: Edition, rules: Vec<KeyRule>) -> EditionCatalogue {
        EditionCatalogue {
            edition,
            rules,
            flavors: vec![Flavor {
                id: "vanilla".into(),
                ..Default::default()
            }],
            name_key: Some(
                match edition {
                    Edition::Java => "motd",
                    Edition::Bedrock => "server-name",
                }
                .into(),
            ),
        }
    }

    fn bedrock_rules() -> Vec<KeyRule> {
        vec![KeyRule {
            key: "gamemode".into(),
            kind: KeyKind::Choice {
                values: vec!["survival".into(), "creative".into()],
            },
        }]
    }

    fn screen(catalogues: Vec<EditionCatalogue>, edition: Option<Edition>) -> WizardScreen {
        let (tx, _rx) = unbounded_channel();
        let seed = WizardSeed {
            kind: EntryKind::Server,
            catalogues,
            edition,
            flavor: None,
            version: None,
            name: None,
            loader: None,
            eula: false,
            fields: vec![
                Field::text("memory", "memory", "", None),
                Field::text(NAME_FIELD, "motd", "", None),
                Field::choice("gamemode", "gamemode", "", &["survival", "spectator"], None),
            ],
            extra: Vec::new(),
        };
        WizardScreen::new(seed, tx)
    }

    fn both() -> Vec<EditionCatalogue> {
        vec![
            catalogue(Edition::Java, vec![]),
            catalogue(Edition::Bedrock, bedrock_rules()),
        ]
    }

    #[test]
    fn several_editions_start_on_the_edition_step() {
        assert_eq!(screen(both(), None).current(), Step::Edition);
    }

    #[test]
    fn one_edition_or_a_named_one_skips_the_edition_step() {
        let one = screen(vec![catalogue(Edition::Java, vec![])], None);
        assert_eq!(one.current(), Step::Flavor);
        let named = screen(both(), Some(Edition::Bedrock));
        assert_eq!(named.current(), Step::Flavor);
        assert_eq!(named.edition, Edition::Bedrock);
    }

    #[test]
    fn fields_follow_the_chosen_editions_rules() {
        let s = screen(
            vec![catalogue(Edition::Bedrock, bedrock_rules())],
            Some(Edition::Bedrock),
        );
        let fields = s.applicable_fields();
        let keys: Vec<_> = fields.iter().map(|f| f.key.as_ref()).collect();
        assert_eq!(keys, ["server-name", "gamemode"]);
        match &fields[1].kind {
            FieldKind::Choice(options) => assert_eq!(options, &["survival", "creative"]),
            _ => panic!("gamemode stays a choice"),
        }
    }
}
