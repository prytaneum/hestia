//! One kept RCON connection per running server: the server logs every connect
//! and disconnect, so reconnecting per command floods its own log.

mod complete;

pub use complete::{alias, candidates, Typed};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::minecraft::rcon::Rcon;
use crate::servers::RconConfig;

const MAX_USAGES: usize = 256;
// Bukkit servers replace `help` with a paged index; vanilla's stays namespaced.
const NAMESPACED_HELP: &str = "minecraft:help";
const HELP: &str = "help";

type Slot = Arc<tokio::sync::Mutex<Option<Session>>>;

struct Session {
    rcon: Rcon,
    help: Option<&'static str>,
    usages: HashMap<String, String>,
}

#[derive(Default)]
pub struct Consoles {
    sessions: Mutex<HashMap<String, Slot>>,
}

impl Consoles {
    pub async fn command(&self, server: &str, rcon: &RconConfig, command: &str) -> Result<String> {
        let slot = self.slot(server);
        let mut session = slot.lock().await;
        run(&mut session, rcon, command).await
    }

    /// The server's `help` reply for what may follow `words`, asked once per connection.
    pub async fn usage(&self, server: &str, rcon: &RconConfig, words: &str) -> Result<String> {
        let slot = self.slot(server);
        let mut session = slot.lock().await;
        let known = session
            .as_ref()
            .filter(|session| session.rcon.is_open())
            .and_then(|session| session.usages.get(words));
        if let Some(usage) = known {
            return Ok(usage.clone());
        }
        let help = match session.as_ref().and_then(|session| session.help) {
            Some(help) => help,
            None => {
                let namespaced = run(&mut session, rcon, NAMESPACED_HELP).await?;
                let help = match namespaced.starts_with('/') {
                    true => NAMESPACED_HELP,
                    false => HELP,
                };
                if let Some(session) = session.as_mut() {
                    session.help = Some(help);
                }
                help
            }
        };
        let usage = run(&mut session, rcon, format!("{help} {words}").trim_end()).await?;
        if let Some(session) = session.as_mut() {
            if session.usages.len() >= MAX_USAGES {
                session.usages.clear();
            }
            session.usages.insert(words.to_string(), usage.clone());
        }
        Ok(usage)
    }

    fn slot(&self, server: &str) -> Slot {
        self.sessions
            .lock()
            .unwrap()
            .entry(server.to_string())
            .or_default()
            .clone()
    }
}

async fn run(session: &mut Option<Session>, rcon: &RconConfig, command: &str) -> Result<String> {
    if session.as_ref().is_some_and(|kept| !kept.rcon.is_open()) {
        *session = None;
    }
    let open = match session {
        Some(open) => open,
        None => {
            tracing::debug!(port = rcon.port, "rcon connection opened");
            session.insert(Session {
                rcon: Rcon::connect(rcon.port, &rcon.password).await?,
                help: None,
                usages: HashMap::new(),
            })
        }
    };
    let reply = open.rcon.command(command).await;
    // A failed exchange leaves the stream at an unknown offset.
    if reply.is_err() {
        *session = None;
    }
    reply
}
