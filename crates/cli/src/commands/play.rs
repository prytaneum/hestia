//! `hestia play` — the happy path: launch an instance. With no argument the
//! sole instance launches directly; several prompt an interactive pick.
//!
//! `--world` / `--server` / `--realm` skip the menus and join on start (Quick
//! Play), the same targets the entry-first `instance <name> world|server play`
//! verbs take. `--recent` picks both the instance and the target from what was
//! played lately.

use anyhow::{bail, Result};

use crate::commands::{instance, mc};
use crate::ui;

#[derive(clap::Args)]
pub struct Play {
    /// Instance name or id; the sole instance launches directly
    instance: Option<String>,
    #[arg(long, help = "Account name or uuid (default: the switched-to account)")]
    account: Option<String>,
    #[arg(short, long, help = "Return immediately instead of following the logs")]
    detach: bool,
    #[arg(
        long,
        help = "Launch another session even if one is already running (needs 'config set \
                instance.multi-session true')"
    )]
    new_session: bool,
    #[arg(
        long,
        help = "Play without signing in: singleplayer only, on the last known account"
    )]
    offline: bool,
    #[command(flatten)]
    join: instance::Join,
    #[arg(
        long,
        conflicts_with_all = ["instance", "world", "server", "realm"],
        help = "Pick from what was played lately and go straight back to it"
    )]
    recent: bool,
}

pub async fn run(play: Play) -> Result<()> {
    let client = super::connect().await?;
    let (reference, quick_play) = if play.recent {
        let recent = instance::recent::pick(&client).await?;
        (recent.instance, Some(recent.target))
    } else {
        let reference = match play.instance {
            Some(reference) => reference,
            None => pick_instance(&client).await?,
        };
        (reference, play.join.target())
    };
    instance::launch(
        &client,
        instance::Launch {
            reference: &reference,
            account: play.account.as_deref().unwrap_or_default(),
            new_session: play.new_session,
            detach: play.detach,
            quick_play,
            offline: play.offline,
            ..instance::Launch::default()
        },
    )
    .await
}

async fn pick_instance(client: &client::Client) -> Result<String> {
    let instances = client.instance().list().await?;
    match instances.len() {
        0 => bail!("no instances yet (hestia instance create)"),
        1 => Ok(instances[0].name.clone()),
        _ => {
            let labels: Vec<String> = instances
                .iter()
                .map(|i| mc::entry_label(&i.name, &i.flavor, &i.game_version))
                .collect();
            let index = ui::select("what do you want to play?", &labels)?;
            Ok(instances[index].name.clone())
        }
    }
}
