use anyhow::{anyhow, Result};
use discord_rich_presence::activity::{self, Assets, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use proto::presence::Activity;

use super::Discord;

/// Discord's local IPC socket (a named pipe on Windows), reached through the
/// `discord-rich-presence` crate.
pub(super) struct Ipc(DiscordIpcClient);

impl Ipc {
    pub(super) fn new(application_id: &str) -> Self {
        Ipc(DiscordIpcClient::new(application_id))
    }
}

impl Discord for Ipc {
    fn connect(&mut self) -> Result<()> {
        self.0.connect().map_err(|error| anyhow!("{error}"))
    }

    fn set(&mut self, card: &Activity) -> Result<()> {
        let mut assets = Assets::new();
        if let Some(key) = &card.large_image {
            assets = assets.large_image(key);
        }
        if let Some(text) = &card.large_text {
            assets = assets.large_text(text);
        }
        if let Some(key) = &card.small_image {
            assets = assets.small_image(key);
        }
        if let Some(text) = &card.small_text {
            assets = assets.small_text(text);
        }
        let mut activity = activity::Activity::new().assets(assets);
        if let Some(details) = &card.details {
            activity = activity.details(details);
        }
        if let Some(state) = &card.state {
            activity = activity.state(state);
        }
        if let Some(start) = card.start {
            activity = activity.timestamps(Timestamps::new().start(start));
        }
        if !card.buttons.is_empty() {
            activity = activity.buttons(
                card.buttons
                    .iter()
                    .map(|button| activity::Button::new(&button.label, &button.url))
                    .collect(),
            );
        }
        self.0
            .set_activity(activity)
            .map_err(|error| anyhow!("{error}"))
    }

    fn clear(&mut self) -> Result<()> {
        self.0.clear_activity().map_err(|error| anyhow!("{error}"))
    }

    fn close(&mut self) {
        let _ = self.0.close();
    }
}
