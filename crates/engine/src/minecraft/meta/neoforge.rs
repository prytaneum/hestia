//! NeoForge metadata, read from the upstream installer jar on
//! `maven.neoforged.net`.
//!
//! NeoForge publishes no metadata service — no Fabric-style profile endpoint and
//! no Mojang-style manifest. The truth for a version lives *inside* its
//! installer jar: `version.json` (the launch profile, a vanilla-shaped manifest
//! meant to be merged over the base game), `install_profile.json` (the libraries
//! and processors that produce a patched game jar), and `data/{client,server}.lzma`
//! (the binary patches those processors apply). All three are read in-process
//! through the `zip` crate, the same way a `.mrpack` index is.
//!
//! Modrinth's launcher reads a pre-processed copy of this from its own hosted
//! meta service; Hestia reads the installer directly, keeping the upstream-direct
//! rule every other flavor follows and taking libraries from NeoForged's own
//! maven with their own checksums. The *technique* below — resolving the data
//! table and running the processors — follows theseus.

use std::io::Read;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use proto::error::Service;

use super::fetch_text;

const MAVEN: &str = "https://maven.neoforged.net/releases";
const GROUP_PATH: &str = "net/neoforged/neoforge";
/// The 1.20.1 line NeoForge inherited from Forge when it forked, still
/// published under the artifact it forked into and versioned Forge's way
/// (`1.20.1-47.1.106`). It is a separate maven artifact rather than a spelling
/// of the modern one, so it is a second catalogue source — which is how
/// Modrinth's daedalus reaches it too.
const LEGACY_GROUP_PATH: &str = "net/neoforged/forge";

/// Published versions whose installer 404s, so they cannot be installed at all.
/// The same two daedalus carries.
const UNINSTALLABLE: &[&str] = &["1.20.1-47.1.7", "47.1.82"];

/// Whether a version belongs to the legacy 1.20.1 line. Its versions lead with
/// the game version, which the modern schemes never do — they lead with the
/// Minecraft minor (`21.1.244`) or the year (`26.2.0.37-beta`).
fn is_legacy(version: &str) -> bool {
    version.starts_with("1.")
}

/// Every published NeoForge version across both artifacts, oldest first
/// (maven's own order). The two lines share no game version, so the order
/// between them carries no meaning.
pub async fn versions() -> Result<Vec<String>> {
    let (legacy, modern) = futures_util::future::try_join(
        artifact_versions(LEGACY_GROUP_PATH),
        artifact_versions(GROUP_PATH),
    )
    .await?;
    let versions: Vec<String> = legacy
        .into_iter()
        .chain(modern)
        .filter(|version| !UNINSTALLABLE.contains(&version.as_str()))
        .collect();
    if versions.is_empty() {
        bail!("neoforge maven-metadata lists no versions");
    }
    Ok(versions)
}

/// One artifact's published versions.
///
/// Parsed out of `maven-metadata.xml` by scanning `<version>` tags rather than
/// through an XML crate: this is the one XML document in the whole tree, its
/// shape is fixed by the Maven repository contract, and a dependency for it
/// would not earn its place. The repository's own JSON API was the alternative
/// and was rejected — it is a detail of the server software NeoForged happens to
/// run, where `maven-metadata.xml` is guaranteed by the format itself.
async fn artifact_versions(group: &str) -> Result<Vec<String>> {
    let body = fetch_text(
        Service::NeoForge,
        &format!("{MAVEN}/{group}/maven-metadata.xml"),
    )
    .await?;
    Ok(body
        .split("<version>")
        .skip(1)
        .filter_map(|rest| rest.split_once("</version>"))
        .map(|(version, _)| version.trim().to_string())
        .filter(|version| !version.is_empty())
        .collect())
}

