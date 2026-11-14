//! A flavor that re-serves a vanilla-shaped profile. The artifacts sit under
//! the plugin's `base-url` data file, so Hestia downloads them from wherever
//! the test serves them. Two game versions are deliberately bad profiles:
//! `foreign` names a host outside the grant, `unsigned` carries no checksum and
//! `generated` says it carries none on purpose. `sha256` verifies by sha256 and
//! `wrongsum` names a sha256 the download does not match.
//!
//! `onvanilla` answers with what it adds to the vanilla profile instead of a
//! whole one, and `onvanilla-foreign` adds a library from a host outside the
//! grant. Four more addition the declarative forms: `onvanilla-maven` adds a
//! Maven coordinate with a `.sha1` sidecar, `onvanilla-maven-missing` one whose
//! sidecar is not there and `onvanilla-maven-bad` a malformed coordinate;
//! `onvanilla-rules` gates a library and arguments by the machine's os and
//! `onvanilla-target` echoes the target facts it was asked with.
//!
//! Four more exercise `build`. On the server, `built` runs the declared
//! managed-Java program in `/build` and returns the jar it wrote, `outside`
//! claims a file outside `/build`, `symlink` has the program leave a symlink and returns it and `empty`
//! produces nothing. On the client, `built` copies the vanilla jar it is shown
//! into the build area as a built library. Every other version has nothing to
//! build.
//!
//! Build plans, on the server (`live` is the imperative form: it runs the step
//! itself with `exec`, and its output must reach progress as it prints): `plan` runs one declared step, `plan-narrate` one
//! whose output is filtered into progress, `plan-steps` two of which the first
//! `creates` a marker, `plan-slow` two of which the first outlasts a cancel,
//! `plan-fetch` has the host download a file the step then copies. The `refused-*` versions
//! are plans the host must refuse before running anything: an undeclared
//! program, arguments no invocation fits, an undeclared environment variable,
//! a `cwd`, `creates`, download destination or output outside `/build`, and a
//! `${data}` mount.
//!
//! Tree outputs: on the server, `tree` writes a directory into the data
//! directory and one into the libraries root, and launches from an args file
//! with a Windows alternative; `badargs` names an args file that leaves the data
//! directory. On the client, `tree` builds a library that is off the classpath
//! into the libraries root and `tree-entry` aims a tree at a data directory a
//! client does not have.

use hestia::proto::content::ContentKind;
use hestia::proto::download::{Checksum, HashAlgorithm};
use hestia::proto::minecraft::{Artifact, GameVersion};
use hestia::proto::profile::{
    ArgsFile, AssetIndex, FlavorDescription, InstanceProfile, Library, OsArgsFile, ServerJar,
    ServerLaunch, ServerProfile,
};
use hestia::proto::provider::flavor::{
    Argument, Build, BuildFetch, BuildOutput, BuildPlan, BuildStep, LineFilter, BuildQuery, BuildSide, ClientAddition,
    ClientResolution, Destination, Info, LibrarySpec, Maven, ResolveClient, ResolveQuery, ResolveServer,
    RuledArgs, ServerAddition, ServerResolution, Versions,
};
use hestia::proto::rules::{OsRule, Rule, RuleAction};
use hestia::{Error, Plugin, Provide, Provided};
use std::time::Duration;

const FLAVOR: &str = "replay";
const JAR_SHA1: &str = "32a22d349740b9065493b697ae61279f4f85aea2";
const BUILT_JAR: &str = "built.jar";
const BUILT_LIBRARY: &str = "example/built/1.0/built-1.0.jar";
const JAR_SHA256: &str = "8b9c02d7974c19e06b56f75b0d9e4f969fa896f722a5679209ce0b9c040caa2c";
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
        "foreign" | "onvanilla-foreign" => format!("https://evil.example.org/{name}"),
        _ => format!("{}/{name}", base.trim_end_matches('/')),
    };
    Ok(Artifact {
        url,
        filename: name.into(),
        size: 0,
        checksum: checksum(version, sha1),
        unhashed: version == "generated",
    })
}

