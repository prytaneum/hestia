//! Typed client SDK: a Session and one facade per domain. Front-ends drive the
//! daemon only through here — never by linking the engine.

mod facades;
mod session;
mod spawn;

pub use facades::{
    Accounts, App, Cache, Config, Content, Daemon, Entry, Files, Host, Instance, Java,
    LaunchOptions, Modpack, Net, Players, Process, ProcessEvent, Screenshot, Server, Skins, Sync,
    Tasks, Transfer, Update,
};
pub use ipc::errors::IpcError;

pub mod errors {
    pub use proto::error::code::{NOT_FOUND, UNAUTHORIZED};
}
pub use session::{job_id, Listener, Session};

/// Re-export `proto` so front-ends need only depend on `client`.
pub use proto;

/// Recover the structured `ErrorInfo` a client error carries: the daemon's
/// serialized error when there was one, else a generic `Internal` from the
/// transport-level message. The inverse of how a handler's `ErrorInfo` crosses
/// the socket — so a caller can rebuild a typed failure to render or re-report.
pub fn error_info(error: &IpcError) -> proto::error::ErrorInfo {
    if let IpcError::Daemon { info, .. } = error {
        if let Ok(parsed) = serde_json::from_value::<proto::error::ErrorInfo>(info.clone()) {
            return parsed;
        }
    }
    proto::error::ErrorInfo::Internal {
        detail: error.to_string(),
    }
}

use std::path::Path;

/// A connection to the daemon plus the typed facades over it.
pub struct Client {
    session: Session,
}

impl Client {
    /// Drive a daemon over an already-established connection.
    pub fn over(connection: ipc::Connection) -> Client {
        Client {
            session: Session::new(connection),
        }
    }

    /// Connect to a running daemon. Never spawns — use [`Client::start`] for that.
    pub async fn connect() -> Result<Client, IpcError> {
        let endpoint = ipc::endpoint::default_endpoint();
        let conn = ipc::connect(&endpoint).await?;
        tracing::debug!(endpoint = %endpoint.display(), "connected to the daemon");
        Ok(Client::over(conn))
    }

    /// Start the daemon if not already running, then connect. The one path
    /// that spawns `hestiad` — every explicit start action routes through here.
    pub async fn start() -> Result<Client, IpcError> {
        let endpoint = ipc::endpoint::default_endpoint();
        if let Ok(conn) = ipc::connect(&endpoint).await {
            return Ok(Client::over(conn));
        }
        tracing::debug!(endpoint = %endpoint.display(), "daemon not running; starting it");
        spawn::spawn_daemon()?;
        match spawn::connect_with_retry(&endpoint).await {
            Some(conn) => Ok(Client::over(conn)),
            None => Err(IpcError::ConnectionLost),
        }
    }

    /// Connect to a daemon listening on `endpoint` (no auto-spawn).
    pub async fn connect_to(endpoint: &Path) -> Result<Client, IpcError> {
        Ok(Client::over(ipc::connect(endpoint).await?))
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn app(&self) -> App<'_> {
        App {
            session: &self.session,
        }
    }

    pub fn daemon(&self) -> Daemon<'_> {
        Daemon {
            session: &self.session,
        }
    }

    /// Ask the daemon to cancel a running job by the id its events carry.
    /// `false` means it was already over — a normal race, not an error.
    ///
    /// Cancelling is always explicit: a disconnecting client never cancels
    /// anything, because a job outlives the client that started it.
    pub async fn cancel_job(&self, id: &str) -> Result<bool, IpcError> {
        let params = proto::job::JobCancelParams { id: id.to_string() };
        Ok(self
            .session
            .call::<proto::job::JobCancel>(&params)
            .await?
            .cancelled)
    }

    pub fn config(&self) -> Config<'_> {
        Config {
            session: &self.session,
        }
    }

    pub fn cache(&self) -> Cache<'_> {
        Cache {
            session: &self.session,
        }
    }

    pub fn java(&self) -> Java<'_> {
        Java {
            session: &self.session,
        }
    }

    pub fn host(&self) -> Host<'_> {
        Host {
            session: &self.session,
        }
    }

    pub fn net(&self) -> Net<'_> {
        Net {
            session: &self.session,
        }
    }

    pub fn tasks(&self) -> Tasks<'_> {
        Tasks {
            session: &self.session,
        }
    }

    pub fn players(&self) -> Players<'_> {
        Players {
            session: &self.session,
        }
    }

    pub fn update(&self) -> Update<'_> {
        Update {
            session: &self.session,
        }
    }

    pub fn accounts(&self) -> Accounts<'_> {
        Accounts {
            session: &self.session,
        }
    }

    pub fn process(&self) -> Process<'_> {
        Process {
            session: &self.session,
        }
    }

    pub fn server(&self) -> Server<'_> {
        Server {
            session: &self.session,
        }
    }

    pub fn entry(&self) -> Entry<'_> {
        Entry {
            session: &self.session,
        }
    }

    pub fn instance(&self) -> Instance<'_> {
        Instance {
            session: &self.session,
        }
    }

    pub fn content(&self) -> Content<'_> {
        Content {
            session: &self.session,
        }
    }

    pub fn modpack(&self) -> Modpack<'_> {
        Modpack {
            session: &self.session,
        }
    }

    pub fn skins(&self) -> Skins<'_> {
        Skins {
            session: &self.session,
        }
    }

    pub fn files(&self) -> Files<'_> {
        Files {
            session: &self.session,
        }
    }

    pub fn screenshot(&self) -> Screenshot<'_> {
        Screenshot {
            session: &self.session,
        }
    }

    pub fn sync(&self) -> Sync<'_> {
        Sync {
            session: &self.session,
        }
    }

    pub fn transfer(&self) -> Transfer<'_> {
        Transfer {
            session: &self.session,
        }
    }
}
