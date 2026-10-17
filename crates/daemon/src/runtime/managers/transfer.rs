//! Instance import and export jobs.
//!
//! An export is keyed by the instance it reads, so it queues behind the same
//! in-flight claim a backup or content job takes — archiving a tree while
//! something else rewrites it would produce an archive of neither state. An
//! import has no entry to key on yet (that is what it creates), so it takes no
//! claim, exactly as a modpack install that creates its entry does.

use std::sync::Arc;

use engine::Engine;
use proto::transfer::{ExportDone, ExportFormat, ImportDone, InstanceExport, InstanceImport};

use super::job::{Cancellations, Runner, Spec};
use crate::runtime::{instance_info, instance_process_id, EventHub};

/// One import or export — what `TransferManager::start` runs off-thread.
pub enum TransferJob {
    Export {
        instance_id: String,
        format: ExportFormat,
        destination: String,
        exclude: Vec<String>,
    },
    Import {
        path: String,
        name: String,
    },
}

pub struct TransferManager {
    runner: Runner<String>,
}

impl TransferManager {
    pub fn new(engine: Arc<Engine>, hub: Arc<EventHub>, cancellations: Cancellations) -> Self {
        TransferManager {
            runner: Runner::new(engine, hub, cancellations),
        }
    }

    /// Whether a transfer is still running for this entry key
    /// (`instance-<id>`).
    pub fn in_flight(&self, key: &str) -> bool {
        self.runner.in_flight(key)
    }

    /// Start a transfer off-thread. Returns the job id, or `None` when that
    /// entry is already busy.
    pub fn start(&self, job: TransferJob, id: String) -> Option<String> {
        match job {
            TransferJob::Export {
                instance_id,
                format,
                destination,
                exclude,
            } => self.export(id, instance_id, format, destination, exclude),
            TransferJob::Import { path, name } => self.import(id, path, name),
        }
    }

    fn export(
        &self,
        id: String,
        instance_id: String,
        format: ExportFormat,
        destination: String,
        exclude: Vec<String>,
    ) -> Option<String> {
        let spec = Spec {
            id,
            prefix: "instance-export",
            key: Some(instance_process_id(&instance_id)),
        };

        self.runner
            .start::<InstanceExport, _>(spec, move |engine, reporter| {
                Box::pin(async move {
                    let export = engine.export_instance(
                        &instance_id,
                        format,
                        &destination,
                        &exclude,
                        &reporter.job(),
                    )?;
                    Ok(ExportDone {
                        path: export.path.to_string_lossy().into_owned(),
                        size_bytes: export.size_bytes,
                        files: export.files,
                        warnings: export.warnings,
                    })
                })
            })
    }

    fn import(&self, id: String, path: String, name: String) -> Option<String> {
        let spec = Spec {
            id,
            prefix: "instance-import",
            key: None,
        };

        self.runner
            .start::<InstanceImport, _>(spec, move |engine, reporter| {
                Box::pin(async move {
                    let import = engine
                        .import_instance(&path, &name, &reporter.job())
                        .await?;
                    let accepts = engine.instance_accepts(&import.record.profile.flavor);
                    // The view an import answers with is the same one
                    // `instance.list` returns, so a front-end can drop it
                    // straight into the library it already renders. A
                    // just-imported instance has no sessions by definition.
                    let instance = instance_info(import.record, Vec::new(), accepts);
                    Ok(ImportDone {
                        format: import.format,
                        instance,
                        failures: import.failures,
                        warnings: import.warnings,
                    })
                })
            })
    }
}
