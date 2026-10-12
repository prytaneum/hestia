//! A running server's console: one command over its kept RCON connection, and
//! what a half-typed line could continue with.

use anyhow::{Context, Result};
use proto::server::ServerCommandCompleteResult;

use crate::console::{self, Typed};
use crate::engine::Engine;
use crate::servers::RconConfig;

const PLAYER_ARGUMENTS: [&str; 4] = ["<target>", "<targets>", "<player>", "<players>"];

impl Engine {
    pub async fn server_command(&self, reference: &str, command: &str) -> Result<String> {
        let (id, rcon) = self.server_rcon(reference)?;
        self.consoles.command(&id, &rcon, command).await
    }

    /// Completions come from the server's own `help`; a player argument adds who is online.
    pub async fn server_complete(
        &self,
        reference: &str,
        line: &str,
    ) -> Result<ServerCommandCompleteResult> {
        let (id, rcon) = self.server_rcon(reference)?;
        let Some(typed) = Typed::parse(line) else {
            return Ok(ServerCommandCompleteResult::default());
        };
        let mut words = String::new();
        let mut usage = self.consoles.usage(&id, &rcon, "").await?;
        for word in &typed.words {
            let through = format!("{words} {word}");
            let through = through.trim_start();
            let after = self.consoles.usage(&id, &rcon, through).await?;
            let target = console::redirect(&usage, &words, word)
                .filter(|_| console::is_end(&after, through));
            (words, usage) = match target {
                Some(target) => {
                    let usage = self.consoles.usage(&id, &rcon, target).await?;
                    (target.to_string(), usage)
                }
                None => (through.to_string(), after),
            };
        }
        let mut found = console::candidates(&usage, &words);
        if PLAYER_ARGUMENTS
            .iter()
            .any(|argument| found.arguments.contains(*argument))
        {
            let online = self.online_players(&id).await?;
            found
                .literals
                .extend(online.into_iter().map(|player| player.name));
        }
        let partial = typed.partial.to_lowercase();
        let slash = match typed.slash && typed.words.is_empty() {
            true => "/",
            false => "",
        };
        Ok(ServerCommandCompleteResult {
            completions: found
                .literals
                .into_iter()
                .filter(|literal| literal.to_lowercase().starts_with(&partial))
                .map(|literal| format!("{slash}{literal}"))
                .collect(),
            arguments: found.arguments.into_iter().collect(),
        })
    }

    fn server_rcon(&self, reference: &str) -> Result<(String, RconConfig)> {
        let record = self
            .servers
            .get(reference)
            .with_context(|| format!("unknown server: {reference}"))?;
        let rcon = record
            .rcon
            .context("this server has no console yet (restart it to enable one)")?;
        Ok((record.id, rcon))
    }
}
