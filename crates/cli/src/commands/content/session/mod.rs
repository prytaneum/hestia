//! The fullscreen content session: search a source with a live results list and
//! detail pane, check items (with optional per-item version pins), review the
//! batch, and install it as one daemon job — one alternate-screen flow from
//! query to report.
//!
//! The state is two cohesive sub-models the session composes: [`Catalogue`] is
//! the browse half (search, results, detail, versions) and drives the
//! browse-only path in full; [`Cart`] is the staged additions and removals. The
//! session ties them together, runs the mode machine, and owns the async
//! driver ([`driver`]) it talks to over two channels.

mod cart;
mod catalogue;
mod driver;
mod input;
mod state;
mod view;

use anyhow::Result;
use client::proto::content::{
    ContentFailure, ContentProject, ContentVersion, InstalledContent, SearchQuery,
};
use client::proto::error::EntryTarget;
use client::proto::minecraft::ProvisionProgress;
use client::Client;
use ratatui::crossterm::event::{KeyEvent, MouseEvent, MouseEventKind};
use ratatui::Frame;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::ui::components::working::draw_working;
use crate::ui::components::{Picker, SelectList};
use crate::ui::session::{self, Flow, Screen};
use cart::Cart;
use catalogue::Catalogue;
use driver::{drive, AppEvent, Request};

/// The entry a session installs into; `None` browses read-only.
pub struct Target {
    pub entry: EntryTarget,
    pub name: String,
    /// An instance's save worlds, for the datapack world picker.
    pub worlds: Vec<String>,
    /// What the entry already has of this kind, for the installed markers.
    pub installed: Vec<InstalledContent>,
}

/// What the session resolved to; `None` when the user quit without applying.
pub struct SessionReport {
    pub items: Vec<InstalledContent>,
    pub removed: Vec<InstalledContent>,
    pub failures: Vec<ContentFailure>,
    pub error: Option<String>,
}

enum Focus {
    Search,
    List,
}

enum Mode {
    Browse,
    Review { cursor: usize },
    Installing { progress: Option<ProvisionProgress> },
}

enum Overlay {
    Versions {
        project: Box<ContentProject>,
        picker: Option<(Picker, Vec<ContentVersion>)>,
    },
    Worlds(SelectList, Vec<String>),
    /// Drop a staged datapack removal to some of the worlds it loads in — opened
    /// with all of them checked; unchecking every world uninstalls it outright.
    RemoveWorlds {
        project: String,
        list: SelectList,
        names: Vec<String>,
    },
}

struct ContentSession {
    catalogue: Catalogue,
    cart: Cart,
    target: Option<Target>,
    base: SearchQuery,
    /// A second handle on the request channel, for the install job the session
    /// itself sends (the catalogue holds the other, for search/detail/version
    /// lookups).
    requests: UnboundedSender<Request>,

    mode: Mode,
    overlay: Option<Overlay>,
    focus: Focus,
}

/// Run the content session over `base` (the seeded filters — kind, source,
/// loader, game version, sort). With a target, checked items install into it;
/// without one this is a pure browser.
pub async fn run(
    client: &Client,
    base: SearchQuery,
    target: Option<Target>,
) -> Result<Option<SessionReport>> {
    let (request_tx, request_rx) = unbounded_channel();
    let (event_tx, event_rx) = unbounded_channel();
    let screen = ContentSession::new(base, target, request_tx);
    let driver = drive(client, request_rx, event_tx);
    let (outcome, ()) = tokio::join!(session::run_async(screen, Some(event_rx)), driver);
    outcome
}

impl ContentSession {
    fn new(base: SearchQuery, target: Option<Target>, requests: UnboundedSender<Request>) -> Self {
        let catalogue = Catalogue::new(requests.clone(), &base);
        ContentSession {
            catalogue,
            cart: Cart::default(),
            target,
            base,
            requests,
            mode: Mode::Browse,
            overlay: None,
            focus: Focus::Search,
        }
    }
}

impl Screen for ContentSession {
    type Event = AppEvent;
    type Outcome = Option<SessionReport>;

    fn wants_mouse(&self) -> bool {
        true
    }

    fn tick(&mut self) -> bool {
        self.catalogue.tick(&self.base)
    }

    fn on_event(&mut self, event: AppEvent) -> Flow<Self::Outcome> {
        match event {
            AppEvent::Search {
                seq,
                offset,
                result,
            } => self.catalogue.apply_search(seq, offset, result),
            AppEvent::Detail(project) => self.catalogue.apply_detail(*project),
            AppEvent::Versions { project, versions } => {
                if let Some(Overlay::Versions {
                    project: wanted,
                    picker,
                }) = self.overlay.as_mut()
                {
                    if wanted.id == project && picker.is_none() {
                        *picker = Some(super::format::version_picker(&versions));
                    }
                }
                self.catalogue.apply_versions(project, versions);
            }
            AppEvent::Progress(progress) => {
                if let Mode::Installing { progress: current } = &mut self.mode {
                    *current = Some(progress);
                }
            }
            AppEvent::Done {
                items,
                removed,
                failures,
            } => {
                return Flow::Done(Some(SessionReport {
                    items,
                    removed,
                    failures,
                    error: None,
                }))
            }
            AppEvent::Failed { message } => match self.mode {
                Mode::Installing { .. } => {
                    return Flow::Done(Some(SessionReport {
                        items: Vec::new(),
                        removed: Vec::new(),
                        failures: Vec::new(),
                        error: Some(message),
                    }))
                }
                _ => self.catalogue.status = Some(message),
            },
        }
        Flow::Continue
    }

    fn on_key(&mut self, key: KeyEvent) -> Flow<Self::Outcome> {
        if self.overlay.is_some() {
            self.on_key_overlay(key);
            return Flow::Continue;
        }
        match &self.mode {
            Mode::Browse => self.on_key_browse(key),
            Mode::Review { cursor } => {
                let cursor = *cursor;
                self.on_key_review(key, cursor)
            }
            Mode::Installing { .. } => Flow::Continue,
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) -> Flow<Self::Outcome> {
        if matches!(self.mode, Mode::Browse) && self.overlay.is_none() {
            let over_detail = self.catalogue.reader.contains(mouse.column, mouse.row);
            match (mouse.kind, over_detail) {
                (MouseEventKind::ScrollUp, true) => self.catalogue.scroll(-3),
                (MouseEventKind::ScrollDown, true) => self.catalogue.scroll(3),
                (MouseEventKind::ScrollUp, false) => self.catalogue.step(&self.base, -3),
                (MouseEventKind::ScrollDown, false) => self.catalogue.step(&self.base, 3),
                _ => {}
            }
        }
        Flow::Continue
    }

    fn draw(&mut self, frame: &mut Frame) {
        match &self.mode {
            Mode::Browse => self.draw_browse(frame),
            Mode::Review { cursor } => {
                let cursor = *cursor;
                self.draw_review(frame, cursor)
            }
            Mode::Installing { progress } => draw_working(frame, "applying", progress.as_ref()),
        }
        if self.overlay.is_some() {
            self.draw_overlay(frame);
        }
    }
}
