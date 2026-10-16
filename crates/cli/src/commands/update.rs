//! `hestia update` — check the release feed and install the new version.

use std::sync::Arc;

use anyhow::{Context, Result};
use client::IpcError;

use crate::ui::{self, ByteReporter, Spinner, View};

pub async fn run(yes: bool) -> Result<()> {
    let client = super::connect().await?;
    let status = {
        let _spinner = Spinner::start("checking for updates");
        client.update().check().await?
    };
    let Some(info) = status.available else {
        return ui::show(View::line(format!(
            "hestia {} is up to date on the {} channel",
            status.current,
            status.channel.as_str()
        )));
    };

    if !info.applicable {
        return ui::show(View::note(format!(
            "hestia {} is available (installed: {}) — download it at {}",
            info.version, status.current, info.url
        )));
    }

    if !yes {
        let accepted = ui::confirm(
            &format!("update hestia {} → {}?", status.current, info.version),
            "download and update",
            "cancel",
        )
        .context("pass --yes to update without a prompt")?;
        if !accepted {
            return ui::show(View::note("update cancelled"));
        }
    }

    const LABEL: &str = "downloading update";
    let reporter = Arc::new(ByteReporter::new(LABEL));
    let progress = reporter.clone();
    let (path, version) = client
        .update()
        .download(move |p| progress.bytes(LABEL, p.downloaded, p.total))
        .await?;
    reporter.finish();

    let applied = match client.update().apply(&path).await {
        Ok(applied) => applied,
        Err(e) => match elevation_command(&e) {
            Some(command) => return elevate(&command, yes),
            None => return Err(e.into()),
        },
    };

    ui::show(View::line(if applied.relaunches {
        format!("installer for {version} started — it stops the daemon, updates, and restarts it")
    } else {
        format!("hestia {version} installed — restart hestiad to run it")
    }))
}

/// The daemon has no terminal to prompt on, so it hands back the command it
/// could not elevate; this process does have one.
fn elevate(command: &str, yes: bool) -> Result<()> {
    if !yes {
        let accepted = ui::confirm(
            &format!("installing needs administrator rights — run `{command}`?"),
            "run it",
            "cancel",
        )
        .context("pass --yes to elevate without a prompt")?;
        if !accepted {
            return ui::show(View::note(format!(
                "update staged — run `{command}` to finish"
            )));
        }
    }

    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .context("the daemon returned an empty command")?;
    let status = std::process::Command::new(program)
        .args(parts)
        .status()
        .with_context(|| format!("cannot run {program}"))?;
    if !status.success() {
        anyhow::bail!("`{command}` exited with {status}");
    }
    ui::show(View::line("update installed — restart hestiad to run it"))
}

fn elevation_command(error: &IpcError) -> Option<String> {
    let IpcError::Daemon { info, .. } = error else {
        return None;
    };
    match serde_json::from_value::<proto::error::ErrorInfo>(info.clone()).ok()? {
        proto::error::ErrorInfo::ElevationRequired { command } => Some(command),
        _ => None,
    }
}
