//! Rendering per mode: thin composition over the shared components (the
//! scrollable reader, the modal frame, the pickers). The models supply the
//! data; nothing here mutates them beyond the reader's own scroll geometry.

use client::proto::content::ContentKind;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use super::{ContentSession, Focus, Overlay};
use crate::commands::content::format::{compact, kind_plural, side_label};
use crate::ui::components::modal;
use crate::ui::markdown;

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn heading() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

impl ContentSession {
    fn status_line(&self) -> String {
        if let Some(error) = &self.catalogue.status {
            return error.clone();
        }
        let mut parts = Vec::new();
        parts.extend(self.base.loaders.iter().cloned());
        if let Some(game) = &self.base.game_version {
            parts.push(game.clone());
        }
        if self.catalogue.searching() {
            parts.push("searching…".to_string());
        } else {
            parts.push(format!("{} results", self.catalogue.total));
        }
        parts.join(" · ")
    }

    pub(super) fn draw_browse(&mut self, frame: &mut Frame) {
        let [box_area, body, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let title = format!("search {}", kind_plural(self.base.kind));
        let block = Block::bordered().border_style(dim()).title(title);
        let inner = block.inner(box_area);
        frame.render_widget(block, box_area);
        let status = self.status_line();
        let [input_area, status_area] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(status.chars().count() as u16 + 1),
        ])
        .areas(inner);
        frame.render_widget(Paragraph::new(Span::styled(status, dim())), status_area);
        self.catalogue.search.render_focused(
            frame,
            input_area,
            matches!(self.focus, Focus::Search),
        );

        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(55), Constraint::Min(0)]).areas(body);
        self.draw_hits(frame, list_area);
        self.draw_detail(frame, detail_area);

        let hint = match (self.target.is_some(), &self.focus) {
            (_, Focus::Search) => "type to search · ↓ results · esc quit",
            (true, Focus::List) => {
                "↑/↓ move · space toggle · v version · pgup/pgdn description · enter review · esc quit"
            }
            (false, Focus::List) => "↑/↓ move · enter versions · pgup/pgdn description · esc quit",
        };
        frame.render_widget(Paragraph::new(Line::from(hint)).style(dim()), footer);
    }

    fn draw_hits(&mut self, frame: &mut Frame, area: Rect) {
        if self.catalogue.hits.is_empty() {
            let text = if self.catalogue.searching() {
                "searching…"
            } else {
                "no results"
            };
            frame.render_widget(Paragraph::new(Line::styled(text, dim())), area);
            return;
        }
        let with_boxes = self.target.is_some();
        let rows: Vec<ListItem> = self
            .catalogue
            .hits
            .iter()
            .map(|hit| {
                let mut spans = Vec::new();
                if with_boxes {
                    let installed = self.installed_entry(hit).is_some();
                    let mark = if installed && self.cart.is_removing(&hit.id) {
                        Span::styled("[-] ", Style::default().fg(Color::Red))
                    } else if installed && !self.cart.is_chosen(&hit.id) {
                        Span::styled("[✓] ", Style::default().fg(Color::Green))
                    } else if self.cart.is_chosen(&hit.id) {
                        Span::raw("[x] ")
                    } else {
                        Span::raw("[ ] ")
                    };
                    spans.push(mark);
                }
                spans.push(Span::raw(hit.title.clone()));
                spans.push(Span::styled(
                    format!("  ↓{}", compact(hit.downloads)),
                    dim(),
                ));
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(heading());
        frame.render_stateful_widget(list, area, &mut self.catalogue.list);
    }

    fn draw_detail(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::new().borders(Borders::LEFT).border_style(dim());
        let inner = block.inner(area).inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        frame.render_widget(block, area);
        let Some(hit) = self.catalogue.highlighted() else {
            return;
        };
        let project = self.catalogue.detail_project(hit);

        let mut lines = vec![
            Line::styled(project.title.clone(), heading()),
            Line::styled(
                format!(
                    "by {} · {} downloads",
                    project.author,
                    compact(project.downloads)
                ),
                dim(),
            ),
            Line::styled(project.categories.join(" · "), dim()),
            Line::styled(
                format!(
                    "client {} · server {}",
                    side_label(project.client_side),
                    side_label(project.server_side)
                ),
                dim(),
            ),
        ];
        if let Some(label) = self.installed_label(project) {
            lines.push(Line::styled(
                format!("✓ installed {label}"),
                Style::default().fg(Color::Green),
            ));
        }
        lines.push(Line::raw(""));
        for line in project.description.split('\n') {
            lines.push(Line::raw(line.to_string()));
        }
        if !project.body.is_empty() {
            lines.push(Line::raw(""));
            lines.extend(markdown::render(&project.body));
        }

        self.catalogue.reader.render(frame, inner, lines);
    }

    pub(super) fn draw_review(&self, frame: &mut Frame, cursor: usize) {
        let [header, body, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(frame.area());

        let name = self
            .target
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let mut what = Vec::new();
        if !self.cart.chosen.is_empty() {
            what.push(format!("install {}", self.cart.chosen.len()));
        }
        if !self.cart.removals.is_empty() {
            what.push(format!("remove {}", self.cart.removals.len()));
        }
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!(
                    "review · {} {} for '{name}'",
                    what.join(", "),
                    kind_plural(self.base.kind)
                ),
                heading(),
            )),
            header,
        );

        let mut rows: Vec<ListItem> = self
            .cart
            .chosen
            .iter()
            .map(|c| {
                let mut spans = vec![
                    Span::styled("+ ", Style::default().fg(Color::Green)),
                    Span::raw(c.project.title.clone()),
                    Span::styled(format!("  {}", c.version_label), dim()),
                ];
                if let Some(marker) = self.review_marker(&c.project) {
                    spans.push(Span::styled(
                        format!("  {marker}"),
                        Style::default().fg(Color::Yellow),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        for staged in &self.cart.removals {
            let record = self.staged_record(staged);
            let title = record
                .map(|r| r.title.clone())
                .unwrap_or_else(|| staged.project_id.clone());
            let what = match record {
                Some(record) => format!("remove {}", record.version_number),
                None => "remove".to_string(),
            };
            rows.push(ListItem::new(Line::from(vec![
                Span::styled("- ", Style::default().fg(Color::Red)),
                Span::raw(title),
                Span::styled(format!("  {what}"), Style::default().fg(Color::Yellow)),
            ])));
        }
        if self.base.kind == ContentKind::DataPack && !self.cart.chosen.is_empty() {
            let worlds = if !self.instance_datapacks() {
                "world: the server's own".to_string()
            } else if self.cart.worlds.is_empty() {
                "worlds: all (w to pick)".to_string()
            } else {
                format!("worlds: {}", self.cart.worlds.join(", "))
            };
            rows.push(ListItem::new(Line::styled(
                worlds,
                Style::default().fg(Color::Yellow),
            )));
        }
        let mut state = ListState::default();
        state.select(Some(cursor));
        let list = List::new(rows)
            .highlight_symbol("> ")
            .highlight_style(heading());
        frame.render_stateful_widget(list, body, &mut state);

        let hint = if self.base.kind == ContentKind::DataPack {
            "enter apply · v version · w worlds · space drop · esc back"
        } else {
            "enter apply · v version · space drop · esc back"
        };
        frame.render_widget(Paragraph::new(Line::from(hint)).style(dim()), footer);
    }

    pub(super) fn draw_overlay(&mut self, frame: &mut Frame) {
        let area = modal::centered_rect(frame.area(), 60, 70);
        frame.render_widget(Clear, area);
        match self.overlay.as_mut() {
            Some(Overlay::Versions { project, picker }) => match picker {
                Some((picker, _)) => {
                    let [picker_area, hint] =
                        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
                    picker.render(frame, picker_area, &project.title);
                    frame.render_widget(
                        Paragraph::new(Line::from("enter pin version · esc keep latest"))
                            .style(dim()),
                        hint,
                    );
                }
                None => {
                    let inner = modal::bordered(frame, area, &project.title);
                    frame.render_widget(
                        Paragraph::new(Line::styled("fetching versions…", dim())),
                        inner,
                    );
                }
            },
            Some(Overlay::Worlds(list, _)) => {
                let inner = modal::bordered(frame, area, "install into world(s)");
                let [list_area, hint] =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
                list.render(frame, list_area);
                frame.render_widget(
                    Paragraph::new(Line::from("space toggle · enter confirm · esc cancel"))
                        .style(dim()),
                    hint,
                );
            }
            Some(Overlay::RemoveWorlds { list, .. }) => {
                let inner = modal::bordered(frame, area, "keep loading in world(s)");
                let [list_area, hint] =
                    Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
                list.render(frame, list_area);
                frame.render_widget(
                    Paragraph::new(Line::from(
                        "space toggle · enter confirm · none = uninstall · esc uninstall",
                    ))
                    .style(dim()),
                    hint,
                );
            }
            None => {}
        }
    }
}
