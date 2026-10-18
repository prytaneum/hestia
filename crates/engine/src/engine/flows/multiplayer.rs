//! The instance's multiplayer list: reading `servers.dat`, editing it, and
//! saying when an edit is at risk.
//!
//! The file is the running game's, not ours ([`crate::minecraft::servers`]),
//! so a write while a session is open is reported as a degraded outcome rather
//! than refused — the edit is made, and the caller is told the game will
//! overwrite it when it exits.

use anyhow::{bail, Result};
use proto::error::{ErrorInfo, Field, Reason};
use proto::instance::ServerEntry;
use proto::server::ServerPingResult;
use proto::warning::WarningInfo;

use crate::engine::Engine;
use crate::instances::InstanceRecord;
use crate::minecraft::{ping, servers};

impl Engine {
    /// The instance's multiplayer list, in the order the game shows it.
    pub fn instance_servers(&self, reference: &str) -> Result<Vec<ServerEntry>> {
        let record = self.instances.require(reference)?;
        Ok(servers::read(&self.instances.data_dir(&record)))
    }

    /// Add an entry, or rewrite the one `target` names (by name or address).
    /// The address is validated here — the game stores whatever it is handed,
    /// and an unreachable-looking entry is far easier to explain now than as a
    /// launch that cannot connect later.
    pub fn edit_instance_server(
        &self,
        reference: &str,
        target: &str,
        entry: ServerEntry,
    ) -> Result<ServerListWrite> {
        let record = self.instances.require(reference)?;
        let name = entry.name.trim().to_string();
        if name.is_empty() {
            bail!(ErrorInfo::FieldRequired { field: Field::Name });
        }
        if ping::split_address(&entry.address).is_err() {
            bail!(ErrorInfo::InvalidValue {
                field: Field::Address,
                reason: Reason::ServerAddress,
            });
        }
        let data_dir = self.instances.data_dir(&record);
        let mut rows = servers::read_rows(&data_dir)?;
        let list = servers::entries(&rows);
        let entry = ServerEntry {
            name,
            address: entry.address.trim().to_string(),
            ..entry
        };
        match target.trim() {
            "" => rows.push(servers::overlay(servers::Row::new(), &entry)),
            target => {
                let index =
                    servers::find(&list, target).ok_or(ErrorInfo::ServerListEntryNotFound {
                        reference: target.to_string(),
                    })?;
                // The icon is the game's cache for that row, not something a
                // caller supplies, so an edit keeps whatever is already there.
                let entry = ServerEntry {
                    icon: list[index].icon.clone(),
                    ..entry
                };
                rows[index] = servers::overlay(std::mem::take(&mut rows[index]), &entry);
            }
        }
        self.commit_server_list(&record, rows)
    }

    pub fn remove_instance_server(&self, reference: &str, target: &str) -> Result<ServerListWrite> {
        let record = self.instances.require(reference)?;
        let data_dir = self.instances.data_dir(&record);
        let mut rows = servers::read_rows(&data_dir)?;
        let index = servers::find(&servers::entries(&rows), target).ok_or(
            ErrorInfo::ServerListEntryNotFound {
                reference: target.to_string(),
            },
        )?;
        rows.remove(index);
        self.commit_server_list(&record, rows)
    }

    /// Rewrite the list into the order `order` names — the arrangement a
    /// caller made against the rows it was shown, committed as one write.
    pub fn arrange_instance_servers(
        &self,
        reference: &str,
        order: &[String],
    ) -> Result<ServerListWrite> {
        let record = self.instances.require(reference)?;
        let data_dir = self.instances.data_dir(&record);
        let rows = servers::read_rows(&data_dir)?;
        let Some(permutation) = servers::rearrange(&servers::entries(&rows), order) else {
            bail!(ErrorInfo::InvalidValue {
                field: Field::Order,
                reason: Reason::ListOrder,
            });
        };
        let arranged = permutation.into_iter().map(|i| rows[i].clone()).collect();
        self.commit_server_list(&record, arranged)
    }

    /// The status of an arbitrary multiplayer address, over the same Server
    /// List Ping the in-game list uses.
    pub async fn ping_address(&self, address: &str) -> Result<ServerPingResult> {
        if ping::split_address(address).is_err() {
            bail!(ErrorInfo::InvalidValue {
                field: Field::Address,
                reason: Reason::ServerAddress,
            });
        }
        ping::ping_address(address).await
    }

    fn commit_server_list(
        &self,
        record: &InstanceRecord,
        rows: Vec<servers::Row>,
    ) -> Result<ServerListWrite> {
        servers::write_rows(&self.instances.data_dir(record), &rows)?;
        let servers = servers::entries(&rows);
        let sessions = self.running_sessions(&record.id);
        let warnings = if sessions > 0 {
            tracing::warn!(
                instance = %record.name,
                sessions,
                "multiplayer list edited while the instance is running"
            );
            vec![WarningInfo::ServerListInUse {
                instance: record.name.clone(),
                sessions,
            }]
        } else {
            Vec::new()
        };
        Ok(ServerListWrite { servers, warnings })
    }
}

/// The list as it now stands, plus what the write could not guarantee.
pub struct ServerListWrite {
    pub servers: Vec<ServerEntry>,
    pub warnings: Vec<WarningInfo>,
}
