//! The functions of each import, with the signatures the generated bindings have
//! on `wasm32`, so a plugin builds unchanged for either target.

use super::{gate, note, with};

pub mod log {
    pub use crate::base::hestia::kernel::log::Level;

    pub fn write(level: Level, message: &str) {
        super::note("hestia:kernel/log", "write");
        super::with(|host| host.logs.push((level, message.to_owned())));
    }
}

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

pub mod data {
    use crate::Error;

    pub fn try_lock(name: &str) -> Result<bool, Error> {
        super::note("hestia:kernel/data", "try-lock");
        Ok(super::with(|host| host.locks.insert(name.to_owned())))
    }

    pub fn unlock(name: &str) {
        super::note("hestia:kernel/data", "unlock");
        super::with(|host| host.locks.remove(name));
    }
}

pub mod settings {
    use crate::Error;

    pub fn get(key: &str) -> Result<Option<String>, Error> {
        super::note("hestia:kernel/settings", "get");
        Ok(super::with(|host| host.settings.get(key).cloned()))
    }

    pub fn set(key: &str, value: &str) -> Result<(), Error> {
        super::note("hestia:kernel/settings", "set");
        super::with(|host| host.settings.insert(key.to_owned(), value.to_owned()));
        Ok(())
    }
}

pub mod secrets {
    use crate::Error;

    pub fn get(key: &str) -> Result<Option<String>, Error> {
        super::gate("hestia:kernel/secrets", "get")?;
        Ok(super::with(|host| host.secrets.get(key).cloned()))
    }

    pub fn set(key: &str, value: &str) -> Result<(), Error> {
        super::gate("hestia:kernel/secrets", "set")?;
        super::with(|host| host.secrets.insert(key.to_owned(), value.to_owned()));
        Ok(())
    }
}

pub mod messages {
    pub fn text(key: &str, args: &[(String, String)]) -> String {
        super::note("hestia:kernel/messages", "text");
        args.iter().fold(key.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
    }
}

pub mod progress {
    pub fn report(_current: u64, _total: u64, _message: &str) {
        super::note("hestia:kernel/progress", "report");
    }

    pub fn cancelled() -> bool {
        super::note("hestia:kernel/progress", "cancelled");
        super::with(|host| host.cancelled)
    }
}

pub mod notify {
    pub use crate::base::hestia::kernel::notify::Level;

    use crate::Error;

    pub fn notify(_level: Level, _message: &str) -> Result<(), Error> {
        super::note("hestia:kernel/notify", "notify");
        Ok(())
    }

    pub fn confirm(_message: &str) -> Result<bool, Error> {
        super::note("hestia:kernel/notify", "confirm");
        Ok(super::with(|host| host.confirm))
    }

    pub fn open_url(_url: &str) -> Result<(), Error> {
        super::note("hestia:kernel/notify", "open-url");
        Ok(())
    }
}

pub mod jobs {
    use crate::Error;

    pub fn start(_command: &str, _args: &str) -> Result<String, Error> {
        super::note("hestia:kernel/jobs", "start");
        Ok(super::with(|host| {
            host.jobs += 1;
            format!("job-{}", host.jobs)
        }))
    }
}

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

pub mod download {
    use crate::Error;

    pub fn fetch(_url: &str, _path: &str, _sha256: Option<&str>) -> Result<u64, Error> {
        super::gate("hestia:kernel/download", "fetch")?;
        Ok(0)
    }
}

pub mod programs {
    pub use crate::base::hestia::kernel::programs::Output;

    use crate::Error;

    pub fn run(_program: &str, _args: &[String]) -> Result<Output, Error> {
        super::gate("hestia:kernel/programs", "run")?;
        Ok(Output {
            status: 0,
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }
}

pub mod ipc {
    use crate::Error;

    pub struct Connection;

    impl Connection {
        pub fn write(&self, _bytes: &[u8]) -> Result<(), Error> {
            super::note("hestia:kernel/ipc", "write");
            Ok(())
        }

        pub fn read(&self, _max: u32) -> Result<Vec<u8>, Error> {
            super::note("hestia:kernel/ipc", "read");
            Ok(Vec::new())
        }
    }

    pub fn connect(_endpoint: &str) -> Result<Connection, Error> {
        super::gate("hestia:kernel/ipc", "connect")?;
        Ok(Connection)
    }
}