/// The Minecraft version a NeoForge version targets, or `None` when the version
/// string does not follow either published scheme.
///
/// NeoForge's version is an adapted semver whose leading fields *are* the game
/// version, so the mapping is arithmetic rather than a lookup — there is no
/// endpoint that states it. Two schemes, split by Minecraft's move to calendar
/// versioning in 2026:
///
/// - `<mc-minor>.<mc-patch>.<build>` — `21.1.244` is the 245th build for 1.21.1,
///   and a zero patch means the `.0` release: `21.0.x` is 1.21, not 1.21.0.
/// - `<year>.<release>.<hotfix>.<build>` — `26.1.2.84` is for 26.1.2, and a zero
///   hotfix again drops: `26.2.0.35-beta` is for 26.2.
///
/// A build that carries **semver build metadata** names the prerelease it was
/// built against there, and that wins over the fields above:
/// `26.1.0.0-alpha.15+pre-3` is for `26.1-pre-3`, not for 26.1. Reading it is
/// what keeps a snapshot build out of the release's catalogue — merged over the
/// release's own `version.json` it would run against a base jar it was never
/// built for, and it would outrank every `-beta` build the release does have.
/// This is a deliberate divergence from Modrinth's published manifest, which
/// files all 15 of these under the release.
///
/// The field arithmetic is otherwise theseus's rule, artifacts included: an
/// April Fools' build (`0.25w14craftmine.5-beta`) maps to a game version that
/// does not exist. Callers filter the result against Mojang's manifest, which
/// drops it — a mapping that names no real version is a failed derivation, not
/// a version to offer.
pub fn game_version(version: &str) -> Option<String> {
    if is_legacy(version) {
        return version.split_once('-').map(|(game, _)| game.to_string());
    }
    let (core, prerelease) = match version.split_once('+') {
        Some((core, metadata)) => (core, Some(metadata)),
        None => (version, None),
    };
    let mut parts = core.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor = parts.next()?;
    let release = if major >= 26 {
        let hotfix = parts.next()?;
        if hotfix == "0" {
            format!("{major}.{minor}")
        } else {
            format!("{major}.{minor}.{hotfix}")
        }
    } else if minor == "0" {
        format!("1.{major}")
    } else {
        format!("1.{major}.{minor}")
    };
    Some(match prerelease {
        Some(prerelease) => format!("{release}-{prerelease}"),
        None => release,
    })
}

/// The maven artifact a version is published under — `forge` for the legacy
/// 1.20.1 line, `neoforge` for everything since. It names the installer, the
/// group path, and the directory the install writes a server's argument file
/// into, so every path a build needs derives from this one answer.
pub fn artifact(version: &str) -> &'static str {
    if is_legacy(version) {
        "forge"
    } else {
        "neoforge"
    }
}

pub fn installer_url(version: &str) -> String {
    let group = if is_legacy(version) {
        LEGACY_GROUP_PATH
    } else {
        GROUP_PATH
    };
    format!(
        "{MAVEN}/{group}/{version}/{}-{version}-installer.jar",
        artifact(version)
    )
}

/// The three things an installer carries that the launcher needs. Read once and
/// held together: the binary patches are addressed by the install profile's own
/// data table, so they are only meaningful beside it.
pub struct Installer {
    /// `version.json` — the launch profile to merge over the base game version.
    pub version: Value,
    /// `install_profile.json` — libraries, processors, and the data table.
    pub profile: Value,
    /// Entries under `data/` (the `.lzma` binary patches and the server arg
    /// files), by their archive-relative name.
    pub data: Vec<(String, Vec<u8>)>,
}