fn checksum(version: &str, sha1: &str) -> Option<Checksum> {
    let sha256 = |hex: &str| Checksum {
        algorithm: HashAlgorithm::Sha256,
        hex: hex.into(),
    };
    match version {
        "unsigned" | "generated" => None,
        "sha256" => Some(sha256(JAR_SHA256)),
        "wrongsum" => Some(sha256(&"0".repeat(64))),
        _ => Some(Checksum {
            algorithm: HashAlgorithm::Sha1,
            hex: sha1.into(),
        }),
    }
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
    Ok(["1.21", "foreign", "unsigned", "generated", "sha256", "wrongsum", "onvanilla", "onvanilla-foreign", "onvanilla-maven", "onvanilla-maven-missing", "onvanilla-maven-bad", "onvanilla-rules", "onvanilla-target", "built", "ordered", "expand", "outside", "symlink", "empty", "tree", "badargs", "tree-entry", "live", "plan", "plan-narrate", "plan-steps", "plan-slow", "plan-fetch", "refused-program", "refused-args", "refused-env", "refused-cwd", "refused-creates", "refused-dest", "refused-output", "refused-mount"]
        .into_iter()
        .map(|id| GameVersion {
            id: id.into(),
            ..GameVersion::default()
        })
        .collect())
}

fn loader_library(version: &str) -> Result<Library, Error> {
    Ok(Library {
        name: "example:loader:1.0".into(),
        path: "example/loader/1.0/loader-1.0.jar".into(),
        artifact: artifact(version, "loader.jar", JAR_SHA1)?,
        ..Library::default()
    })
}

fn only_on(os: &str) -> Vec<Rule> {
    vec![Rule {
        action: RuleAction::Allow,
        os: Some(OsRule {
            name: Some(os.into()),
            ..OsRule::default()
        }),
        features: None,
    }]
}

fn maven(coordinate: &str) -> Result<LibrarySpec, Error> {
    let repository = hestia::data::load::<String>("base-url")?
        .unwrap_or_else(|| "https://files.example.com".into());
    Ok(LibrarySpec {
        maven: Some(Maven {
            coordinate: coordinate.into(),
            repository,
            sha1_sidecar: true,
        }),
        ..LibrarySpec::default()
    })
}

fn gated_library(version: &str, name: &str, os: &str) -> Result<LibrarySpec, Error> {
    Ok(LibrarySpec {
        library: Library {
            name: name.into(),
            path: format!("{}.jar", name.replace(':', "/")),
            artifact: artifact(version, "gated.jar", JAR_SHA1)?,
            ..Library::default()
        },
        rules: only_on(os),
        ..LibrarySpec::default()
    })
}

fn gated_args(flag: &str, os: &str) -> Argument {
    Argument::Ruled(RuledArgs {
        value: vec![flag.into()],
        rules: only_on(os),
    })
}

fn addition(version: &str, query: &ResolveQuery) -> Result<ClientAddition, Error> {
    let target = &query.target;
    let mut addition = ClientAddition {
        loader_version: Some("9.9".into()),
        libraries: vec![loader_library(version)?.into()],
        main_class: "example.loader.Main".into(),
        jvm_args: vec!["-Dloader".into()],
        game_args: vec!["--loader".into()],
    };
    match version {
        "onvanilla-maven" => addition.libraries.push(maven("example:mvn:1.0")?),
        "onvanilla-maven-missing" => addition.libraries.push(maven("example:nosum:1.0")?),
        "onvanilla-maven-bad" => addition.libraries.push(maven("example:bad")?),
        "onvanilla-rules" => {
            addition
                .libraries
                .push(gated_library(version, "example:here:1", &target.os)?);
            addition
                .libraries
                .push(gated_library(version, "example:elsewhere:1", "plan9")?);
            addition.jvm_args.push(gated_args("-Dhere", &target.os));
            addition.jvm_args.push(gated_args("-Delsewhere", "plan9"));
        }
        "onvanilla-target" => addition.game_args.push(Argument::Plain(format!(
            "--target={}/{}/{}/{:?}",
            target.os, target.arch, target.path_separator, target.java_major
        ))),
        _ => {}
    }
    Ok(addition)
}

