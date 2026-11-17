//! What the host asks an enabled WASM plugin to do after it started: run a
//! command, tick a schedule, hear that a job it started ended.

use std::sync::Arc;

use proto::plugin::PluginState;
use tokio::task::JoinSet;

use super::guests::Guest;
use super::{find, Kind, Plugins};
use crate::commands::CommandDecl;
use crate::error::PluginError;
use crate::runtime::{Invocation, JobOutcome, Loaded, SandboxError};

impl Plugins {
    /// Whether `id` is an enabled plugin that declares `command` and `args`
    /// fit it, so a job is only started for a call that can run.
    pub fn check_command(&self, id: &str, command: &str, args: &str) -> Result<(), PluginError> {
        self.ready(id, command)
            .and_then(|(_, _, decl)| decl.check(args).map_err(PluginError::Arguments))
    }

    /// Runs the command for up to its budget. While it runs the plugin cannot
    /// be disabled or removed; one that is stopped anyway (developer mode off,
    /// its trap limit, the daemon stopping) ends the call. A failure that is the
    /// plugin's last allowed trap disables it.
    pub async fn run_command(
        &self,
        id: &str,
        command: &str,
        args: &str,
        invocation: Invocation,
    ) -> Result<(), PluginError> {
        let mut running = self.running.enter(id);
        let (guest, loaded, decl) = self.ready(id, command)?;
        decl.check(args).map_err(PluginError::Arguments)?;
        tracing::info!(
            plugin = id,
            command,
            person = invocation.person,
            "command starting"
        );
        let sandbox = self.sandbox()?;
        let outcome = tokio::select! {
            outcome = sandbox.command(&loaded, command, args, invocation) => outcome,
            () = running.halted() => Err(SandboxError::Stopped),
        };
        self.settled(&guest.id, outcome)
    }

    /// Fills the test built-in's extension point through the enabled plugin
    /// `id`, as the router would call it.
    #[cfg(feature = "probe")]
    pub async fn fill(&self, id: &str, text: &str) -> Result<Option<String>, PluginError> {
        let (guest, loaded) = self
            .running(id)
            .ok_or_else(|| PluginError::NotEnabled(id.to_owned()))?;
        self.sandbox()?
            .fill(&loaded, crate::api::Chain::default(), text)
            .await
            .map_err(|source| PluginError::Call {
                plugin: guest.id.clone(),
                source,
            })
    }

    /// Ends every running command, for a daemon that is stopping.
    pub fn stop_commands(&self) {
        self.running.halt_all();
    }

    /// Tells the plugin a job it started ended. Never fails the caller: the
    /// job is over whatever the plugin makes of it.
    pub async fn job_done(&self, id: &str, job: &str, outcome: &JobOutcome) {
        let Some((guest, loaded)) = self.running(id) else {
            return;
        };
        let Ok(sandbox) = self.sandbox() else { return };
        let told = sandbox.job_done(&loaded, &job.to_owned(), outcome).await;
        if let Err(error) = self.settled(&guest.id, told) {
            tracing::warn!(plugin = id, job, %error, "plugin failed on-job-done");
        }
    }

    /// Ticks every schedule of an enabled plugin that has come due at `now`
    /// (unix seconds). Plugins tick side by side, a plugin's own schedules one
    /// after another; a tick that finds the plugin out of wake-ups, disabled or
    /// stopped meanwhile is skipped.
    pub async fn run_due(self: &Arc<Self>, now: u64) {
        let mut ticking = JoinSet::new();
        for guest in self.listening() {
            let plugins = self.clone();
            ticking.spawn(async move { plugins.tick_due(&guest, now).await });
        }
        while ticking.join_next().await.is_some() {}
    }

    async fn tick_due(&self, guest: &Guest, now: u64) {
        let Ok(sandbox) = self.sandbox() else { return };
        for schedule in self.due_schedules(guest, now) {
            let Some((guest, loaded)) = self.running(&guest.id) else {
                return;
            };
            if !loaded.provides_on_schedule() {
                return;
            }
            if !sandbox.spend_wakeup(&guest.id) {
                tracing::warn!(
                    plugin = guest.id,
                    schedule,
                    "no wake-ups left; tick skipped"
                );
                continue;
            }
            tracing::debug!(plugin = guest.id, schedule, "schedule ticking");
            let told = sandbox.tick(&loaded, &schedule).await;
            if let Err(error) = self.settled(&guest.id, told) {
                tracing::warn!(plugin = guest.id, schedule, %error, "plugin failed on-schedule");
            }
        }
    }

    pub(super) fn running(&self, id: &str) -> Option<(Arc<Guest>, Loaded)> {
        let slots = self.slots();
        slots.iter().find_map(|slot| match &slot.kind {
            Kind::Guest(guest) if guest.id == id && slot.state == PluginState::Enabled => {
                guest.loaded.clone().map(|loaded| (guest.clone(), loaded))
            }
            _ => None,
        })
    }

    fn listening(&self) -> Vec<Arc<Guest>> {
        let slots = self.slots();
        slots
            .iter()
            .filter_map(|slot| match &slot.kind {
                Kind::Guest(guest)
                    if slot.state == PluginState::Enabled
                        && guest
                            .loaded
                            .as_ref()
                            .is_some_and(Loaded::provides_on_schedule) =>
                {
                    Some(guest.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn ready(
        &self,
        id: &str,
        command: &str,
    ) -> Result<(Arc<Guest>, Loaded, CommandDecl), PluginError> {
        let slots = self.slots();
        let slot = &slots[find(&slots, id)?];
        let Kind::Guest(guest) = &slot.kind else {
            return Err(PluginError::NoCommand {
                plugin: id.to_owned(),
                command: command.to_owned(),
            });
        };
        let loaded = match (&guest.loaded, slot.state) {
            (Some(loaded), PluginState::Enabled) => loaded.clone(),
            _ => return Err(PluginError::NotEnabled(id.to_owned())),
        };
        let decl = guest
            .commands
            .iter()
            .find(|decl| decl.name == command)
            .cloned()
            .ok_or_else(|| PluginError::NoCommand {
                plugin: id.to_owned(),
                command: command.to_owned(),
            })?;
        Ok((guest.clone(), loaded, decl))
    }

    /// A call that reached the trap limit leaves the plugin disabled, as an
    /// `init` that does, and ends the commands it still has running.
    pub(super) fn settled(
        &self,
        id: &str,
        outcome: Result<(), SandboxError>,
    ) -> Result<(), PluginError> {
        let Err(source) = outcome else { return Ok(()) };
        if matches!(source, SandboxError::TrapLimit(_)) {
            let mut slots = self.slots();
            if let Ok(index) = find(&slots, id) {
                slots[index].state = PluginState::Disabled;
                if let Err(error) = self.save(&slots) {
                    tracing::warn!(plugin = id, %error, "cannot save a plugin disabled for its traps");
                }
            }
            self.running.halt(id);
            tracing::warn!(plugin = id, "plugin disabled after reaching its trap limit");
        }
        Err(PluginError::Call {
            plugin: id.to_owned(),
            source,
        })
    }
}
