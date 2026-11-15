//! A server's worlds: what it has, and bringing a Bedrock `.mcworld` in.

use anyhow::Result;
use proto::instance::WorldInfo;

use super::transfer::daemon_local_path;
use crate::cancel::Job;
use crate::edition;
use crate::engine::Engine;
use crate::properties;

const LEVEL_NAME: &str = "level-name";

impl Engine {
    /// Java keeps the one world `level-name` names; Bedrock keeps every folder
    /// under `worlds/`, the active one first.
    pub fn server_worlds(&self, reference: &str) -> Result<Vec<WorldInfo>> {
        let record = self.servers.require(reference)?;
        let data_dir = self.servers.data_dir(&record);
        let active = properties::level_name(&data_dir);
        Ok(edition::worlds(record.edition, &data_dir, &active))
    }

    /// Unpack a `.mcworld` into the server's `worlds/`; with `activate`, make it
    /// the world the server loads at its next start.
    pub fn import_server_world(
        &self,
        reference: &str,
        path: &str,
        activate: bool,
        job: &Job<'_>,
    ) -> Result<WorldInfo> {
        let record = self.servers.require(reference)?;
        let archive = daemon_local_path(path)?;
        let data_dir = self.servers.data_dir(&record);
        let world = edition::import_world(record.edition, &data_dir, &archive, job)?;
        if activate {
            if let Err(error) = self
                .servers
                .config_set(&record.id, LEVEL_NAME, &world.folder)
            {
                let _ = std::fs::remove_dir_all(data_dir.join("worlds").join(&world.folder));
                return Err(error);
            }
        }
        Ok(world)
    }
}
