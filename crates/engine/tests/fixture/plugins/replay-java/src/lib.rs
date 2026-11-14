//! A Java distribution that names one build per feature version. Java 21 is
//! served from the plugin's granted host; Java 17 deliberately names a host
//! outside the grant, so a test can see the host refuse it.

use hestia::proto::java::JavaRelease;
use hestia::proto::provider::java::{JavaBuild, Majors, Resolve, TargetQuery};
use hestia::{Error, Plugin, Provide, Provided};

const DISTRIBUTION: &str = "replay";
const SHA256: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct Replay;

fn check(distribution: &str) -> Result<(), Error> {
    match distribution {
        DISTRIBUTION => Ok(()),
        other => Err(Error::NotFound(format!("unknown distribution `{other}`"))),
    }
}

fn majors(distribution: &str) -> Result<Vec<JavaRelease>, Error> {
    check(distribution)?;
    Ok(vec![
        JavaRelease {
            major: 17,
            lts: true,
        },
        JavaRelease {
            major: 21,
            lts: true,
        },
    ])
}

fn resolve(distribution: &str, target: TargetQuery) -> Result<Option<JavaBuild>, Error> {
    check(distribution)?;
    let host = match target.major {
        21 => "downloads.example.com",
        17 => "mirror.example.org",
        _ => return Ok(None),
    };
    let archive = if target.os == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(Some(JavaBuild {
        vendor: "replay".into(),
        version: format!("{}.0.1", target.major),
        url: format!("https://{host}/replay-{}.{archive}", target.major),
        sha256: SHA256.into(),
        archive: archive.into(),
    }))
}

impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Majors>(|distribution, _| majors(distribution))
            .on::<Resolve>(resolve)
            .finish()
    }
}

hestia::plugin!(Replay);
