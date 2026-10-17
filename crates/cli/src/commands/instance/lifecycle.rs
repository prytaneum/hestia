//! Launching an instance and driving it through the supervisor: launch, stop,
//! restart, remove, and its captured output.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use client::proto::error::{EntryKind, EntryTarget};
use client::proto::instance::{InstanceLaunchDone, QuickPlay, Replace};
use client::Client;

use super::entry;
use crate::ui::{self, MonitorSample, ProvisionReporter, Spinner, View};

/// Launch `reference`, rendering preparation progress, then follow its output
/// until it exits or Ctrl-C (unless detached or piped); shared with
/// `hestia play`.
pub async fn launch(client: &Client, options: Launch<'_>) -> Result<()> {
    let reference = options.reference;
    let detach = options.detach;
    let launched = launch_once(client, &options).await?;
    let process_id = launched.process_id;
    ui::show(View::line(format!(
        "instance '{reference}' launched (pid {})",
        launched.pid
    )))?;
    ui::show_warnings(&launched.warnings)?;
    if detach || !ui::interactive_output() {
        return Ok(());
    }
    let backfill = client
        .entry()
        .logs(
            &instance_target(reference),
            Some(process_id.clone()),
            Some(100),
        )
        .await
        .unwrap_or_default();
    for line in backfill {
        ui::show(View::line(line.line))?;
    }
    crate::commands::lifecycle::follow_logs(client, "instance", reference, &process_id, true).await
}

/// One launch's inputs. Named fields: `new_session`, `detach` and `offline` are
/// adjacent bools a positional call could swap without the compiler noticing.
#[derive(Default)]
pub struct Launch<'a> {
    pub reference: &'a str,
    pub account: &'a str,
    pub new_session: bool,
    pub detach: bool,
    pub quick_play: Option<QuickPlay>,
    pub offline: bool,
    pub replace: Option<Replace>,
}

/// Launch; on an `unauthorized` (expired sign-in) offer to re-auth and retry once.
async fn launch_once(client: &Client, options: &Launch<'_>) -> Result<InstanceLaunchDone> {
    let mut retried = false;
    loop {
        let reporter = Arc::new(ProvisionReporter::new());
        let progress = reporter.clone();
        let result = client
            .instance()
            .launch(
                client::LaunchOptions {
                    instance: options.reference,
                    account: options.account,
                    new_session: options.new_session,
                    quick_play: options.quick_play.clone(),
                    offline: options.offline,
                    replace: options.replace.clone(),
                },
                move |p| progress.update(p),
            )
            .await;
        reporter.finish();
        match result {
            Ok(launched) => return Ok(launched),
            Err(e) if is_unauthorized(&e) => {
                if retried || !ui::interactive_output() {
                    bail!(
                        "your Microsoft sign-in has expired; run `hestia account login`, then try again"
                    );
                }
                ui::show(View::note("your Microsoft sign-in has expired"))?;
                if !ui::confirm("Sign in again now?", "sign in", "cancel")? {
                    bail!("sign-in required to play");
                }
                crate::commands::account::reauth(client).await?;
                retried = true;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn is_unauthorized(err: &client::IpcError) -> bool {
    matches!(err, client::IpcError::Daemon { code, .. } if code == client::errors::UNAUTHORIZED)
}

pub(crate) async fn stop(client: &Client, instance: &str, session: Option<String>) -> Result<()> {
    let target = resolve_session(client, instance, &session).await?;
    {
        let _spinner = Spinner::start(format!("stopping '{instance}'"));
        client
            .entry()
            .stop(&instance_target(instance), target.clone())
            .await?;
    }
    match target {
        Some(id) => ui::show(View::line(format!(
            "instance '{instance}' session {} stopped",
            session_label(&id)
        ))),
        None => ui::show(View::line(format!("instance '{instance}' stopped"))),
    }
}

pub(crate) async fn restart(
    client: &Client,
    instance: &str,
    session: Option<String>,
    account: &str,
    detach: bool,
) -> Result<()> {
    let replace = match resolve_session(client, instance, &session).await? {
        Some(id) => Replace::One(id),
        None => Replace::All,
    };
    launch(
        client,
        Launch {
            reference: instance,
            account,
            detach,
            replace: Some(replace),
            ..Launch::default()
        },
    )
    .await
}

/// Resolve an optional `--session` handle to a full process id against the live
/// instance; `None` stays `None` (all sessions / newest).
async fn resolve_session(
    client: &Client,
    instance: &str,
    session: &Option<String>,
) -> Result<Option<String>> {
    match session {
        Some(input) => {
            let info = entry::fetch(client, instance).await?;
            Ok(Some(entry::resolve_session(&info, input)?))
        }
        None => Ok(None),
    }
}

fn instance_target(instance: &str) -> EntryTarget {
    EntryTarget::new(EntryKind::Instance, instance)
}

fn session_label(process_id: &str) -> &str {
    process_id.rsplit('_').next().unwrap_or(process_id)
}

/// Read (or follow) an instance's captured output. A named `--session` is one
/// process and its exit ends the follow; without one the subject is the
/// **instance**, so the stream survives a session ending and picks up the next
/// launch — and can be started against a stopped instance.
pub(crate) async fn logs(
    client: &Client,
    instance: &str,
    session: Option<String>,
    tail: Option<usize>,
    follow: bool,
) -> Result<()> {
    let target = resolve_session(client, instance, &session).await?;
    let lines = client
        .entry()
        .logs(&instance_target(instance), target.clone(), tail)
        .await?;
    if lines.is_empty() && !follow {
        return ui::show(View::note("no output captured (has it been launched?)"));
    }
    for line in lines {
        ui::show(View::line(line.line))?;
    }
    if !follow {
        return Ok(());
    }
    let info = entry::fetch(client, instance).await?;
    let key = target
        .clone()
        .unwrap_or_else(|| client::proto::naming::instance_process_id(&info.id));
    crate::commands::lifecycle::follow_logs(client, "instance", &info.name, &key, target.is_some())
        .await
}

/// Run the fullscreen resource monitor over one running session (named, else
/// the newest), filtering the daemon's metrics stream to it.
pub(crate) async fn monitor(
    client: &Client,
    instance: &str,
    session: Option<String>,
) -> Result<()> {
    let target = resolve_session(client, instance, &session).await?;
    let info = entry::fetch(client, instance).await?;
    let process_id = follow_target(&info, &target)?;

    let mut samples = client.process().subscribe_metrics().await?;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let forward = tokio::spawn(async move {
        while let Some(batch) = samples.recv().await {
            let sample = batch
                .into_iter()
                .find(|m| m.id == process_id)
                .map(|m| MonitorSample {
                    cpu_pct: m.cpu_pct,
                    mem_bytes: m.mem_bytes,
                });
            if tx.send(sample).is_err() {
                break;
            }
        }
    });

    let title = format!("{} — resources", info.name);
    let result = tokio::task::spawn_blocking(move || ui::monitor(&title, rx)).await?;
    forward.abort();
    result
}

/// The process id to follow: the named session, else the newest running one.
fn follow_target(
    info: &client::proto::instance::InstanceInfo,
    target: &Option<String>,
) -> Result<String> {
    match target {
        Some(id) => Ok(id.clone()),
        None => entry::running_process(info)
            .map(|p| p.id)
            .with_context(|| format!("instance '{}' is not running", info.name)),
    }
}