fn resolve_server(flavor: &str, query: ResolveQuery) -> Result<ServerResolution, Error> {
    check(flavor)?;
    let version = query.version.as_str();
    if version.starts_with("onvanilla") {
        return Ok(ServerResolution::OnVanilla(ServerAddition {
            loader_version: Some("9.9".into()),
            jar: Some(ServerJar::Download(artifact(
                version,
                "loader-server.jar",
                JAR_SHA1,
            )?)),
            jvm_args: vec![
                "-Dloader".into(),
                gated_args("-Dhere", &query.target.os),
                gated_args("-Delsewhere", "plan9"),
            ],
        }));
    }
    let jar = match version {
        "built" | "ordered" | "expand" | "outside" | "symlink" | "empty" | "live" | "plan" | "plan-narrate"
        | "plan-steps" | "plan-slow" | "plan-fetch" | "refused-program" | "refused-args"
        | "refused-env" | "refused-cwd" | "refused-creates" | "refused-dest" | "refused-output"
        | "refused-mount" => ServerJar::Built {
            filename: BUILT_JAR.into(),
        },
        _ => ServerJar::Download(artifact(version, "server.jar", JAR_SHA1)?),
    };
    let args = |path: &str| ArgsFile {
        path: format!("{path}/unix_args.txt"),
        alternatives: vec![OsArgsFile {
            os: "windows".into(),
            path: format!("{path}/win_args.txt"),
        }],
    };
    let launch = match version {
        "tree" => ServerLaunch::ArgsFile(args("tree")),
        "badargs" => ServerLaunch::ArgsFile(args("../escape")),
        _ => ServerLaunch::Jar,
    };
    Ok(ServerResolution::Full(Box::new(ServerProfile {
        game_version: version.into(),
        jar,
        launch,
        java_major: 21,
        ..ServerProfile::default()
    })))
}

