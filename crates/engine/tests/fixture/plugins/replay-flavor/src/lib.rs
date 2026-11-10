//! A flavor that re-serves a vanilla-shaped profile. The artifacts sit under
//! the plugin's `base-url` data file, so Hestia downloads them from wherever
//! the test serves them. Two game versions are deliberately bad profiles:
//! `foreign` names a host outside the grant, `unsigned` carries no sha1.
//!
//! Four more exercise `build`. On the server, `built` runs the declared
//! managed-Java program in `/build` and returns the jar it wrote, `outside`
//! claims a file outside `/build`, `symlink` has the program leave a symlink and returns it and `empty`
//! produces nothing. On the client, `built` copies the vanilla jar it is shown
//! into the build area as a built library. Every other version has nothing to
//! build.

use hestia::proto::content::ContentKind;
use hestia::proto::download::{Checksum, HashAlgorithm};
use hestia::proto::minecraft::{Artifact, GameVersion};
use hestia::proto::profile::{
    AssetIndex, FlavorDescription, InstanceProfile, Library, ServerJar, ServerProfile,
};
use hestia::proto::provider::flavor::{
    Build, BuildDone, BuildOutput, BuildQuery, BuildSide, Info, ResolveClient, ResolveQuery,
    ResolveServer, Versions,
};
use hestia::{Error, Plugin, Provide, Provided};
use std::time::Duration;

const FLAVOR: &str = "replay";
const JAR_SHA1: &str = "32a22d349740b9065493b697ae61279f4f85aea2";
const BUILT_JAR: &str = "built.jar";
const BUILT_LIBRARY: &str = "example/built/1.0/built-1.0.jar";
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

fn info(flavor: &str) -> Result<FlavorDescription, Error> {
    check(flavor)?;
    Ok(FlavorDescription {
        name: "Replay".into(),
        summary: "Vanilla, re-served by a plugin.".into(),
        loads: None,
        server: true,
        client: true,
        ..FlavorDescription::default()
    })
}

fn versions(flavor: &str) -> Result<Vec<GameVersion>, Error> {
    check(flavor)?;
    Ok(["1.21", "foreign", "unsigned", "built", "ordered", "expand", "outside", "symlink", "empty"]
        .into_iter()
        .map(|id| GameVersion {
            id: id.into(),
            ..GameVersion::default()
        })
        .collect())
}

fn resolve_server(flavor: &str, query: ResolveQuery) -> Result<ServerProfile, Error> {
    check(flavor)?;
    let version = query.version.as_str();
    let jar = match version {
        "built" | "ordered" | "expand" | "outside" | "symlink" | "empty" => ServerJar::Built {
            filename: BUILT_JAR.into(),
        },
        _ => ServerJar::Download(artifact(version, "server.jar", JAR_SHA1)?),
    };
    Ok(ServerProfile {
        game_version: version.into(),
        jar,
        java_major: 21,
        ..ServerProfile::default()
    })
}

fn resolve_client(flavor: &str, query: ResolveQuery) -> Result<InstanceProfile, Error> {
    check(flavor)?;
    let version = query.version.as_str();
    Ok(InstanceProfile {
        game_version: version.into(),
        client: artifact(version, "client.jar", JAR_SHA1)?,
        libraries: (version == "built")
            .then(|| Library {
                name: "example:built:1.0".into(),
                path: BUILT_LIBRARY.into(),
                built: true,
                ..Library::default()
            })
            .into_iter()
            .collect(),
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

fn produced(path: &str, target: &str) -> BuildDone {
    BuildDone {
        outputs: vec![BuildOutput {
            path: path.into(),
            target: target.into(),
        }],
    }
}

fn run_builder(mode: &str) -> Result<(), Error> {
    run_builder_with(&[mode])
}

fn run_builder_with(args: &[&str]) -> Result<(), Error> {
    let ran = hestia::process::exec_with(
        "builder",
        args,
        &[],
        Duration::from_secs(10),
        Some("/build"),
    )?;
    match ran.status {
        Some(0) => Ok(()),
        _ => Err(Error::Failed("the builder program failed".into())),
    }
}

fn build(flavor: &str, query: BuildQuery) -> Result<BuildDone, Error> {
    check(flavor)?;
    match (query.side, query.version.as_str()) {
        (BuildSide::Server, "built") => {
            run_builder("build")?;
            Ok(produced("/build/built.jar", BUILT_JAR))
        }
        (BuildSide::Server, "ordered") => {
            run_builder("ordered")?;
            Ok(produced("/build/built.jar", BUILT_JAR))
        }
        (BuildSide::Server, "expand") => {
            run_builder_with(&["expand", "${build}/x"])?;
            Ok(produced("/build/built.jar", BUILT_JAR))
        }
        (BuildSide::Server, "outside") => {
            std::fs::write("/data/built.jar", b"outside")?;
            Ok(produced("/data/built.jar", BUILT_JAR))
        }
        (BuildSide::Server, "symlink") => {
            run_builder("symlink")?;
            Ok(produced("/build/link.jar", BUILT_JAR))
        }
        (BuildSide::Server, "empty") => Ok(BuildDone::default()),
        (BuildSide::Client, "built") => {
            let vanilla = query
                .vanilla_jar
                .ok_or_else(|| Error::Invalid("the client has a vanilla jar".into()))?;
            std::fs::copy(vanilla, "/build/lib.jar")?;
            Ok(produced("/build/lib.jar", BUILT_LIBRARY))
        }
        _ => Err(Error::Unsupported("nothing to build".into())),
    }
}

impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|flavor, _| info(flavor))
            .on::<Versions>(|flavor, _| versions(flavor))
            .on::<ResolveServer>(resolve_server)
            .on::<ResolveClient>(resolve_client)
            .on::<Build>(build)
            .finish()
    }
}

hestia::plugin!(Replay);
