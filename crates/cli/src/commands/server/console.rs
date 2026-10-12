//! The server console over RCON: `attach` (interactive) and the one-shot
//! `command`'s reply rendering.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use client::{Client, ProcessEvent};

use super::entry;
use crate::ui::{self, View};

/// Start the server, then attach its console. Detached (or piped) it prints
/// the started line and returns; attaching prints nothing until the session
/// ends — output written between the shell prompt and the alternate screen
/// gets duplicated into scrollback by some terminals.
pub(crate) async fn start_attached(client: Client, server: &str, detach: bool) -> Result<()> {
    if detach || !ui::interactive_output() {
        return super::lifecycle::start(&client, server).await;
    }
    super::lifecycle::start_quiet(&client, server).await?;
    super::lifecycle::wait_until_running(&client, server).await?;
    attach(client, server).await
}

/// Stop, start again, and attach — the attach-path twin of
/// `lifecycle::restart`.
pub(crate) async fn restart_attached(client: Client, server: &str, detach: bool) -> Result<()> {
    if detach || !ui::interactive_output() {
        return super::lifecycle::restart(&client, server).await;
    }
    super::lifecycle::restart_quiet(&client, server).await?;
    super::lifecycle::wait_until_running(&client, server).await?;
    attach(client, server).await
}

/// Attach an interactive console to a running server: its live output above
/// an input line; Esc detaches without touching the server.
pub(crate) async fn attach(client: Client, server: &str) -> Result<()> {
    if !ui::is_interactive() {
        bail!("attach needs an interactive terminal (use `server logs -f` and `server command`)");
    }
    let info = client.server().status(server).await?;
    let process = entry::running_process(&info)
        .with_context(|| format!("server '{}' is not running", info.name))?;
    let backfill = client
        .server()
        .logs(&info.id, Some(100))
        .await?
        .into_iter()
        .map(|l| l.line)
        .collect();
    let mut process_events = client.process().subscribe(&process.id).await?;

    let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    let (command_tx, mut command_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let forward_tx = event_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = process_events.recv().await {
            let sent = match event {
                ProcessEvent::Output(line) => forward_tx.send(ui::ConsoleEvent::Output(line.line)),
                // The console drives one running server over its rcon channel,
                // so it closes with that process rather than following the entry.
                ProcessEvent::Exit(_) => {
                    let _ = forward_tx.send(ui::ConsoleEvent::Closed("server stopped".into()));
                    break;
                }
                ProcessEvent::Started(_) => continue,
            };
            if sent.is_err() {
                break;
            }
        }
    });

    // The driver tasks own the client: the session (and with it the
    // subscription) lives exactly as long as the console runs.
    let client = Arc::new(client);
    let commander = client.clone();
    let server_id = info.id.clone();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            let event = match commander.server().command(&server_id, &command).await {
                Ok(reply) => ui::ConsoleEvent::Reply(strip_codes(&reply)),
                Err(e) => ui::ConsoleEvent::Notice(format!("{e:#}")),
            };
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    let (line_tx, mut line_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (completion_tx, completion_rx) = tokio::sync::mpsc::unbounded_channel();
    let server_id = info.id.clone();
    tokio::spawn(async move {
        while let Some(mut line) = line_rx.recv().await {
            while let Ok(newer) = line_rx.try_recv() {
                line = newer;
            }
            let found = match client.server().complete(&server_id, &line).await {
                Ok(found) => found,
                Err(error) => {
                    tracing::debug!(%error, "console completion failed");
                    continue;
                }
            };
            let offered = ui::Completions {
                line,
                completions: found.completions,
                arguments: found.arguments,
            };
            if completion_tx.send(offered).is_err() {
                break;
            }
        }
    });

    let title = info.name.clone();
    let link = ui::ConsoleLink {
        commands: command_tx,
        lines: line_tx,
        completions: completion_rx,
    };
    let closed = tokio::task::spawn_blocking(move || ui::console(&title, backfill, event_rx, link))
        .await??;
    match closed {
        Some(message) => ui::show(View::note(message)),
        None => ui::show(View::note(format!("detached ('{server}' keeps running)"))),
    }
}

pub(super) fn show_reply(reply: &str) -> Result<()> {
    let reply = strip_codes(reply);
    if reply.trim().is_empty() {
        return ui::show(View::note("(no reply)"));
    }
    for line in reply.lines() {
        ui::show(View::line(line))?;
    }
    Ok(())
}

/// Drop Minecraft's `§x` color codes — RCON replies carry them verbatim.
fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}
