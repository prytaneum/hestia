use super::*;

fn vet(declared: &[&str], jvm: &[&str], game: &[&str]) -> LaunchPatch {
    LaunchPatch::vetted(
        "demo",
        &declared.iter().map(|key| key.to_string()).collect(),
        jvm.iter().map(|arg| arg.to_string()).collect(),
        game.iter().map(|arg| arg.to_string()).collect(),
    )
}

fn dropped(patch: &LaunchPatch) -> Vec<&str> {
    patch
        .dropped
        .iter()
        .map(|warning| match warning {
            WarningInfo::LaunchArgDropped { arg, .. } => arg.as_str(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn named_tuning_flags_and_declared_properties_pass() {
    let patch = vet(
        &["my.flag"],
        &[
            "-XX:+UseG1GC",
            "-XX:-AlwaysPreTouch",
            "-XX:MaxGCPauseMillis=50",
            "-XX:G1HeapRegionSize=16M",
            "-XX:ReservedCodeCacheSize=512m",
            "-Dmy.flag=on",
        ],
        &["--width", "1280", "--height", "720", "--fullscreen"],
    );
    assert_eq!(patch.jvm_args.len(), 6);
    assert_eq!(
        patch.game_args,
        ["--width", "1280", "--height", "720", "--fullscreen"]
    );
    assert!(patch.dropped.is_empty());
}

#[test]
fn every_way_to_load_code_or_run_a_command_is_dropped() {
    let escapes = [
        "-javaagent:x.jar",
        "-agentlib:jdwp=transport=dt_socket",
        "-agentpath:/tmp/x.so",
        "-XX:OnError=sh -c evil",
        "-XX:OnOutOfMemoryError=evil",
        "-XX:Flags=.hotspotrc",
        "-cp",
        "-classpath",
        "--class-path",
        "--module-path",
        "-p",
        "--add-modules",
        "-Xbootclasspath/a:x.jar",
        "-Djava.library.path=/tmp",
        "-Djava.system.class.loader=Evil",
        "-Djava.security.manager=allow",
        "-Dlog4j.configurationFile=http://evil",
        "-Dlog4j2.formatMsgNoLookups=false",
        "-Djdk.module.path=/tmp",
        "-Dundeclared=1",
        "-XX:+UseSomethingElse",
        "-XX:MaxGCPauseMillis=fast",
        "-XX:MaxGCPauseMillis=",
        "-XX:G1HeapRegionSize=16MB",
        "-XX:+UseG1GC=true",
        "-Dmy.flag",
        "-Xmx1G",
        "-jar",
    ];
    let declared = [
        "java.library.path",
        "java.system.class.loader",
        "java.security.manager",
        "log4j.configurationFile",
        "log4j2.formatMsgNoLookups",
        "jdk.module.path",
        "my.flag",
    ];
    let patch = vet(&declared, &escapes, &[]);
    assert!(patch.jvm_args.is_empty(), "{:?}", patch.jvm_args);
    assert_eq!(dropped(&patch), escapes);
}

#[test]
fn only_the_named_game_arguments_pass() {
    let patch = vet(
        &[],
        &[],
        &[
            "--accessToken",
            "secret",
            "--width",
            "wide",
            "--height",
            "--username",
            "Notch",
            "--fullscreen",
        ],
    );
    assert_eq!(patch.game_args, ["--fullscreen"]);
    assert_eq!(
        dropped(&patch),
        [
            "--accessToken",
            "secret",
            "--width",
            "wide",
            "--height",
            "--username",
            "Notch"
        ]
    );
}

#[test]
fn a_long_dropped_argument_is_cut_in_its_warning() {
    let long = format!("-javaagent:{}", "a".repeat(1000));
    let patch = vet(&[], &[&long], &[]);
    assert_eq!(dropped(&patch)[0].chars().count(), SHOWN_CHARS);
}

#[test]
fn patches_fold_in_the_order_they_ran() {
    let first = vet(&[], &["-XX:+UseG1GC"], &["--fullscreen"]);
    let second = vet(&[], &["-XX:+UseZGC", "-javaagent:x.jar"], &[]);
    let folded = first.then(second);
    assert_eq!(folded.jvm_args, ["-XX:+UseG1GC", "-XX:+UseZGC"]);
    assert_eq!(folded.game_args, ["--fullscreen"]);
    assert_eq!(dropped(&folded), ["-javaagent:x.jar"]);
}

#[test]
fn a_server_keeps_the_jvm_allowlist_and_drops_every_game_argument() {
    let patch = LaunchPatch::vetted_for_server(
        "demo",
        &BTreeSet::new(),
        vec!["-XX:+UseZGC".into(), "-javaagent:x.jar".into()],
        vec!["--fullscreen".into(), "--width".into(), "1280".into()],
    );
    assert_eq!(patch.jvm_args, ["-XX:+UseZGC"]);
    assert!(patch.game_args.is_empty());
    assert_eq!(
        dropped(&patch),
        ["-javaagent:x.jar", "--fullscreen", "--width", "1280"]
    );
}

#[test]
fn a_wrapper_is_kept_only_when_it_resolves_and_nests_first_outermost() {
    let path = PathBuf::from("/usr/bin/gamemoderun");
    let kept = vet(&[], &[], &[]).with_wrapper("demo", Some("gamemode".into()), |name| {
        (name == "gamemode").then(|| path.clone())
    });
    assert_eq!(kept.wrappers, std::slice::from_ref(&path));
    assert!(kept.dropped.is_empty());

    let unknown = vet(&[], &[], &[]).with_wrapper("demo", Some("nope".into()), |_| None);
    assert!(unknown.wrappers.is_empty());
    assert_eq!(dropped(&unknown), ["nope"]);

    let none = vet(&[], &[], &[]).with_wrapper("demo", None, |_| unreachable!());
    assert!(none.wrappers.is_empty() && none.dropped.is_empty());

    let other = LaunchPatch {
        wrappers: vec![PathBuf::from("/usr/bin/mangohud")],
        ..Default::default()
    };
    assert_eq!(
        kept.then(other).wrappers,
        [path, PathBuf::from("/usr/bin/mangohud")]
    );
}
