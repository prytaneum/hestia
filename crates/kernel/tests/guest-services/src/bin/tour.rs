#![no_main]

use hestia::notify::Level;
use hestia::{data, jobs, messages, notify, progress, secrets, settings};
use hestia::{Commands, Error, Lifecycle, OnJobDone, OnSchedule, SettingsChanged};

struct Tour;

fn expect(ok: bool, what: &str) -> Result<(), Error> {
    match ok {
        true => Ok(()),
        false => Err(Error::Other(what.to_owned())),
    }
}

fn tour() -> Result<(), Error> {
    std::fs::write("/data/note.txt", "kept").map_err(|e| Error::Other(e.to_string()))?;

    settings::set("volume", "7")?;
    expect(settings::get("volume")?.as_deref() == Some("7"), "setting")?;
    expect(
        matches!(settings::set("bad", "{"), Err(Error::Invalid(_))),
        "setting must be JSON",
    )?;

    let args = [("name".to_owned(), "Ann".to_owned())];
    expect(
        messages::text("greeting", &args) == "Hello Ann",
        "catalogue text",
    )?;
    expect(messages::text("absent", &[]) == "absent", "key fallback")?;

    for line in 0..5 {
        notify::notify(Level::Info, &format!("notice {line}"))?;
    }
    expect(
        matches!(
            notify::notify(Level::Info, "one too many"),
            Err(Error::Busy)
        ),
        "notice budget",
    )?;

    for bad in [
        "http://example.com/",
        "https://1.2.3.4/",
        "https://user:pw@example.com/",
        "nonsense",
    ] {
        expect(matches!(notify::open_url(bad), Err(Error::Invalid(_))), bad)?;
    }
    notify::open_url("https://example.com/docs")?;
    expect(
        matches!(
            notify::open_url("https://example.com/again"),
            Err(Error::Busy)
        ),
        "url budget",
    )?;

    expect(!progress::cancelled(), "not cancelled")?;
    expect(
        matches!(notify::confirm("sure?"), Err(Error::Unsupported)),
        "confirm needs a person",
    )
}

fn secret() -> Result<(), Error> {
    secrets::set("token", "s3cret")?;
    expect(
        secrets::get("token")?.as_deref() == Some("s3cret"),
        "secret",
    )
}

fn hold() -> Result<(), Error> {
    expect(data::try_lock("x")?, "first lock")?;
    notify::notify(Level::Info, "locked")?;
    let mut spins = 0u64;
    loop {
        spins = std::hint::black_box(spins.wrapping_add(1));
    }
}

impl Lifecycle for Tour {
    fn init() -> Result<(), Error> {
        let mode = std::fs::read_to_string("/data/mode").unwrap_or_default();
        match mode.trim() {
            "tour" => tour(),
            "secret" => secret(),
            "hold" => hold(),
            "contend" => expect(!data::try_lock("x")?, "lock is held"),
            "free" => expect(data::try_lock("x")?, "lock was released"),
            other => Err(Error::Invalid(other.to_owned())),
        }
    }
}

impl SettingsChanged for Tour {
    fn changed(key: String, value: String) -> Result<(), Error> {
        if !notify::confirm(&format!("apply {key}?"))? {
            return Err(Error::Failed("declined".into()));
        }
        std::fs::write("/data/changed", format!("{key}={value}"))
            .map_err(|e| Error::Other(e.to_string()))
    }
}

fn note(file: &str, text: &str) -> Result<(), Error> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("/data/{file}"))
        .map_err(|e| Error::Other(e.to_string()))?;
    writeln!(file, "{text}").map_err(|e| Error::Other(e.to_string()))
}

impl Commands for Tour {
    fn run(command: String, args: String) -> Result<(), Error> {
        match command.as_str() {
            "echo" => note("ran", &format!("{command} {args}")),
            "ask" => {
                notify::notify(Level::Info, "asking")?;
                notify::open_url("https://example.com/docs")?;
                match notify::confirm("proceed?")? {
                    true => note("ran", "confirmed"),
                    false => Err(Error::Failed("declined".into())),
                }
            }
            "work" => {
                for step in 0..3 {
                    progress::report(step, 3, "working");
                    if progress::cancelled() {
                        return Err(Error::Failed("cancelled".into()));
                    }
                }
                note("ran", "worked")
            }
            "wait" => {
                notify::notify(Level::Info, "waiting")?;
                loop {
                    std::hint::spin_loop();
                }
            }
            "trap" => panic!("trapped on purpose"),
            "spawn" => note("spawned", &jobs::start("echo", "{}")?),
            "flood" => {
                let mut started = 0;
                while jobs::start("echo", "{}").is_ok() {
                    started += 1;
                }
                note("flood", &started.to_string())
            }
            other => Err(Error::NotFound(other.to_owned())),
        }
    }
}

impl OnSchedule for Tour {
    fn tick(schedule: String) -> Result<(), Error> {
        note("ticks", &schedule)
    }
}

impl OnJobDone for Tour {
    fn finished(job: String, outcome: hestia::JobOutcome) -> Result<(), Error> {
        note("done", &format!("{job} {outcome:?}"))?;
        notify::notify(Level::Info, "job ended")
    }
}

hestia::export!(Tour);
