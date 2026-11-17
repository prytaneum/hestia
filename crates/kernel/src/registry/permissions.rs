//! What the user decided within a grant, after approval: optional permissions
//! granted or revoked, and the entries a plugin may reach.

use std::sync::Arc;

use proto::error::IoOp;
use proto::plugin::{EntryScope, PluginInfo};

use super::guests::Guest;
use super::{find, info, Kind, Plugins};
use crate::entries::Entries;
use crate::error::{PackageError, PluginError};
use crate::grant::GrantError;

impl Plugins {
    /// Grants or revokes an optional permission of an installed WASM plugin and
    /// relinks it at once: a revoked interface answers `denied` from the next
    /// call. A revoke narrows, so it halts the plugin's running commands; a grant
    /// is refused while it has work running. A call already in flight keeps the
    /// link it began with until it ends.
    pub async fn set_permission(
        &self,
        id: &str,
        permission: &str,
        on: bool,
    ) -> Result<PluginInfo, PluginError> {
        let _changing = self.changes.lock().await;
        let old = self.listed(id)?;
        let record = self
            .store
            .record(id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        if !record
            .grant
            .permissions
            .iter()
            .any(|granted| granted.optional && granted.permission == permission)
        {
            return Err(PackageError::Grant(GrantError::NotOptional(permission.to_owned())).into());
        }
        let before = record.choice.optional;
        if before.iter().any(|held| held == permission) != on {
            if on && self.running.any(id) {
                return Err(PluginError::Busy(id.to_owned()));
            }
            let mut after = before.clone();
            match on {
                true => after.push(permission.to_owned()),
                false => after.retain(|held| held != permission),
            }
            self.save_optional(id, after).await?;
            if !on {
                self.running.halt(id);
            }
            match self.load_guest(id, Some(old.entries.clone())).await {
                Ok(guest) => self.place(id, Arc::new(guest), None)?,
                Err(error) => {
                    self.save_optional(id, before).await?;
                    return Err(error);
                }
            }
            tracing::info!(
                plugin = id,
                permission,
                granted = on,
                "optional permission changed"
            );
        }
        let slots = self.slots();
        Ok(info(&slots[find(&slots, id)?]))
    }

    /// Replaces the entries an installed WASM plugin may reach. The plugin keeps
    /// its link: the scope is read at each call.
    pub async fn set_entries(
        &self,
        id: &str,
        scope: EntryScope,
    ) -> Result<PluginInfo, PluginError> {
        let _changing = self.changes.lock().await;
        let guest = self.listed(id)?;
        guest.entries.set(scope);
        self.save_entries(id, &guest.entries).await?;
        tracing::info!(plugin = id, "entries scope changed");
        let slots = self.slots();
        Ok(info(&slots[find(&slots, id)?]))
    }

    async fn save_optional(&self, id: &str, optional: Vec<String>) -> Result<(), PluginError> {
        let (store, id) = (self.store.clone(), id.to_owned());
        tokio::task::spawn_blocking(move || {
            store.amend(&id, |record| record.choice.optional = optional)
        })
        .await
        .map_err(|error| PluginError::io(IoOp::Write, error))?
    }

    /// Keeps `entries` as they are now: read inside the store's lock, so a
    /// write that lost the race still saves what the others added.
    async fn save_entries(&self, id: &str, entries: &Entries) -> Result<(), PluginError> {
        let (store, id, entries) = (self.store.clone(), id.to_owned(), entries.clone());
        tokio::task::spawn_blocking(move || {
            store.amend(&id, |record| record.choice.entries = entries.get())
        })
        .await
        .map_err(|error| PluginError::io(IoOp::Write, error))?
    }

    fn listed(&self, id: &str) -> Result<Arc<Guest>, PluginError> {
        let slots = self.slots();
        match &slots[find(&slots, id)?].kind {
            Kind::Guest(guest) => Ok(guest.clone()),
            Kind::Builtin(_) => Err(PluginError::Builtin(id.to_owned())),
        }
    }

    fn guest(&self, plugin: &str) -> Option<Arc<Guest>> {
        self.slots().iter().find_map(|slot| match &slot.kind {
            Kind::Guest(guest) if guest.id == plugin => Some(guest.clone()),
            _ => None,
        })
    }

    /// The first of `touched` (stable ids) that lies outside the `entries` scope
    /// of `plugin`. A plugin the registry does not list reaches no entry.
    pub fn refused_entry(&self, plugin: &str, touched: &[String]) -> Option<String> {
        match self.guest(plugin) {
            Some(guest) => guest.entries.refused(touched).map(str::to_owned),
            None => touched.first().cloned(),
        }
    }

    /// Adds entries `plugin` created to its scope, and keeps them across a
    /// restart.
    pub async fn entries_created(&self, plugin: &str, created: &[String]) {
        let Some(guest) = self.guest(plugin) else {
            return;
        };
        guest.entries.add(created);
        if let Err(error) = self.save_entries(plugin, &guest.entries).await {
            tracing::warn!(plugin, %error, "cannot keep the entries the plugin created");
        }
    }
}
