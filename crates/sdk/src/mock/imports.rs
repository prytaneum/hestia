//! The functions of each import, with the signatures the generated bindings have
//! on `wasm32` (a compile-time check below keeps them equal), so a plugin builds
//! unchanged for either target.

use super::{denied, gate, note, with};

/// In memory; the WIT interface `log` has the same functions.
pub mod log {
    pub use crate::base::hestia::kernel::log::Level;

    pub fn write(level: Level, message: &str) {
        super::note("hestia:kernel/log", "write");
        super::with(|host| host.logs.push((level, message.to_owned())));
    }
}

/// In memory; the WIT interface `app` has the same functions.
pub mod app {
    pub use crate::base::hestia::kernel::app::AppInfo;

    pub fn info() -> AppInfo {
        super::note("hestia:kernel/app", "info");
        AppInfo {
            name: "Hestia".to_owned(),
            version: "0.0.0".to_owned(),
            channel: "mock".to_owned(),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            online: true,
        }
    }
}

/// In memory; the WIT interface `data` has the same functions.
pub mod data {
    pub use crate::base::hestia::kernel::types::PluginError;

    pub fn try_lock(name: &str) -> Result<bool, PluginError> {
        super::note("hestia:kernel/data", "try-lock");
        Ok(super::with(|host| host.locks.insert(name.to_owned())))
    }

    pub fn unlock(name: &str) {
        super::note("hestia:kernel/data", "unlock");
        super::with(|host| host.locks.remove(name));
    }
}

/// In memory; the WIT interface `settings` has the same functions.
pub mod settings {
    pub use crate::base::hestia::kernel::types::PluginError;

    pub fn get(key: &str) -> Result<Option<String>, PluginError> {
        super::note("hestia:kernel/settings", "get");
        Ok(super::with(|host| host.settings.get(key).cloned()))
    }

    pub fn set(key: &str, value: &str) -> Result<(), PluginError> {
        super::note("hestia:kernel/settings", "set");
        super::with(|host| host.settings.insert(key.to_owned(), value.to_owned()));
        Ok(())
    }
}

/// Permission `secrets`.
pub mod secrets {
    pub use crate::base::hestia::kernel::types::PluginError;

    pub fn get(key: &str) -> Result<Option<String>, PluginError> {
        super::gate("hestia:kernel/secrets", "get")?;
        Ok(super::with(|host| host.secrets.get(key).cloned()))
    }

    pub fn set(key: &str, value: &str) -> Result<(), PluginError> {
        super::gate("hestia:kernel/secrets", "set")?;
        super::with(|host| host.secrets.insert(key.to_owned(), value.to_owned()));
        Ok(())
    }
}

/// In memory; the WIT interface `messages` has the same functions.
pub mod messages {
    pub fn text(key: &str, args: &[(String, String)]) -> String {
        super::note("hestia:kernel/messages", "text");
        args.iter().fold(key.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
    }
}

/// In memory; the WIT interface `progress` has the same functions.
pub mod progress {
    pub fn report(_current: u64, _total: u64, _message: &str) {
        super::note("hestia:kernel/progress", "report");
    }

    pub fn cancelled() -> bool {
        super::note("hestia:kernel/progress", "cancelled");
        super::with(|host| host.cancelled)
    }
}

/// In memory; the WIT interface `notify` has the same functions.
pub mod notify {
    pub use crate::base::hestia::kernel::notify::Level;

    pub use crate::base::hestia::kernel::types::PluginError;

    pub fn notify(_level: Level, _message: &str) -> Result<(), PluginError> {
        super::note("hestia:kernel/notify", "notify");
        Ok(())
    }

    pub fn confirm(_message: &str) -> Result<bool, PluginError> {
        super::note("hestia:kernel/notify", "confirm");
        Ok(super::with(|host| host.confirm))
    }

    pub fn open_url(_url: &str) -> Result<(), PluginError> {
        super::note("hestia:kernel/notify", "open-url");
        Ok(())
    }
}

/// In memory; the WIT interface `jobs` has the same functions.
pub mod jobs {
    pub use crate::base::hestia::kernel::jobs::JobId;
    pub use crate::base::hestia::kernel::types::PluginError;

    pub fn start(_command: &str, _args: &str) -> Result<JobId, PluginError> {
        super::note("hestia:kernel/jobs", "start");
        Ok(super::with(|host| {
            host.jobs += 1;
            format!("job-{}", host.jobs)
        }))
    }
}

/// In memory; the WIT interface `permissions` has the same functions.
pub mod permissions {
    pub use crate::base::hestia::kernel::permissions::EntryScope;

    pub fn granted(permission: &str) -> bool {
        super::note("hestia:kernel/permissions", "granted");
        super::with(|host| host.granted.contains(permission))
    }

    pub fn entries() -> EntryScope {
        super::note("hestia:kernel/permissions", "entries");
        super::with(|host| {
            host.entries.clone().unwrap_or(EntryScope {
                all: false,
                ids: Vec::new(),
            })
        })
    }
}

/// Permission `network:connect`: `fetch` checks the grant's hosts as the host does.
pub mod download {
    pub use crate::base::hestia::kernel::types::PluginError;

    const INTERFACE: &str = "hestia:kernel/download";
    const PATH_BYTES: usize = 1024;

