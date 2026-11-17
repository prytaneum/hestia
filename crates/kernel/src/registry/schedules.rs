//! When each schedule of a plugin last ran, kept as wall-clock time under the
//! plugin's directory so a restart or a suspend neither loses a run nor repeats
//! one.

use std::collections::BTreeMap;

use common::schema::{self, Document};
use serde::{Deserialize, Serialize};

use super::guests::Guest;
use super::Plugins;
use crate::commands::ScheduleDecl;

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Runs {
    /// Unix seconds, by schedule id.
    last: BTreeMap<String, u64>,
}

impl Document for Runs {
    const NAME: &'static str = "schedules.json";
}

/// Moves each declared schedule's last run to `now` where it came due, and
/// returns those. A schedule never seen, or last run in the future (the clock
/// was set back), starts counting from `now` without running. One that fell due
/// while nothing ticked runs once, however many intervals went by.
fn advance(runs: &mut Runs, declared: &[ScheduleDecl], now: u64) -> Vec<String> {
    let mut due = Vec::new();
    runs.last
        .retain(|id, _| declared.iter().any(|schedule| &schedule.id == id));
    for schedule in declared {
        let Some(every) = schedule.interval() else {
            continue;
        };
        match runs.last.get(&schedule.id) {
            Some(last) if now >= *last && now - last >= every.as_secs() => {
                due.push(schedule.id.clone())
            }
            Some(last) if now >= *last => continue,
            _ => {}
        }
        runs.last.insert(schedule.id.clone(), now);
    }
    due
}

impl Plugins {
    fn runs_path(&self, id: &str) -> std::path::PathBuf {
        self.store.dir(id).join(Runs::NAME)
    }

    /// Starts every schedule of `guest` counting from `now`, as an install or
    /// an enable does.
    pub(super) fn start_schedules(&self, guest: &Guest, now: u64) {
        if guest.schedules.is_empty() {
            return;
        }
        let runs = Runs {
            last: guest
                .schedules
                .iter()
                .map(|s| (s.id.clone(), now))
                .collect(),
        };
        if let Err(error) = schema::save(&self.runs_path(&guest.id), &runs) {
            tracing::warn!(plugin = guest.id, %error, "cannot save when its schedules started");
        }
    }

    /// The schedules of `guest` that are due at `now`, recorded as run.
    pub(super) fn due_schedules(&self, guest: &Guest, now: u64) -> Vec<String> {
        if guest.schedules.is_empty() {
            return Vec::new();
        }
        let path = self.runs_path(&guest.id);
        let mut runs = match schema::try_load::<Runs>(&path) {
            Ok(runs) => runs.unwrap_or_default(),
            Err(error) => {
                tracing::error!(plugin = guest.id, %error, "schedules not checked");
                return Vec::new();
            }
        };
        let before = runs.last.clone();
        let due = advance(&mut runs, &guest.schedules, now);
        if runs.last != before {
            if let Err(error) = schema::save(&path, &runs) {
                tracing::warn!(plugin = guest.id, %error, "cannot save when its schedules ran");
            }
        }
        due
    }
}
