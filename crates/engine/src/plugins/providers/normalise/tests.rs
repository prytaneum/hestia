use proto::minecraft::Artifact;
use proto::plugin::Grant;
use proto::provider::flavor::RuledArgs;
use proto::rules::{OsRule, Rule, RuleAction};

use super::*;

const REPO: &str = "https://maven.example.com/releases/";
const SHA1: &str = "32a22d349740b9065493b697ae61279f4f85aea2";

fn on(os: &str, arch: &str) -> Target {
    Target {
        os: os.into(),
        arch: arch.into(),
        ..Target::default()
    }
}

fn maven(coordinate: &str, sidecar: bool) -> LibrarySpec {
    LibrarySpec {
        maven: Some(Maven {
            coordinate: coordinate.into(),
            repository: REPO.into(),
            sha1_sidecar: sidecar,
        }),
        ..LibrarySpec::default()
    }
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

fn normaliser<'a>(policy: &'a Policy, target: &'a Target) -> Normaliser<'a> {
    Normaliser { policy, target }
}

#[test]
fn a_maven_library_gets_its_name_path_url_and_file() {
    let declared = complete(maven("org.example:lib:1.2:natives-linux@zip", false)).unwrap();
    let library = declared.pending.library;
    assert_eq!(library.name, "org.example:lib:1.2:natives-linux@zip");
    assert_eq!(
        library.path,
        "org/example/lib/1.2/lib-1.2-natives-linux.zip"
    );
    assert_eq!(
        library.artifact.url,
        "https://maven.example.com/releases/org/example/lib/1.2/lib-1.2-natives-linux.zip"
    );
    assert_eq!(library.artifact.filename, "lib-1.2-natives-linux.zip");
    assert!(library.artifact.checksum.is_none());
    assert!(declared.pending.sidecar.is_none());
}

#[test]
fn a_sidecar_is_sought_only_when_asked_for_and_no_checksum_was_given() {
    let wanted = complete(maven("a:b:1", true)).unwrap().pending;
    assert_eq!(
        wanted.sidecar.as_deref(),
        Some("https://maven.example.com/releases/a/b/1/b-1.jar.sha1")
    );

    let mut given = maven("a:b:1", true);
    given.library.artifact.checksum = parse_sidecar(SHA1);
    assert!(complete(given).unwrap().pending.sidecar.is_none());
}

#[test]
fn a_plain_library_passes_through_untouched() {
    let spec = LibrarySpec::from(Library {
        name: "x:y:1".into(),
        path: "x/y/1/y-1.jar".into(),
        artifact: Artifact {
            url: "https://files.example.com/y.jar".into(),
            ..Artifact::default()
        },
        ..Library::default()
    });
    let pending = complete(spec).unwrap().pending;
    assert_eq!(pending.library.path, "x/y/1/y-1.jar");
    assert!(pending.sidecar.is_none());
}

#[test]
fn a_bad_coordinate_or_repository_refuses_the_library() {
    for coordinate in ["", "a:b", "a:b:../x", "a/b:c:1", "a:b:1:c:d"] {
        assert!(
            matches!(
                complete(maven(coordinate, false)),
                Err(Unresolved::Coordinate(_))
            ),
            "{coordinate:?}"
        );
    }
    for repository in [
        "",
        "not a url",
        "ftp://repo.example.com",
        "https://repo.example.com/?token=1",
        "https://repo.example.com/#frag",
    ] {
        let mut spec = maven("a:b:1", false);
        spec.maven.as_mut().unwrap().repository = repository.into();
        assert!(
            matches!(complete(spec), Err(Unresolved::Repository(_))),
            "{repository:?}"
        );
    }
}

#[test]
fn a_maven_library_may_not_also_name_what_the_host_derives() {
    let named = |edit: fn(&mut Library)| {
        let mut spec = maven("a:b:1", false);
        edit(&mut spec.library);
        complete(spec)
    };
    for result in [
        named(|library| library.name = "a:b:1".into()),
        named(|library| library.path = "a/b/1/b-1.jar".into()),
        named(|library| library.artifact.url = "https://x.example.com/b.jar".into()),
        named(|library| library.artifact.filename = "b.jar".into()),
        named(|library| library.built = true),
    ] {
        assert!(matches!(result, Err(Unresolved::Explicit(_))));
    }
}

#[test]
fn a_sidecar_carries_a_sha1_with_or_without_the_file_name() {
    let expected = parse_sidecar(SHA1).unwrap();
    assert_eq!(expected.algorithm, HashAlgorithm::Sha1);
    for body in [
        SHA1.to_string(),
        format!("{SHA1}\n"),
        format!("  {SHA1}  lib-1.0.jar\n"),
        format!("{SHA1} *lib-1.0.jar\r\n"),
        SHA1.to_uppercase(),
    ] {
        let parsed = parse_sidecar(&body).unwrap_or_else(|| panic!("{body:?}"));
        assert_eq!(parsed.hex, SHA1, "{body:?}");
    }
    for body in ["", "\n", "abc", "<html>not found</html>", &"g".repeat(40)] {
        assert!(parse_sidecar(body).is_none(), "{body:?}");
    }
    assert!(
        parse_sidecar(&"a".repeat(64)).is_none(),
        "a sha256 is not a sha1"
    );
}

#[test]
fn each_sidecar_is_looked_up_once() {
    let pending: Vec<Pending> = ["a:b:1", "c:d:2", "a:b:1"]
        .into_iter()
        .map(|coordinate| complete(maven(coordinate, true)).unwrap().pending)
        .chain([complete(maven("e:f:3", false)).unwrap().pending])
        .collect();
    assert_eq!(
        sidecar_urls(&pending),
        [
            "https://maven.example.com/releases/a/b/1/b-1.jar.sha1",
            "https://maven.example.com/releases/c/d/2/d-2.jar.sha1",
        ]
    );
}

#[tokio::test]
async fn libraries_the_rules_disallow_here_are_dropped() {
    let policy = Policy::new(&Grant::default());
    let rules = |os: &str| LibrarySpec {
        rules: only_on(os),
        ..LibrarySpec::from(Library {
            name: format!("native:{os}:1"),
            ..Library::default()
        })
    };
    let addition = || ClientAddition {
        libraries: vec![
            LibrarySpec::from(Library {
                name: "always:lib:1".into(),
                ..Library::default()
            }),
            rules("linux"),
            rules("windows"),
            rules("osx"),
        ],
        ..ClientAddition::default()
    };
    let names = |layer: ClientLayer| -> Vec<String> {
        layer
            .libraries
            .into_iter()
            .map(|library| library.name)
            .collect()
    };

    for (os, kept) in [
        ("linux", "native:linux:1"),
        ("windows", "native:windows:1"),
        ("osx", "native:osx:1"),
    ] {
        let target = on(os, "x86_64");
        let layer = normaliser(&policy, &target)
            .client(addition())
            .await
            .unwrap();
        assert_eq!(names(layer), ["always:lib:1", kept], "{os}");
    }
}

#[test]
fn arguments_follow_the_rules_in_order() {
    let policy = Policy::new(&Grant::default());
    let ruled = |os: &str, values: &[&str]| {
        Argument::Ruled(RuledArgs {
            value: values.iter().map(|value| value.to_string()).collect(),
            rules: only_on(os),
        })
    };
    let arguments = || {
        vec![
            Argument::from("-Dalways"),
            ruled("osx", &["-XstartOnFirstThread"]),
            ruled("windows", &["-Dwin", "-Dwin2"]),
            Argument::Ruled(RuledArgs {
                value: vec!["-Dunruled".into()],
                rules: Vec::new(),
            }),
        ]
    };

    let linux = on("linux", "x86_64");
    assert_eq!(
        normaliser(&policy, &linux).arguments(arguments()),
        ["-Dalways", "-Dunruled"]
    );
    let windows = on("windows", "x86_64");
    assert_eq!(
        normaliser(&policy, &windows).arguments(arguments()),
        ["-Dalways", "-Dwin", "-Dwin2", "-Dunruled"]
    );
    let mac = on("osx", "arm64");
    assert_eq!(
        normaliser(&policy, &mac).arguments(arguments()),
        ["-Dalways", "-XstartOnFirstThread", "-Dunruled"]
    );
}

#[test]
fn a_server_addition_keeps_its_jar_and_filters_its_flags() {
    let policy = Policy::new(&Grant::default());
    let target = on("linux", "x86_64");
    let layer = normaliser(&policy, &target).server(ServerAddition {
        loader_version: Some("9".into()),
        jar: None,
        jvm_args: vec![
            "-Da".into(),
            Argument::Ruled(RuledArgs {
                value: vec!["-Db".into()],
                rules: only_on("windows"),
            }),
        ],
    });
    assert_eq!(layer.loader_version.as_deref(), Some("9"));
    assert_eq!(layer.jvm_args, ["-Da"]);
}

#[test]
fn the_declared_forms_decode_from_the_wire() {
    let addition: ClientAddition = serde_json::from_value(serde_json::json!({
        "libraries": [
            { "name": "a:b:1", "path": "a/b/1/b-1.jar", "artifact": { "url": "https://x.example.com/b.jar" } },
            { "maven": { "coordinate": "c:d:2", "repository": REPO, "sha1Sidecar": true },
              "rules": [{ "action": "allow", "os": { "name": "linux", "arch": "x86_64" } }] }
        ],
        "jvmArgs": ["-Da", { "value": ["-Db"], "rules": [{ "action": "disallow" }] }],
        "gameArgs": []
    }))
    .unwrap();
    assert_eq!(addition.libraries[0].library.path, "a/b/1/b-1.jar");
    assert!(addition.libraries[0].maven.is_none());
    assert_eq!(addition.libraries[1].rules.len(), 1);
    assert!(addition.libraries[1].maven.as_ref().unwrap().sha1_sidecar);
    assert_eq!(addition.jvm_args[0], Argument::from("-Da"));
    assert!(matches!(addition.jvm_args[1], Argument::Ruled(_)));
}
