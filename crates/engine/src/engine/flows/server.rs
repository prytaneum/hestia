//! Server provisioning, in-place version updates, and the launch plan.

use anyhow::{Context, Result};
use proto::backup::BackupKind;
use proto::minecraft::ProvisionPhase;

use proto::server::{JvmArgsSource, ServerDetails, ServerPingResult};
use proto::warning::WarningInfo;

use super::{effective_name, guard_downgrade, meta_dir, phase_progress};
use crate::content::install;
use crate::engine::{Engine, ServerCreateSpec, ServerUpdateSpec};
use crate::minecraft::launch::{JavaSettings, LaunchPlan};
use crate::minecraft::materialize::OnProgress;
use crate::minecraft::ping;
use crate::servers::ServerRecord;
use crate::usage;

impl Engine {
    /// Create a fully provisioned server: resolve the profile, register the
    /// record, ensure the Java runtime, and download its files. A failure after
    /// registration removes the record so nothing half-built is left behind.
    /// Returns the record with any degraded outcome of the pipeline — a create
    /// can succeed while a best-effort step did not.
    pub async fn provision_server(
        &self,
        spec: ServerCreateSpec,
        on_progress: OnProgress<'_>,
    ) -> Result<(ServerRecord, Vec<WarningInfo>)> {
        if !spec.eula {
            anyhow::bail!(proto::error::ErrorInfo::EulaRequired);
        }
        on_progress.report(&phase_progress(ProvisionPhase::Resolving));
        let profile = self
            .minecraft
            .resolve_server(&spec.flavor, &spec.version, spec.loader_version)
            .await?;
        let name = effective_name(&spec.name, &spec.flavor, &spec.version);
        // Before the record exists: a cancel up to here leaves nothing at all,
        // not even a port claim.
        on_progress.check()?;
        let record = self.servers.create(&name, profile, spec.port)?;

        // Config entries apply after provisioning so property keys validate
        // against the schema the server itself generated.
        let provisioned = async {
            let java = self
                .ensure_java(record.profile.java_major, on_progress)
                .await?;
            on_progress.check()?;
            self.servers
                .provision(&record, Some(&self.cache), on_progress)
                .await?;
            let data = self.servers.data_dir(&record);
            self.minecraft
                .install_server(
                    &record.profile.flavor,
                    &crate::minecraft::InstallRequest {
                        game_version: &record.profile.game_version,
                        loader_version: record.profile.loader_version.as_deref(),
                        root: &data,
                        meta: &meta_dir(&self.data_home()),
                        minecraft_jar: &data.join(&record.profile.primary.filename),
                        java: &java,
                        cache: Some(&self.cache),
                        processes: self.processes(),
                    },
                    on_progress,
                )
                .await?;
            self.servers
                .derive_properties_schema(&record, &java, self.processes(), on_progress)
                .await;
            for entry in &spec.config {
                self.servers
                    .config_set(&record.id, &entry.key, &entry.value)?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if provisioned.is_err() {
            let _ = self.servers.remove(&record.id);
        }
        provisioned?;
        let record = self.servers.mark_ready(&record.id)?;
        let warnings = self.server_warnings(&record);
        Ok((record, warnings))
    }

    /// The degraded state a server carries right now — what a create's warnings
    /// said, still true long after that output scrolled past.
    pub(crate) fn server_warnings(&self, record: &ServerRecord) -> Vec<WarningInfo> {
        if self.servers.has_schema(record) {
            return Vec::new();
        }
        vec![WarningInfo::PropertiesSchemaMissing {
            name: record.name.clone(),
        }]
    }

    /// Move a server to another version of its flavor. A downgrade must be
    /// allowed explicitly — Minecraft cannot load a world written by a newer
    /// version.
    pub async fn update_server(
        &self,
        spec: ServerUpdateSpec,
        on_progress: OnProgress<'_>,
    ) -> Result<(ServerRecord, Vec<WarningInfo>)> {
        let record = self
            .servers
            .get(&spec.server)
            .with_context(|| format!("unknown server: {}", spec.server))?;
        let versions = self
            .minecraft
            .server_versions(&record.profile.flavor)
            .await?;
        guard_downgrade(
            "world",
            &record.name,
            &record.profile.game_version,
            &spec.version,
            &versions,
            spec.allow_downgrade,
        )?;
        on_progress.report(&phase_progress(ProvisionPhase::Resolving));
        let profile = self
            .minecraft
            .resolve_server(&record.profile.flavor, &spec.version, spec.loader_version)
            .await?;
        if self.servers.data_dir(&record).is_dir() {
            on_progress.report(&phase_progress(ProvisionPhase::Backup));
            self.backup_server(&record.id, BackupKind::Update, false, on_progress)
                .await
                .context("pre-update backup failed")?;
        }
        let java = self.ensure_java(profile.java_major, on_progress).await?;
        // The pre-update backup is on disk by now, so a cancel here costs
        // nothing: the server is still on its old version.
        on_progress.check()?;
        let record = self
            .servers
            .update(&record.id, profile, Some(&self.cache), on_progress)
            .await?;
        let data = self.servers.data_dir(&record);
        self.minecraft
            .install_server(
                &record.profile.flavor,
                &crate::minecraft::InstallRequest {
                    game_version: &record.profile.game_version,
                    loader_version: record.profile.loader_version.as_deref(),
                    root: &data,
                    meta: &meta_dir(&self.data_home()),
                    minecraft_jar: &data.join(&record.profile.primary.filename),
                    java: &java,
                    cache: Some(&self.cache),
                    processes: self.processes(),
                },
                on_progress,
            )
            .await?;
        self.servers
            .derive_properties_schema(&record, &java, self.processes(), on_progress)
            .await;
        let warnings = self.server_warnings(&record);
        Ok((record, warnings))
    }

    /// The ready-to-spawn invocation for a provisioned server, with its ports
    /// reconciled into `server.properties`.
    pub fn server_launch_plan(&self, reference: &str) -> Result<(ServerRecord, LaunchPlan)> {
        let record = self
            .servers
            .get(reference)
            .with_context(|| format!("unknown server: {reference}"))?;
        if !record.ready() {
            anyhow::bail!("server '{}' is still provisioning", record.name);
        }
        let record = self.servers.ensure_start_config(&record.id)?;
        let data_dir = self.servers.data_dir(&record);
        install::sync(
            &self.servers.server_dir(&record),
            &data_dir,
            &[crate::servers::level_name(&data_dir)],
        )?;
        let java = self.installed_java(record.profile.java_major)?;
        let jvm = record
            .jvm
            .or_defaults(&self.config.settings().java_defaults())
            .or_defaults(&flavor_defaults(&record.profile));
        let plan = self.servers.launch_plan(&record, &java, &jvm);
        Ok((record, plan))
    }

    pub async fn server_ping(&self, reference: &str) -> Result<ServerPingResult> {
        let record = self
            .servers
            .get(reference)
            .with_context(|| format!("unknown server: {reference}"))?;
        let port = record
            .game_port
            .context("this server has no game port allocated")?;
        ping::ping(port).await
    }

    /// The server's static, informational view: descriptor, locations, and the
    /// on-disk footprint (a directory walk).
    pub fn server_detail(&self, reference: &str) -> Result<ServerDetails> {
        let record = self
            .servers
            .get(reference)
            .with_context(|| format!("unknown server: {reference}"))?;
        let entry_dir = self.servers.server_dir(&record);
        let data_dir = self.servers.data_dir(&record);
        let warnings = self.server_warnings(&record);
        let defaults = self.config.settings().java_defaults();
        let jvm_args_source = jvm_args_source(&record.jvm, &defaults, &record.profile.jvm_args);
        let jvm_args = record
            .jvm
            .or_defaults(&defaults)
            .or_defaults(&flavor_defaults(&record.profile))
            .jvm_args;
        Ok(ServerDetails {
            id: record.id,
            name: record.name,
            flavor: record.profile.flavor,
            game_version: record.profile.game_version,
            loader_version: record.profile.loader_version,
            java_major: record.profile.java_major,
            created_unix: record.created_unix,
            game_port: record.game_port,
            disk_bytes: usage::dir_size(&entry_dir),
            entry_dir: entry_dir.to_string_lossy().into_owned(),
            data_dir: data_dir.to_string_lossy().into_owned(),
            jvm_args,
            jvm_args_source,
            warnings,
        })
    }
}

/// The flavor's own recommended JVM flags as the last fallback layer, beneath
/// the entry's `jvm-args` and the launcher-wide `defaults.jvm-args`. Paper
/// publishes a tuned G1GC set per version and running without it is a
/// measurably worse server, but a user who wrote their own flags meant them —
/// so this only fills what neither of the two settings layers did, which is
/// exactly what `or_defaults` means.
fn flavor_defaults(profile: &proto::minecraft::ServerProfile) -> JavaSettings {
    JavaSettings {
        memory: None,
        jvm_args: profile.jvm_args.clone(),
    }
}

/// Which layer supplies the flags a server will actually start with — the same
/// order `or_defaults` resolves them in, reported so `info` can say it.
fn jvm_args_source(
    entry: &JavaSettings,
    defaults: &JavaSettings,
    flavor: &[String],
) -> JvmArgsSource {
    if !entry.jvm_args.is_empty() {
        JvmArgsSource::Entry
    } else if !defaults.jvm_args.is_empty() {
        JvmArgsSource::Defaults
    } else if !flavor.is_empty() {
        JvmArgsSource::Flavor
    } else {
        JvmArgsSource::None
    }
}

#[cfg(test)]
mod tests {
    use proto::error::ErrorInfo;

    use super::*;

    #[tokio::test]
    async fn a_create_without_eula_acceptance_is_refused_before_any_work() {
        let home = tempfile::tempdir().expect("temp home");
        let engine = Engine::new(Some(home.path()));
        let cancel = crate::Cancel::new();
        let report = |_: &proto::minecraft::ProvisionProgress| {};
        let job = crate::Job::new(&report, &cancel);

        let refused = engine
            .provision_server(
                ServerCreateSpec {
                    name: "smp".into(),
                    flavor: "vanilla".into(),
                    version: "1.21".into(),
                    eula: false,
                    ..ServerCreateSpec::default()
                },
                &job as OnProgress<'_>,
            )
            .await
            .expect_err("no eula, no server");

        assert!(matches!(
            crate::error_info(refused),
            ErrorInfo::EulaRequired
        ));
        assert!(
            engine.servers().list().is_empty(),
            "the refusal must land before the record is registered"
        );
    }

    fn settings(args: &[&str]) -> JavaSettings {
        JavaSettings {
            memory: None,
            jvm_args: args.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn flavor(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_innermost_set_layer_wins() {
        assert_eq!(
            jvm_args_source(&settings(&[]), &settings(&[]), &flavor(&["-XX:+UseG1GC"])),
            JvmArgsSource::Flavor,
            "nothing set, so paper's own recommendation runs"
        );
        assert_eq!(
            jvm_args_source(
                &settings(&[]),
                &settings(&["-Xss1M"]),
                &flavor(&["-XX:+UseG1GC"])
            ),
            JvmArgsSource::Defaults,
            "a launcher-wide default outranks the flavor's"
        );
        assert_eq!(
            jvm_args_source(
                &settings(&["-Xss2M"]),
                &settings(&["-Xss1M"]),
                &flavor(&["-XX:+UseG1GC"])
            ),
            JvmArgsSource::Entry,
            "a user who wrote flags meant them"
        );
        assert_eq!(
            jvm_args_source(&settings(&[]), &settings(&[]), &flavor(&[])),
            JvmArgsSource::None
        );
    }

    #[test]
    fn a_flavor_recommends_flags_but_never_memory() {
        let profile = proto::minecraft::ServerProfile {
            jvm_args: flavor(&["-XX:+UseG1GC"]),
            ..proto::minecraft::ServerProfile::default()
        };
        let resolved = settings(&[])
            .or_defaults(&settings(&[]))
            .or_defaults(&flavor_defaults(&profile));
        assert_eq!(
            resolved.flags(),
            ["-XX:+UseG1GC"],
            "memory stays the user's call; the flavor only recommends flags"
        );
    }
}
