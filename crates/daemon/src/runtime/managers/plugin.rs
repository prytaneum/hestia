use std::pin::pin;
use std::sync::Arc;

use kernel::{Invocation, JobOutcome, Plugins};
use proto::error::ErrorInfo;
use proto::plugin::{PluginRun, PluginRunDone, PluginRunProgress};

use super::job::{generate_id, job_id, Coalescible, Jobs, Spec};
use crate::runtime::Surface;

impl Coalescible for PluginRunProgress {
    type Phase = ();
    fn phase(&self) {}
    fn ratio(&self) -> f64 {
        match self.total {
            0 => 0.0,
            total => self.current as f64 / total as f64,
        }
    }
}

/// One command of one plugin, already checked against its declaration.
pub struct PluginCommand {
    /// The caller's job id; empty generates one naming the plugin.
    pub id: String,
    pub plugin: String,
    pub command: String,
    pub args: String,
    /// Whether a person asked for it, as against the plugin itself through
    /// `jobs.start`; only the plugin hears when its own job ends.
    pub person: bool,
}

impl Jobs {
    /// Runs the command as a job: visible in the job list, cancellable, its
    /// progress and everything it shows tagged with the job id. Returns the id.
    ///
    /// A cancel is cooperative first: the plugin sees it through
    /// `progress.cancelled`, and one that has not stopped when its grace runs
    /// out is dropped. The job then reports `cancelled`; whatever the plugin
    /// wrote to `/data` stays, and the locks it held are released.
    pub fn start_plugin_command(
        &self,
        plugins: Arc<Plugins>,
        surface: Arc<Surface>,
        command: PluginCommand,
    ) -> String {
        let id = match command.id.is_empty() {
            true => generate_id(&format!("plugin-{}", command.plugin)),
            false => job_id(command.id.clone(), "plugin"),
        };
        let spec = Spec {
            id: id.clone(),
            prefix: "plugin",
            claim: None,
            subject: None,
        };
        let PluginCommand {
            plugin,
            command,
            args,
            person,
            ..
        } = command;

        self.run::<PluginRun, _>(spec, move |_engine, reporter| {
            Box::pin(async move {
                let mut reports = surface.watch_progress(&id);
                let invocation = Invocation {
                    person,
                    job: Some(id.clone()),
                    cancelled: reporter.cancel().flag(),
                };
                let report = reporter.checked();
                let result = {
                    let mut called =
                        pin!(plugins.run_command(&plugin, &command, &args, invocation));
                    loop {
                        tokio::select! {
                            result = &mut called => break result,
                            Some(update) = reports.changed() => { let _ = report(&update); }
                        }
                    }
                };
                if let Some(update) = reports.unseen() {
                    let _ = report(&update);
                }

                let cancelled = reporter.cancel().is_cancelled();
                let outcome = match (&result, cancelled) {
                    (Ok(()), _) => JobOutcome::Succeeded,
                    (Err(_), true) => JobOutcome::Cancelled,
                    (Err(error), false) => JobOutcome::Failed(error.to_string()),
                };
                if !person {
                    tokio::spawn(async move { plugins.job_done(&plugin, &id, &outcome).await });
                }
                match result {
                    Ok(()) => Ok(PluginRunDone {}),
                    Err(_) if cancelled => Err(engine::Cancelled.into()),
                    Err(error) => Err(anyhow::Error::new(ErrorInfo::from(error))),
                }
            })
        })
    }
}