fn resolve_client(flavor: &str, query: ResolveQuery) -> Result<ClientResolution, Error> {
    check(flavor)?;
    let version = query.version.as_str();
    if version.starts_with("onvanilla") {
        return Ok(ClientResolution::OnVanilla(addition(version, &query)?));
    }
    Ok(ClientResolution::Full(Box::new(InstanceProfile {
        game_version: version.into(),
        client: artifact(version, "client.jar", JAR_SHA1)?,
        libraries: matches!(version, "built" | "tree")
            .then(|| Library {
                name: "example:built:1.0".into(),
                path: BUILT_LIBRARY.into(),
                built: true,
                classpath: version != "tree",
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
    })))
}

fn produced(path: &str, target: &str) -> BuildPlan {
    BuildPlan {
        outputs: vec![BuildOutput {
            path: path.into(),
            target: target.into(),
            destination: None,
        }],
        ..BuildPlan::default()
    }
}

fn tree(path: &str, target: &str, destination: Destination) -> BuildOutput {
    BuildOutput {
        path: path.into(),
        target: target.into(),
        destination: Some(destination),
    }
}

fn write_tree(files: &[(&str, &str)]) -> Result<(), Error> {
    for (path, contents) in files {
        let path = std::path::Path::new(path);
        std::fs::create_dir_all(path.parent().expect("a file path"))?;
        std::fs::write(path, contents)?;
    }
    Ok(())
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
        None,
    )?;
    match ran.status {
        Some(0) => Ok(()),
        _ => Err(Error::Failed("the builder program failed".into())),
    }
}

fn step(args: &[&str]) -> BuildStep {
    BuildStep {
        program: "builder".into(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        ..BuildStep::default()
    }
}

fn planned(steps: Vec<BuildStep>) -> BuildPlan {
    BuildPlan {
        steps,
        ..produced("/build/built.jar", BUILT_JAR)
    }
}

fn plan(version: &str, base: &str) -> Option<BuildPlan> {
    let creating = |args: &[&str], created: &str| BuildStep {
        creates: vec![created.into()],
        ..step(args)
    };
    Some(match version {
        "plan" => planned(vec![creating(&["build"], "/build/built.jar")]),
        "plan-narrate" => planned(vec![BuildStep {
            narrate: Some(LineFilter {
                skip_prefixes: vec!["[".into()],
            }),
            ..step(&["narrate"])
        }]),
        "plan-steps" => planned(vec![
            creating(&["count"], "/build/marker"),
            step(&["build"]),
        ]),
        "plan-slow" => planned(vec![step(&["slow"]), creating(&["second"], "/build/second")]),
        "plan-fetch" => BuildPlan {
            fetch: vec![BuildFetch {
                url: format!("{}/server.jar", base.trim_end_matches('/')),
                checksum: Some(Checksum {
                    algorithm: HashAlgorithm::Sha1,
                    hex: JAR_SHA1.into(),
                }),
                dest: "/build/dl/tool.jar".into(),
            }],
            ..planned(vec![step(&["copy"])])
        },
        "refused-program" => planned(vec![BuildStep {
            program: "ghost".into(),
            ..step(&["x"])
        }]),
        "refused-args" => planned(vec![BuildStep {
            program: "strict".into(),
            ..step(&["other"])
        }]),
        "refused-env" => planned(vec![BuildStep {
            env: [("EVIL".to_string(), "1".to_string())].into(),
            ..step(&["build"])
        }]),
        "refused-cwd" => planned(vec![BuildStep {
            cwd: Some("/build/../etc".into()),
            ..step(&["build"])
        }]),
        "refused-creates" => planned(vec![creating(&["build"], "/build/../escaped")]),
        "refused-dest" => BuildPlan {
            fetch: vec![BuildFetch {
                url: format!("{}/server.jar", base.trim_end_matches('/')),
                checksum: Some(Checksum {
                    algorithm: HashAlgorithm::Sha1,
                    hex: JAR_SHA1.into(),
                }),
                dest: "/data/tool.jar".into(),
            }],
            ..planned(vec![step(&["build"])])
        },
        "refused-output" => BuildPlan {
            steps: vec![step(&["build"])],
            ..produced("/build/../escaped.jar", BUILT_JAR)
        },
        "refused-mount" => planned(vec![step(&["expand", "${data}/x"])]),
        _ => return None,
    })
}

fn build(flavor: &str, query: BuildQuery) -> Result<BuildPlan, Error> {
    check(flavor)?;
    if query.side == BuildSide::Server {
        let base = hestia::data::load::<String>("base-url")?
            .unwrap_or_else(|| "https://files.example.com".into());
        if let Some(planned) = plan(&query.version, &base) {
            return Ok(planned);
        }
    }
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
        (BuildSide::Server, "empty") => Ok(BuildPlan::default()),
        (BuildSide::Server, "live") => {
            run_builder("narrate")?;
            Ok(produced("/build/built.jar", BUILT_JAR))
        }
        (BuildSide::Server, "tree") => {
            write_tree(&[
                ("/build/entry/tree/unix_args.txt", "unix"),
                ("/build/entry/tree/win_args.txt", "win"),
                ("/build/libs/example/loader/1.0/loader-1.0.jar", "loader"),
            ])?;
            Ok(BuildPlan {
                outputs: vec![
                    tree("/build/entry", "", Destination::Entry),
                    tree("/build/libs", "", Destination::Libraries),
                ],
                ..BuildPlan::default()
            })
        }
        (BuildSide::Client, "tree") => {
            write_tree(&[("/build/libs/example/built/1.0/built-1.0.jar", "off")])?;
            Ok(BuildPlan {
                outputs: vec![tree("/build/libs", "", Destination::Libraries)],
                ..BuildPlan::default()
            })
        }
        (BuildSide::Client, "tree-entry") => {
            write_tree(&[("/build/entry/a.txt", "a")])?;
            Ok(BuildPlan {
                outputs: vec![tree("/build/entry", "", Destination::Entry)],
                ..BuildPlan::default()
            })
        }
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
