//! The browse sub-model: the live search over a content source and its result
//! list, detail cache, and version cache. It drives the browse-only path in
//! full and feeds the install path's picker. Query building takes the session's
//! immutable filter context (`base`) by reference; everything dynamic lives
//! here.
//!
//! Search replies carry the sequence number of the query that produced them, so
//! a stale reply (the query changed while it was in flight) is dropped.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use client::proto::content::{
    ContentProject, ContentVersion, SearchQuery, SearchResult, VersionQuery,
};
use ratatui::widgets::ListState;
use tokio::sync::mpsc::UnboundedSender;

use super::driver::Request;
use crate::commands::content::PAGE;
use crate::ui::components::{ScrollText, TextInput};

const DEBOUNCE: Duration = Duration::from_millis(250);

pub(super) struct Catalogue {
    requests: UnboundedSender<Request>,

    pub search: TextInput,
    debounce: Option<Instant>,
    sent_seq: u64,
    applied_seq: u64,

    pub hits: Vec<ContentProject>,
    pub total: u32,
    pub list: ListState,

    details: HashMap<String, ContentProject>,
    detail_requested: HashSet<String>,
    versions: HashMap<String, Vec<ContentVersion>>,
    pub status: Option<String>,

    /// The detail pane's scroll state and geometry, reset when the highlight
    /// moves.
    pub reader: ScrollText,
}

impl Catalogue {
    pub(super) fn new(requests: UnboundedSender<Request>, base: &SearchQuery) -> Self {
        let mut catalogue = Catalogue {
            search: TextInput::with_text(&base.query),
            requests,
            debounce: None,
            sent_seq: 0,
            applied_seq: 0,
            hits: Vec::new(),
            total: 0,
            list: ListState::default(),
            details: HashMap::new(),
            detail_requested: HashSet::new(),
            versions: HashMap::new(),
            status: None,
            reader: ScrollText::default(),
        };
        catalogue.send_search(base, 0);
        catalogue
    }

    fn query(&self, base: &SearchQuery, offset: u32) -> SearchQuery {
        SearchQuery {
            query: self.search.text().trim().to_string(),
            offset,
            limit: PAGE,
            ..base.clone()
        }
    }

    fn send_search(&mut self, base: &SearchQuery, offset: u32) {
        self.sent_seq += 1;
        let _ = self.requests.send(Request::Search {
            seq: self.sent_seq,
            query: self.query(base, offset),
        });
    }

    pub(super) fn searching(&self) -> bool {
        self.applied_seq < self.sent_seq
    }

    pub(super) fn highlighted(&self) -> Option<&ContentProject> {
        self.hits.get(self.list.selected().unwrap_or(0))
    }

    /// The detailed project for a hit if its long description has arrived, else
    /// the hit itself.
    pub(super) fn detail_project<'a>(&'a self, hit: &'a ContentProject) -> &'a ContentProject {
        self.details.get(&hit.id).unwrap_or(hit)
    }

    /// Ask for the highlighted project's long description once.
    pub(super) fn want_detail(&mut self) {
        let Some((id, source, slug, kind)) = self.highlighted().map(|hit| {
            (
                hit.id.clone(),
                hit.source.clone(),
                hit.slug.clone(),
                hit.kind,
            )
        }) else {
            return;
        };
        if self.details.contains_key(&id) || !self.detail_requested.insert(id) {
            return;
        }
        let _ = self.requests.send(Request::Detail {
            source,
            project: slug,
            kind,
        });
    }

    pub(super) fn versions_for(&self, project_id: &str) -> Option<&Vec<ContentVersion>> {
        self.versions.get(project_id)
    }

    pub(super) fn request_versions(&self, base: &SearchQuery, project: &ContentProject) {
        let _ = self.requests.send(Request::Versions {
            query: VersionQuery {
                source: project.source.clone(),
                project: project.id.clone(),
                loaders: base.loaders.clone(),
                game_version: base.game_version.clone(),
            },
        });
    }

    pub(super) fn step(&mut self, base: &SearchQuery, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let last = self.hits.len() as isize - 1;
        let current = self.list.selected().unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, last) as usize;
        self.list.select(Some(next));
        self.reader.reset();
        self.want_detail();
        if next + 3 >= self.hits.len() && (self.hits.len() as u32) < self.total && !self.searching()
        {
            let offset = self.hits.len() as u32;
            self.send_search(base, offset);
        }
    }

    pub(super) fn scroll(&mut self, delta: i32) {
        self.reader.scroll_by(delta);
    }

    /// A key that neither the list nor navigation claimed goes to the search
    /// field; a consumed key (re)arms the debounce and reports `true`.
    pub(super) fn search_key(&mut self, key: &ratatui::crossterm::event::KeyEvent) -> bool {
        if self.search.on_key(key) {
            self.debounce = Some(Instant::now());
            true
        } else {
            false
        }
    }

    /// Fire the pending debounced search once its deadline passes; `true`
    /// requests a redraw.
    pub(super) fn tick(&mut self, base: &SearchQuery) -> bool {
        if let Some(since) = self.debounce {
            if since.elapsed() >= DEBOUNCE {
                self.debounce = None;
                self.send_search(base, 0);
                return true;
            }
        }
        false
    }

    pub(super) fn apply_search(&mut self, seq: u64, offset: u32, result: SearchResult) {
        if seq < self.sent_seq {
            return;
        }
        self.applied_seq = seq;
        self.total = result.total;
        if offset == 0 {
            self.hits = result.hits;
            self.list.select((!self.hits.is_empty()).then_some(0));
        } else {
            let known: HashSet<String> = self.hits.iter().map(|h| h.id.clone()).collect();
            self.hits
                .extend(result.hits.into_iter().filter(|h| !known.contains(&h.id)));
        }
        self.status = None;
        self.want_detail();
    }

    pub(super) fn apply_detail(&mut self, project: ContentProject) {
        self.details.insert(project.id.clone(), project);
    }

    pub(super) fn apply_versions(&mut self, project_id: String, versions: Vec<ContentVersion>) {
        self.versions.insert(project_id, versions);
    }
}
