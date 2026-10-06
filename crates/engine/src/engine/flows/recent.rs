//! The recently played list: every instance's join history, as the launches
//! that return to each target.

use std::cmp::Reverse;

use proto::instance::{QuickPlay, RecentPlay, ServerEntry};

use crate::engine::Engine;
use crate::instances::InstanceRecord;
use crate::minecraft::{launch, quickplay, servers, world};
use crate::recent::{self, Play};

impl Engine {
    /// Fold what the instance's sessions last joined into its history. Run as a
    /// session exits, and before the list is read — a running session's
    /// current world shows without waiting for it to end.
    pub fn record_plays(&self, reference: &str) {
        if let Some(record) = self.instances.get(reference) {
            self.fold_joins(&record);
        }
    }

    /// What was played lately across every instance, most recent first. A
    /// target that can no longer be joined — a deleted world, an instance moved
    /// below the Quick Play version — is left out rather than offered.
    pub fn recent_plays(&self) -> Vec<RecentPlay> {
        let mut plays: Vec<RecentPlay> = self
            .instances
            .list()
            .iter()
            .filter(|record| launch::supports_quick_play(&record.profile.game_version))
            .flat_map(|record| {
                self.fold_joins(record);
                self.joinable(record)
            })
            .collect();
        plays.sort_by_key(|play| Reverse(play.played_unix));
        plays.truncate(recent::LIMIT);
        plays
    }

    fn fold_joins(&self, record: &InstanceRecord) {
        let entry_dir = self.instances.instance_dir(record);
        let joins = quickplay::joins(&entry_dir.join(super::instance::SESSION_DIR));
        if let Err(e) = recent::record(&entry_dir, joins) {
            tracing::warn!(instance = %record.id, error = %e, "cannot record recent plays");
        }
    }

    fn joinable(&self, record: &InstanceRecord) -> Vec<RecentPlay> {
        let data_dir = self.instances.data_dir(record);
        let saves = data_dir.join("saves");
        let plays = recent::load(&self.instances.instance_dir(record));
        let listed = if plays
            .iter()
            .any(|play| matches!(play.target, QuickPlay::Server(_)))
        {
            servers::read(&data_dir)
        } else {
            Vec::new()
        };
        plays
            .into_iter()
            .filter_map(|play| {
                let icon = match &play.target {
                    QuickPlay::World(folder) => {
                        if !saves.join(folder).is_dir() {
                            return None;
                        }
                        world::icon(&saves, folder)
                    }
                    QuickPlay::Server(address) => server_icon(&listed, address),
                    QuickPlay::Realm(_) => String::new(),
                };
                let Play {
                    target,
                    name,
                    game_mode,
                    played_unix,
                } = play;
                Some(RecentPlay {
                    instance: record.id.clone(),
                    instance_name: record.name.clone(),
                    target,
                    name,
                    game_mode,
                    played_unix,
                    icon,
                })
            })
            .collect()
    }
}

fn server_icon(listed: &[ServerEntry], address: &str) -> String {
    listed
        .iter()
        .find(|server| server.address.eq_ignore_ascii_case(address))
        .map(|server| server.icon.clone())
        .unwrap_or_default()
}
