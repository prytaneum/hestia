//! `hestia account …` — Microsoft/Minecraft sign-in and account switching.

use std::process::Stdio;

use anyhow::{bail, Result};
use clap::Subcommand;
use client::proto::accounts::{Account, LoginMethod};
use client::Client;

use crate::ui::{self, Spinner, View};

#[derive(Subcommand)]
pub enum AccountCmd {
    /// Sign in to a Microsoft account
    Login,
    /// Signed-in accounts (* marks the one launches use)
    #[command(visible_alias = "ls")]
    List,
    /// Pick the account launches use; prompts when omitted
    #[command(visible_alias = "use")]
    Switch {
        /// Account name or uuid
        account: Option<String>,
    },
    /// Sign out of an account and forget its tokens
    Logout {
        /// Account name or uuid
        account: String,
    },
}

pub async fn run(cmd: AccountCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        AccountCmd::Login => {
            let account = device_code_login(&client).await?;
            ui::show(View::line(format!(
                "Signed in as {} ({})",
                account.name, account.uuid
            )))?;
        }
        AccountCmd::List => {
            let listing = client.accounts().list().await?;
            if listing.accounts.is_empty() {
                return ui::show(View::note("no accounts signed in (hestia account login)"));
            }
            let rows = listing
                .accounts
                .iter()
                .map(|a| {
                    let marker = if a.uuid == listing.default_uuid {
                        "*"
                    } else {
                        ""
                    };
                    vec![marker.to_string(), a.name.clone(), a.uuid.clone()]
                })
                .collect();
            ui::show(View::table("accounts", ["", "NAME", "UUID"], rows))?;
        }
        AccountCmd::Switch { account } => {
            let reference = match account {
                Some(reference) => reference,
                None => pick_account(&client).await?,
            };
            let switched = client.accounts().switch(&reference).await?;
            ui::show(View::line(format!(
                "launches now use {} ({})",
                switched.name, switched.uuid
            )))?;
        }
        AccountCmd::Logout { account } => {
            client.accounts().remove(&account).await?;
            ui::show(View::line(format!("Signed out {account}")))?;
        }
    }
    Ok(())
}

/// Interactive sign-in for a launch whose account's tokens expired, reusing the
/// device-code flow. Returns the re-signed account.
pub(crate) async fn reauth(client: &Client) -> Result<Account> {
    device_code_login(client).await
}

/// Interactive account picker for a `switch` without an argument.
async fn pick_account(client: &Client) -> Result<String> {
    let listing = client.accounts().list().await?;
    if listing.accounts.is_empty() {
        bail!("no accounts signed in (hestia account login)");
    }
    let labels: Vec<String> = listing
        .accounts
        .iter()
        .map(|a| {
            if a.uuid == listing.default_uuid {
                format!("{} (current)", a.name)
            } else {
                a.name.clone()
            }
        })
        .collect();
    let index = ui::prompt_select("switch launches to", &labels)?;
    Ok(listing.accounts[index].uuid.clone())
}

async fn device_code_login(client: &Client) -> Result<Account> {
    let flow = client
        .accounts()
        .begin_login(LoginMethod::DeviceCode)
        .await?;
    let copied = copy_clipboard(&flow.user_code);
    let hint = if copied {
        " (copied to the clipboard)"
    } else {
        ""
    };
    ui::show(View::line(format!(
        "To sign in, open\n\n  {}\n\nand enter the code\n\n  {}{hint}",
        flow.verification_uri, flow.user_code
    )))?;
    open_browser(&flow.verification_uri);
    let _spinner = Spinner::start("waiting for you to finish in the browser…");
    Ok(client.accounts().complete_login(&flow.id, "").await?)
}

/// Best-effort copy through the platform's own clipboard tool; returns whether
/// one accepted the text.
fn copy_clipboard(text: &str) -> bool {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip", &[])]
    } else {
        &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
    };
    for (program, args) in candidates {
        let child = std::process::Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { continue };
        let written = child.stdin.take().is_some_and(|mut stdin| {
            std::io::Write::write_all(&mut stdin, text.as_bytes()).is_ok()
        });
        if written && matches!(child.wait(), Ok(status) if status.success()) {
            return true;
        }
    }
    false
}

fn open_browser(url: &str) {
    if !url.starts_with("https://") || url.contains(['"', '\'']) {
        return;
    }
    let mut command = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = command.stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}