    fn matches(entry: &str, host: &str) -> bool {
        match entry.strip_prefix("*.") {
            Some(domain) => host
                .strip_suffix(domain)
                .and_then(|rest| rest.strip_suffix('.'))
                .is_some_and(|label| !label.is_empty() && !label.contains('.')),
            None => host == entry,
        }
    }

    fn host(url: &str) -> Result<String, PluginError> {
        let invalid = || PluginError::Invalid("url".into());
        let rest = url.split_once("://").ok_or_else(invalid)?;
        let authority = rest.1.split(['/', '?', '#']).next().unwrap_or_default();
        if !rest.0.eq_ignore_ascii_case("https") || authority.contains('@') {
            return Err(super::denied(INTERFACE));
        }
        let name = authority.split(':').next().unwrap_or_default();
        match name.is_empty() {
            true => Err(invalid()),
            false => Ok(name.trim_end_matches('.').to_ascii_lowercase()),
        }
    }

    /// Permission `network:connect`. Answers as the host does for a path that is
    /// not relative (`invalid`), a host the grant does not list, or no `sha256`
    /// from a host not listed as unhashed (`denied`). Writes nothing.
    pub fn fetch(url: &str, path: &str, sha256: Option<&str>) -> Result<u64, PluginError> {
        super::gate(INTERFACE, "fetch")?;
        if let Some(text) = sha256 {
            if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(PluginError::Invalid("sha256".into()));
            }
        }
        let plain = std::path::Path::new(path)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        if path.is_empty() || path.len() > PATH_BYTES || !plain {
            return Err(PluginError::Invalid("path".into()));
        }
        let host = host(url)?;
        let allowed = super::with(|state| {
            let listed = |hosts: &[String]| hosts.iter().any(|entry| matches(entry, &host));
            listed(&state.scope.hosts) && (sha256.is_some() || listed(&state.scope.unhashed))
        });
        match allowed {
            true => Ok(0),
            false => Err(super::denied(INTERFACE)),
        }
    }
}

/// Permission `process:spawn`.
pub mod programs {
    pub use crate::base::hestia::kernel::programs::Output;
    pub use crate::base::hestia::kernel::types::PluginError;

    /// Permission `process:spawn`. Only an invocation in the grant's scope (the
    /// path and exact arguments, for this OS) runs; anything else is `denied`.
    pub fn run(program: &str, args: &[String]) -> Result<Output, PluginError> {
        super::gate("hestia:kernel/programs", "run")?;
        let declared = super::with(|state| {
            state.scope.programs.iter().any(|granted| {
                granted.path == program
                    && granted.args == args
                    && granted
                        .os
                        .as_deref()
                        .is_none_or(|os| os == std::env::consts::OS)
            })
        });
        match declared {
            true => Ok(Output {
                status: 0,
                stdout: Vec::new(),
                stderr: Vec::new(),
            }),
            false => Err(super::denied("hestia:kernel/programs")),
        }
    }
}

/// Permission `ipc:connect`.
pub mod ipc {
    pub use crate::base::hestia::kernel::types::PluginError;

    /// An open connection, which reads end-of-stream.
    #[derive(Debug)]
    pub struct Connection(());

    impl Connection {
        pub fn write(&self, _bytes: &[u8]) -> Result<(), PluginError> {
            super::note("hestia:kernel/ipc", "write");
            Ok(())
        }

        pub fn read(&self, _max: u32) -> Result<Vec<u8>, PluginError> {
            super::note("hestia:kernel/ipc", "read");
            Ok(Vec::new())
        }
    }

    /// Permission `ipc:connect`. Only an endpoint in the grant's scope opens;
    /// any other is `denied`.
    pub fn connect(endpoint: &str) -> Result<Connection, PluginError> {
        super::gate("hestia:kernel/ipc", "connect")?;
        match super::with(|state| state.scope.endpoints.iter().any(|e| e == endpoint)) {
            true => Ok(Connection(())),
            false => Err(super::denied("hestia:kernel/ipc")),
        }
    }
}

/// Every function has the signature the generated bindings give it, or this does
/// not compile; `ipc::connect` differs only in the connection type it returns.
const _: () = {
    use crate::base::hestia::kernel as wit;
    let _ = [wit::log::write, log::write];
    let _ = [wit::app::info, app::info];
    let _ = [wit::data::try_lock, data::try_lock];
    let _ = [wit::data::unlock, data::unlock];
    let _ = [wit::settings::get, settings::get];
    let _ = [wit::settings::set, settings::set];
    let _ = [wit::secrets::get, secrets::get];
    let _ = [wit::secrets::set, secrets::set];
    let _ = [wit::messages::text, messages::text];
    let _ = [wit::progress::report, progress::report];
    let _ = [wit::progress::cancelled, progress::cancelled];
    let _ = [wit::notify::notify, notify::notify];
    let _ = [wit::notify::confirm, notify::confirm];
    let _ = [wit::notify::open_url, notify::open_url];
    let _ = [wit::jobs::start, jobs::start];
    let _ = [wit::permissions::granted, permissions::granted];
    let _ = [wit::permissions::entries, permissions::entries];
    let _ = [wit::download::fetch, download::fetch];
    let _ = [wit::programs::run, programs::run];
};
