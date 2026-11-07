//! The SpigotMC hub: which versions BuildTools can build, and what each one
//! needs from the JVM.
//!
//! There is no jar here, deliberately — Mojang's takedown means neither Spigot
//! nor CraftBukkit may be redistributed, so the only artifact SpigotMC serves
//! is BuildTools itself and the jar is compiled locally (see
//! `minecraft::spigot`). This client is the catalogue half alone.

use anyhow::Result;
use serde_json::Value;

use proto::error::Service;
use proto::provider::Provider;

use super::{fetch_json, fetch_text};

fn service() -> Service {
    Service::Provider(Provider::new("spigot", "SpigotMC"))
}

const HUB: &str = "https://hub.spigotmc.org";

/// The BuildTools build every install runs. Deliberately Jenkins'
/// `lastSuccessfulBuild` rather than a pinned number: BuildTools tracks the
/// upstream repositories it drives, so a pinned copy stops being able to build
/// each new game version as it lands.
pub const BUILDTOOLS_URL: &str =
    "https://hub.spigotmc.org/jenkins/job/BuildTools/lastSuccessfulBuild/artifact/target/BuildTools.jar";

/// A class-file major is its Java major plus this.
const CLASS_FILE_OFFSET: i64 = 44;

/// Every name the hub publishes metadata for.
///
/// The index is an nginx directory listing of `<name>.json`, and the great
/// majority of those names are Jenkins build numbers (`4458`, `343_legacy`)
/// rather than game versions. The caller filters the set against Mojang's
/// manifest, which is what leaves the game versions behind.
pub async fn versions() -> Result<Vec<String>> {
    Ok(names(
        &fetch_text(service(), &format!("{HUB}/versions/")).await?,
    ))
}

fn names(index: &str) -> Vec<String> {
    index
        .split("href=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter_map(|name| name.strip_suffix(".json"))
        .filter(|name| *name != "latest")
        .map(String::from)
        .collect()
}

/// The Java majors a version can be built and run with, lowest first — empty
/// for a version predating the field (the 1.8 line and older), which the caller
/// reads as "whatever the era used".
pub async fn java_versions(version: &str) -> Result<Vec<i32>> {
    let body = fetch_json(service(), &format!("{HUB}/versions/{version}.json")).await?;
    let mut majors: Vec<i32> = body
        .get("javaVersions")
        .and_then(Value::as_array)
        .map(|range| {
            range
                .iter()
                .filter_map(Value::as_i64)
                .map(|class| (class - CLASS_FILE_OFFSET) as i32)
                .collect()
        })
        .unwrap_or_default();
    majors.sort_unstable();
    Ok(majors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_yields_names_without_the_latest_alias() {
        let index = r#"<a href="../">../</a>
<a href="1.21.4.json">1.21.4.json</a>   28-Sep-2025
<a href="4458.json">4458.json</a>       28-Sep-2025
<a href="latest.json">latest.json</a>   28-Sep-2025"#;
        assert_eq!(names(index), ["1.21.4", "4458"]);
    }
}
