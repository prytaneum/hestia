//! Instance import and export jobs.
//!
//! An export holds the claim of the instance it reads, so nothing rewrites the
//! tree while it is archived — an archive of neither state would result. An
//! import has no entry to claim yet (that is what it creates), exactly as a
//! modpack install that creates its entry has none.

use proto::error::EntryKind;
use proto::transfer::{ExportDone, ExportFormat, ImportDone, InstanceExport, InstanceImport};

use super::job::{Jobs, Spec};
use crate::runtime::{instance_info, Claim};

impl Jobs {
    pub fn start_export(
        &self,
        id: String,
        claim: Claim,
        instance_id: String,
        format: ExportFormat,
        destination: String,
        exclude: Vec<String>,
    ) -> String {
        let spec = Spec {
            id,
            prefix: "instance-export",
            claim: Some(claim),
            subject: Spec::subject(EntryKind::Instance, &instance_id),
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

    pub fn start_import(&self, id: String, path: String, name: String) -> String {
        let spec = Spec {
            id,
            prefix: "instance-import",
            claim: None,
            subject: None,
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
