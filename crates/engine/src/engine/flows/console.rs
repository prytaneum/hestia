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
            let reply = self.consoles.command(&id, &rcon, "list uuids").await?;
            found.literals.extend(online_names(&reply));
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
        let record = self.servers.require(reference)?;
        let rcon = record
            .rcon
            .context("this server has no console yet (restart it to enable one)")?;
        Ok((record.id, rcon))
    }
}

/// The names out of a `list uuids` reply. Every version puts them after the
/// first colon, comma-separated — on the same line or the next — each as
/// `Name (uuid)`; a server too old to know the argument answers with bare names.
fn online_names(reply: &str) -> Vec<String> {
    let Some((_, players)) = reply.split_once(':') else {
        return Vec::new();
    };
    players
        .split([',', '\n'])
        .map(str::trim)
        .filter(|player| !player.is_empty())
        .map(|player| {
            player
                .strip_suffix(')')
                .and_then(|rest| rest.rsplit_once(" ("))
                .map_or(player, |(name, _)| name)
                .to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_online_players_are_read_from_either_reply_shape() {
        assert_eq!(
            online_names("There are 2 of a max of 20 players online: Alex (u1), Steve (u2)"),
            ["Alex", "Steve"]
        );
        assert_eq!(
            online_names("There are 2/20 players online:\nAlex, Steve"),
            ["Alex", "Steve"]
        );
        assert!(online_names("There are 0 of a max of 20 players online: ").is_empty());
    }
}
