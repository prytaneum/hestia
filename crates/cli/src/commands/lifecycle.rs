//! Top-level lifecycle shortcuts: `hestia start|stop|restart|logs|rename <name>`.
//!
//! A server and an instance are driven the same way day to day, but they live
//! in separate registries with different verbs (`server start` vs `instance
//! launch`). These verb-first shortcuts resolve a name across both so the
//! common actions do not force the caller to first recall which kind a name is.

use anyhow::{bail, Result};
use client::proto::error::{EntryKind, EntryTarget};
use client::{Client, ProcessEvent};

use super::{connect, instance, mc, server};
use crate::ui::{self, Spinner, View};

/// Resolve a name (or id) to the single server or instance it identifies,
/// erroring when it matches both or neither.
async fn resolve(client: &Client, name: &str) -> Result<EntryKind> {
    let servers = client.server().list().await?;
    let is_server = mc::find_by_reference(servers, name, |s| (&s.id, &s.name)).is_some();
    let instances = client.instance().list().await?;
    let is_instance = mc::find_by_reference(instances, name, |i| (&i.id, &i.name)).is_some();
    match (is_server, is_instance) {
        (true, false) => Ok(EntryKind::Server),
        (false, true) => Ok(EntryKind::Instance),
        (true, true) => bail!(
            "'{name}' names both a server and an instance; \
             use `hestia server {name} …` or `hestia instance {name} …`"
        ),
        (false, false) => bail!("no server or instance matches '{name}'"),
    }
}

pub async fn start(
    name: String,
    account: Option<String>,
    detach: bool,
    offline: bool,
) -> Result<()> {
    let client = connect().await?;
    match resolve(&client, &name).await? {
        EntryKind::Server => server::console::start_attached(client, &name, detach).await,
        EntryKind::Instance => {
            instance::launch(
                &client,
                instance::Launch {
                    reference: &name,
                    account: account.as_deref().unwrap_or_default(),
                    detach,
                    offline,
                    ..instance::Launch::default()
                },
            )
            .await
        }
    }
}

pub async fn stop(name: String, session: Option<String>) -> Result<()> {
    let client = connect().await?;
    match resolve(&client, &name).await? {
        EntryKind::Server => {
            reject_server_session(&session)?;
            server::lifecycle::stop(&client, &name).await
        }
        EntryKind::Instance => instance::lifecycle::stop(&client, &name, session).await,
    }
}

pub async fn restart(
    name: String,
    session: Option<String>,
    account: Option<String>,
    detach: bool,
) -> Result<()> {
    let client = connect().await?;
    match resolve(&client, &name).await? {
        EntryKind::Server => {
            reject_server_session(&session)?;
            server::console::restart_attached(client, &name, detach).await
        }
        EntryKind::Instance => {
            instance::lifecycle::restart(
                &client,
                &name,
                session,
                account.as_deref().unwrap_or_default(),
                detach,
            )
            .await
        }
    }
}

pub async fn logs(
    name: String,
    session: Option<String>,
    tail: Option<usize>,
    follow: bool,
) -> Result<()> {
    let client = connect().await?;
    match resolve(&client, &name).await? {
        EntryKind::Server => {
            reject_server_session(&session)?;
            server::lifecycle::logs(&client, &name, tail, follow).await
        }
        EntryKind::Instance => {
            instance::lifecycle::logs(&client, &name, session, tail, follow).await
        }
    }
}

/// A server runs a single process, so `--session` is meaningless for one.
fn reject_server_session(session: &Option<String>) -> Result<()> {
    if session.is_some() {
        bail!("--session applies to instances only; a server runs a single process");
    }
    Ok(())
}

pub async fn rename(name: String, new_name: String) -> Result<()> {
    let client = connect().await?;
    let kind = resolve(&client, &name).await?;
    rename_entry(&client, EntryTarget::new(kind, name), &new_name).await
}

pub(crate) async fn rename_entry(
    client: &Client,
    target: EntryTarget,
    new_name: &str,
) -> Result<()> {
    let renamed = {
        let _spinner = Spinner::start(format!("renaming '{}'", target.entry));
        client.entry().rename(&target, new_name).await?
    };
    ui::show(View::line(format!(
        "{} '{}' renamed to '{}' (id {})",
        target.kind, target.entry, renamed.name, renamed.id
    )))
}

pub(crate) async fn remove_entry(client: &Client, target: EntryTarget) -> Result<()> {
    {
        let _spinner = Spinner::start(format!("removing '{}'", target.entry));
        client.entry().remove(&target).await?;
    }
    ui::show(View::line(format!(
        "{} '{}' removed",
        target.kind, target.entry
    )))
}

/// Stream a process's output until Ctrl-C; a stop ends it only when `one_session`.
pub(crate) async fn follow_logs(
    client: &Client,
    noun: &str,
    name: &str,
    key: &str,
    one_session: bool,
) -> Result<()> {
    let mut events = client.process().subscribe(key).await?;
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(ProcessEvent::Output(line)) => ui::show(View::line(line.line))?,
                Some(ProcessEvent::Started(e)) => ui::show(View::note(format!(
                    "{noun} '{name}' started (pid {})",
                    e.pid
                )))?,
                Some(ProcessEvent::Exit(_)) => {
                    ui::show(View::note(format!("{noun} '{name}' stopped")))?;
                    if one_session {
                        return Ok(());
                    }
                }
                None => bail!("connection to the daemon lost"),
            },
            _ = tokio::signal::ctrl_c() => {
                return ui::show(View::note(format!("stopped following {noun} '{name}'")));
            }
        }
    }
}
