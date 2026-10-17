//! Instance import and export jobs.
//!
//! An export holds the claim of the instance it reads, so nothing rewrites the
//! tree while it is archived — an archive of neither state would result. An
//! import has no entry to claim yet (that is what it creates), exactly as a
//! modpack install that creates its entry has none.

use proto::transfer::{ExportDone, ExportFormat, ImportDone, InstanceExport, InstanceImport};

use super::job::{Jobs, Spec};
use crate::runtime::{instance_info, Claim};

/// One import or export — what `Jobs::start_transfer` runs off-thread.
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

impl Jobs {
    /// Start a transfer off-thread; an export carries the instance's claim.
    pub fn start_transfer(&self, job: TransferJob, id: String, claim: Option<Claim>) -> String {
        match job {
            TransferJob::Export {
                instance_id,
                format,
                destination,
                exclude,
            } => self.export(id, claim, instance_id, format, destination, exclude),
            TransferJob::Import { path, name } => self.import(id, path, name),
        }
    }

    fn export(
        &self,
        id: String,
        claim: Option<Claim>,
        instance_id: String,
        format: ExportFormat,
        destination: String,
        exclude: Vec<String>,
    ) -> String {
        let spec = Spec {
            id,
            prefix: "instance-export",
            claim,
        };

        self.run::<InstanceExport, _>(spec, move |engine, reporter| {
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

    fn import(&self, id: String, path: String, name: String) -> String {
        let spec = Spec {
            id,
            prefix: "instance-import",
            claim: None,
        };

        self.run::<InstanceImport, _>(spec, move |engine, reporter| {
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
