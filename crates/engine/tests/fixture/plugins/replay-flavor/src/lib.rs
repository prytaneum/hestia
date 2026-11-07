//! A flavor that re-serves a vanilla-shaped profile. The artifacts sit under
//! the plugin's `base-url` data file, so Hestia downloads them from wherever
//! the test serves them. Two game versions are deliberately bad profiles:
//! `foreign` names a host outside the grant, `unsigned` carries no sha1.

use hestia::proto::content::ContentKind;
use hestia::proto::download::{Checksum, HashAlgorithm};
use hestia::proto::minecraft::{Artifact, GameVersion};
use hestia::proto::profile::{
    AssetIndex, FlavorDescription, InstanceProfile, ServerJar, ServerProfile,
};
use hestia::{Error, Plugin};

const FLAVOR: &str = "replay";
const JAR_SHA1: &str = "32a22d349740b9065493b697ae61279f4f85aea2";
const ASSET_INDEX_SHA1: &str = "62ea787c1f800c091b98678b050453a5ae59d7bc";

struct Replay;

fn check(flavor: &str) -> Result<(), Error> {
    match flavor {
        FLAVOR => Ok(()),
        other => Err(Error::NotFound(format!("unknown flavor `{other}`"))),
    }
}

fn artifact(version: &str, name: &str, sha1: &str) -> Result<Artifact, Error> {
    let base = hestia::data::load::<String>("base-url")?
        .unwrap_or_else(|| "https://files.example.com".into());
    let url = match version {
        "foreign" => format!("https://evil.example.org/{name}"),
        _ => format!("{}/{name}", base.trim_end_matches('/')),
    };
    Ok(Artifact {
        url,
        filename: name.into(),
        size: 0,
        checksum: (version != "unsigned").then(|| Checksum {
            algorithm: HashAlgorithm::Sha1,
            hex: sha1.into(),
        }),
    })
}

impl Plugin for Replay {
    fn flavor_info(flavor: &str) -> Result<FlavorDescription, Error> {
        check(flavor)?;
        Ok(FlavorDescription {
            name: "Replay".into(),
            summary: "Vanilla, re-served by a plugin.".into(),
            loads: None,
            server: true,
            client: true,
        })
    }

    fn flavor_versions(flavor: &str) -> Result<Vec<GameVersion>, Error> {
        check(flavor)?;
        Ok(["1.21", "foreign", "unsigned"]
            .into_iter()
            .map(|id| GameVersion {
                id: id.into(),
                ..GameVersion::default()
            })
            .collect())
    }

    fn flavor_resolve_server(
        flavor: &str,
        version: &str,
        _loader_version: Option<&str>,
    ) -> Result<ServerProfile, Error> {
        check(flavor)?;
        Ok(ServerProfile {
            game_version: version.into(),
            jar: ServerJar::Download(artifact(version, "server.jar", JAR_SHA1)?),
            java_major: 21,
            ..ServerProfile::default()
        })
    }

    fn flavor_resolve_client(
        flavor: &str,
        version: &str,
        _loader_version: Option<&str>,
    ) -> Result<InstanceProfile, Error> {
        check(flavor)?;
        Ok(InstanceProfile {
            game_version: version.into(),
            client: artifact(version, "client.jar", JAR_SHA1)?,
            asset_index: AssetIndex {
                id: version.into(),
                artifact: artifact(version, "assets.json", ASSET_INDEX_SHA1)?,
                total_size: 0,
            },
            java_major: 21,
            main_class: "net.minecraft.client.main.Main".into(),
            ..InstanceProfile::default()
        })
    }
}

hestia::plugin!(Replay);