/// Read an installer jar's payload from bytes already on disk or in the cache.
///
/// Only the entries the install needs are extracted, and each is size-capped:
/// an installer is fetched over the network, so a hostile or corrupt archive
/// must not be able to make the daemon allocate without bound.
pub fn read_installer(bytes: &[u8]) -> Result<Installer> {
    const MAX_ENTRY: u64 = 64 * 1024 * 1024;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("neoforge installer is not a readable zip")?;

    let mut version = None;
    let mut profile = None;
    let mut data = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if !entry.is_file() || entry.size() > MAX_ENTRY {
            continue;
        }
        let name = entry.name()?.into_owned();
        let wanted = name == "version.json"
            || name == "install_profile.json"
            || (name.starts_with("data/") && !name.ends_with('/'));
        if !wanted {
            continue;
        }
        let mut buffer = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buffer)?;
        match name.as_str() {
            "version.json" => version = Some(serde_json::from_slice(&buffer)?),
            "install_profile.json" => profile = Some(serde_json::from_slice(&buffer)?),
            _ => data.push((name, buffer)),
        }
    }

    Ok(Installer {
        version: version.context("neoforge installer carries no version.json")?,
        profile: profile.context("neoforge installer carries no install_profile.json")?,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_names_the_game_version_it_targets() {
        assert_eq!(game_version("21.1.244").as_deref(), Some("1.21.1"));
        assert_eq!(game_version("20.2.29-beta").as_deref(), Some("1.20.2"));
        assert_eq!(game_version("20.6.119").as_deref(), Some("1.20.6"));
        assert_eq!(
            game_version("21.0.167").as_deref(),
            Some("1.21"),
            "a zero patch is the .0 release, not 1.21.0"
        );
    }

    #[test]
    fn calendar_versions_take_the_year_scheme() {
        assert_eq!(game_version("26.1.2.84").as_deref(), Some("26.1.2"));
        assert_eq!(
            game_version("26.2.0.35-beta").as_deref(),
            Some("26.2"),
            "a zero hotfix drops, as a zero patch does"
        );
        assert_eq!(game_version("26.1.0.5-beta").as_deref(), Some("26.1"));
    }

    #[test]
    fn build_metadata_names_the_prerelease_a_build_targets() {
        assert_eq!(
            game_version("26.1.0.0-alpha.14+snapshot-11").as_deref(),
            Some("26.1-snapshot-11"),
            "an alpha belongs to the snapshot it was built against, not to 26.1"
        );
        assert_eq!(
            game_version("26.1.0.0-alpha.15+pre-3").as_deref(),
            Some("26.1-pre-3")
        );
    }

    #[test]
    fn the_legacy_line_leads_with_the_game_version_it_is_for() {
        assert_eq!(game_version("1.20.1-47.1.106").as_deref(), Some("1.20.1"));
        assert!(is_legacy("1.20.1-47.1.106"));
        assert!(
            !is_legacy("21.1.244"),
            "the modern line leads with the minor"
        );
        assert!(!is_legacy("26.2.0.37-beta"), "nor does the calendar line");
    }

    #[test]
    fn each_line_names_its_own_maven_artifact() {
        assert_eq!(artifact("1.20.1-47.1.106"), "forge");
        assert_eq!(artifact("21.1.244"), "neoforge");
        assert_eq!(
            installer_url("1.20.1-47.1.106"),
            "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-47.1.106/forge-1.20.1-47.1.106-installer.jar"
        );
        assert_eq!(
            installer_url("21.1.244"),
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/21.1.244/neoforge-21.1.244-installer.jar"
        );
    }

    #[test]
    fn an_unparseable_version_names_nothing() {
        assert_eq!(game_version("nonsense"), None);
        assert_eq!(game_version("21"), None, "a game version needs two fields");
        assert_eq!(
            game_version("26.1"),
            None,
            "the year scheme needs its hotfix field"
        );
    }

    #[test]
    fn the_craftmine_artifact_maps_to_no_real_version() {
        // Reproduced from theseus deliberately: the caller drops it by checking
        // the result against Mojang's manifest.
        assert_eq!(
            game_version("0.25w14craftmine.5-beta").as_deref(),
            Some("1.0.25w14craftmine")
        );
    }

    #[test]
    fn a_non_zip_installer_is_refused() {
        assert!(read_installer(b"not a zip at all").is_err());
    }
}
