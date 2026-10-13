//! The cross-subsystem flows, driven against fixture providers. What these pin
//! is ordering: which step runs before which, and what is left on disk when one
//! of them fails.

mod fixture;

use engine::{Cancel, EntryRef, Job, LaunchRequest, ServerCreateSpec};
use proto::content::{ContentAddItem, ContentAddSpec, ContentKind};
use proto::error::ErrorInfo;
use proto::instance::QuickPlay;
use proto::minecraft::ProvisionProgress;
use proto::modpack::{ModpackRef, ModpackTarget};

fn job<'a>(cancel: &'a Cancel, report: &'a (dyn Fn(&ProvisionProgress) + Send + Sync)) -> Job<'a> {
    Job::new(report, cancel)
}

fn add(projects: &[&str]) -> ContentAddSpec {
    ContentAddSpec {
        kind: ContentKind::Mod,
        items: projects
            .iter()
            .map(|p| ContentAddItem {
                project: (*p).to_string(),
                ..ContentAddItem::default()
            })
            .collect(),
        ..ContentAddSpec::default()
    }
}

fn spec(flavor: &str) -> ServerCreateSpec {
    ServerCreateSpec {
        name: "smp".into(),
        flavor: flavor.into(),
        version: "1.21".into(),
        eula: true,
        ..ServerCreateSpec::default()
    }
}

#[tokio::test]
async fn the_registry_the_engine_was_built_over_is_the_one_it_answers_from() {
    let home = tempfile::tempdir().expect("temp home");
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fixture")],
        vec![],
    );

    let flavors = engine.minecraft().server_flavors();
    assert_eq!(flavors.len(), 1, "only the injected flavor is offered");
    assert_eq!(flavors[0].id, "fixture");
    assert!(
        !flavors.iter().any(|f| f.id == "vanilla"),
        "the shipped registry must not leak into an engine built over another"
    );

    let versions = engine
        .minecraft()
        .server_versions("fixture")
        .await
        .expect("the fixture catalogue");
    assert_eq!(versions.len(), 2);
}

#[tokio::test]
async fn a_create_that_cannot_resolve_registers_nothing() {
    let home = tempfile::tempdir().expect("temp home");
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::failing("fixture")],
        vec![],
    );
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    engine
        .provision_server(spec("fixture"), &job(&cancel, &report))
        .await
        .expect_err("the flavor cannot resolve");

    assert!(
        engine.servers().list().is_empty(),
        "resolve runs before the record exists, so a failure leaves no port claim behind"
    );
}

#[tokio::test]
async fn an_unknown_flavor_is_refused_by_name() {
    let home = tempfile::tempdir().expect("temp home");
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fixture")],
        vec![],
    );
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let refused = engine
        .provision_server(spec("nosuch"), &job(&cancel, &report))
        .await
        .expect_err("no such flavor");
    assert!(
        matches!(engine::error_info(refused), ErrorInfo::Internal { .. }),
        "an unknown flavor is refused, not resolved against the first one"
    );
    assert!(engine.servers().list().is_empty());
}

#[tokio::test]
async fn a_cancelled_create_stops_before_it_claims_anything() {
    let home = tempfile::tempdir().expect("temp home");
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fixture")],
        vec![],
    );
    let cancel = Cancel::new();
    cancel.cancel();
    let report = |_: &ProvisionProgress| {};

    let stopped = engine
        .provision_server(spec("fixture"), &job(&cancel, &report))
        .await
        .expect_err("cancelled");

    assert!(
        engine::is_cancelled(&stopped),
        "a cancellation is not a failure"
    );
    assert!(
        engine.servers().list().is_empty(),
        "the checkpoint sits between resolve and create, so nothing is registered"
    );
}

/// An instance to install content into, built over the fixture flavor.
async fn instance(home: &std::path::Path, source: fixture::Source) -> (engine::Engine, String) {
    let engine = fixture::engine(
        home,
        vec![fixture::Flavor::resolving("fabric")],
        vec![source],
    );
    let record = engine
        .create_instance("modded", "fabric", "1.21", None, &[])
        .await
        .expect("create the instance");
    (engine, record.id)
}

#[tokio::test]
async fn a_batch_installs_its_roots_and_their_required_dependencies_once() {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![
            fixture::project("sodium", ContentKind::Mod),
            fixture::project("iris", ContentKind::Mod),
            fixture::project("fabric-api", ContentKind::Mod),
        ],
        versions: vec![
            fixture::version("sodium", &files, "1.21", &["fabric-api"]),
            fixture::version("iris", &files, "1.21", &["fabric-api"]),
            fixture::version("fabric-api", &files, "1.21", &[]),
        ],
    };
    let (engine, id) = instance(home.path(), source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let (installed, failures) = engine
        .add_entry_content(
            EntryRef::Instance(&id),
            &add(&["sodium", "iris"]),
            &job(&cancel, &report),
        )
        .await
        .expect("the batch runs");

    assert!(failures.is_empty(), "no item should fail: {failures:?}");
    let names: Vec<&str> = installed.iter().map(|i| i.project_id.as_str()).collect();
    assert_eq!(
        names.iter().filter(|n| **n == "fabric-api").count(),
        1,
        "a dependency shared across the batch installs once, got {names:?}"
    );
    assert_eq!(installed.len(), 3, "two roots plus the shared dependency");
}

fn local_mod(dir: &std::path::Path, name: &str) -> String {
    use std::io::Write;
    let path = dir.join(name);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the jar"));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .expect("start the manifest");
    zip.write_all(name.as_bytes()).expect("write the manifest");
    zip.finish().expect("finish the jar");
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn a_local_file_a_source_recognises_installs_tracked_and_alone() {
    let home = tempfile::tempdir().expect("temp home");
    let drops = tempfile::tempdir().expect("temp drops");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![
            fixture::project("sodium", ContentKind::Mod),
            fixture::project("fabric-api", ContentKind::Mod),
        ],
        versions: vec![
            fixture::version("sodium", &files, "1.21", &["fabric-api"]),
            fixture::version("fabric-api", &files, "1.21", &[]),
        ],
    };
    let (engine, id) = instance(home.path(), source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let spec = ContentAddSpec {
        kind: ContentKind::Mod,
        items: ["sodium.jar", "homemade.jar"]
            .iter()
            .map(|name| ContentAddItem {
                path: local_mod(drops.path(), name),
                ..ContentAddItem::default()
            })
            .collect(),
        ..ContentAddSpec::default()
    };

    let (installed, failures) = engine
        .add_entry_content(EntryRef::Instance(&id), &spec, &job(&cancel, &report))
        .await
        .expect("the batch runs");

    assert!(failures.is_empty(), "no item should fail: {failures:?}");
    assert_eq!(installed.len(), 2, "a dependency is not pulled in");
    let sodium = &installed[0];
    assert_eq!(sodium.source, "fixture");
    assert_eq!(sodium.project_id, "sodium");
    assert_eq!(sodium.version_id, "sodium-1");
    assert_eq!(sodium.version_number, "1.0.0");
    assert!(
        !sodium.sha1.is_empty(),
        "the local bytes are what is hashed"
    );
    let homemade = &installed[1];
    assert_eq!(homemade.source, "file");
    assert!(homemade.project_id.is_empty());
    assert_eq!(homemade.filename, "homemade.jar");
}

#[tokio::test]
async fn a_file_kept_local_can_later_move_to_a_source_that_recognises_it() {
    let home = tempfile::tempdir().expect("temp home");
    let drops = tempfile::tempdir().expect("temp drops");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
    };
    let (engine, id) = instance(home.path(), source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let spec = ContentAddSpec {
        kind: ContentKind::Mod,
        items: vec![ContentAddItem {
            path: local_mod(drops.path(), "sodium.jar"),
            source: proto::content::LOCAL_SOURCE.to_string(),
            ..ContentAddItem::default()
        }],
        ..ContentAddSpec::default()
    };

    let (installed, failures) = engine
        .add_entry_content(EntryRef::Instance(&id), &spec, &job(&cancel, &report))
        .await
        .expect("the batch runs");

    assert!(failures.is_empty(), "no item should fail: {failures:?}");
    assert_eq!(installed[0].source, proto::content::LOCAL_SOURCE);
    assert!(installed[0].project_id.is_empty());

    let matches = engine
        .entry_content_matches(EntryRef::Instance(&id), ContentKind::Mod, "sodium.jar")
        .await
        .expect("the installed file is looked up");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].project.id, "sodium");

    let moved = engine
        .set_entry_content_version(
            EntryRef::Instance(&id),
            ContentKind::Mod,
            "sodium.jar",
            "1.0.0",
            "fixture",
            &job(&cancel, &report),
        )
        .await
        .expect("the item moves to the source that recognises it");
    assert_eq!(moved[0].source, "fixture");
    assert_eq!(moved[0].project_id, "sodium");
    let (listed, _) = engine
        .entry_content(EntryRef::Instance(&id), ContentKind::Mod)
        .expect("list");
    assert_eq!(
        listed.len(),
        1,
        "the move replaces the record, not adds one"
    );
    assert_eq!(listed[0].source, "fixture");
}

fn local_pack(
    dir: &std::path::Path,
    name: &str,
    index: serde_json::Value,
    jars: &[&str],
) -> String {
    use std::io::Write;
    let path = dir.join(name);
    let options = zip::write::SimpleFileOptions::default();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the pack"));
    zip.start_file("modrinth.index.json", options)
        .expect("start the index");
    zip.write_all(index.to_string().as_bytes())
        .expect("write the index");
    for jar in jars {
        zip.start_file(format!("overrides/mods/{jar}"), options)
            .expect("start an override jar");
        zip.write_all(jar.as_bytes())
            .expect("write an override jar");
    }
    zip.start_file("overrides/config/cozy.toml", options)
        .expect("start a config");
    zip.write_all(b"cozy = true").expect("write a config");
    zip.finish().expect("finish the pack");
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn a_pack_read_off_disk_tracks_whatever_a_source_recognises() {
    let home = tempfile::tempdir().expect("temp home");
    let drops = tempfile::tempdir().expect("temp drops");
    let files = fixture::Files::serve().await;
    let mut pack_version = fixture::version("cozy", &files, "1.21", &[]);
    pack_version.files[0].artifact.filename = "cozy.mrpack".to_string();
    let source = fixture::Source {
        projects: vec![
            fixture::project("cozy", ContentKind::Modpack),
            fixture::project("sodium", ContentKind::Mod),
            fixture::project("iris", ContentKind::Mod),
        ],
        versions: vec![
            pack_version,
            fixture::version("sodium", &files, "1.21", &[]),
            fixture::version("iris", &files, "1.21", &[]),
        ],
    };
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fabric")],
        vec![source],
    );
    let index = serde_json::json!({
        "formatVersion": 1,
        "name": "Cozy",
        "versionId": "1.0.0",
        "dependencies": { "minecraft": "1.21", "fabric-loader": "0.16.0" },
        "files": [
            { "path": "mods/sodium.jar", "downloads": [files.url("sodium.jar")] },
            { "path": "mods/stray.jar", "downloads": [files.url("stray.jar")] },
        ],
    });
    let path = local_pack(
        drops.path(),
        "cozy.mrpack",
        index,
        &["iris.jar", "homemade.jar"],
    );
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let outcome = engine
        .install_instance_modpack(
            &ModpackRef {
                path,
                ..ModpackRef::default()
            },
            &ModpackTarget::Create {
                name: String::new(),
            },
            &job(&cancel, &report),
        )
        .await
        .expect("the pack installs");

    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(outcome.pack.source, "fixture", "the archive is a release");
    assert_eq!(outcome.pack.project_id, "cozy");
    assert_eq!(outcome.pack.version_id, "cozy-1");
    let overrides: Vec<&str> = outcome
        .pack
        .overrides
        .iter()
        .map(|o| o.path.as_str())
        .collect();
    assert_eq!(
        overrides,
        ["config/cozy.toml"],
        "an override jar is pool content, not a game-directory file"
    );

    let (listed, untracked) = engine
        .entry_content(EntryRef::Instance(&outcome.entry), ContentKind::Mod)
        .expect("list");
    assert!(untracked.is_empty(), "nothing is left unaccounted for");
    let tracked = |filename: &str| {
        let item = listed
            .iter()
            .find(|i| i.filename == filename)
            .unwrap_or_else(|| panic!("{filename} is in the pool"));
        assert_eq!(item.origin, "modpack:cozy");
        (item.source.as_str(), item.version_number.as_str())
    };
    assert_eq!(
        tracked("sodium.jar"),
        ("fixture", "1.0.0"),
        "an index file is identified by its download URL"
    );
    assert_eq!(
        tracked("iris.jar"),
        ("fixture", "1.0.0"),
        "an override jar is identified by its hash"
    );
    assert_eq!(tracked("stray.jar"), ("file", ""));
    assert_eq!(tracked("homemade.jar"), ("file", ""));

    let record = engine
        .instances()
        .get(&outcome.entry)
        .expect("the instance the pack created");
    assert!(
        engine
            .instances()
            .data_dir(&record)
            .join("mods/iris.jar")
            .is_file(),
        "an override jar still reaches the load dir, through the mirror"
    );
}

#[tokio::test]
async fn a_pack_removed_keeping_its_content_hands_it_to_the_user() {
    let home = tempfile::tempdir().expect("temp home");
    let drops = tempfile::tempdir().expect("temp drops");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
    };
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fabric")],
        vec![source],
    );
    let index = serde_json::json!({
        "formatVersion": 1,
        "name": "Cozy",
        "versionId": "1.0.0",
        "dependencies": { "minecraft": "1.21", "fabric-loader": "0.16.0" },
        "files": [
            { "path": "mods/sodium.jar", "downloads": [files.url("sodium.jar")] },
        ],
    });
    let path = local_pack(drops.path(), "cozy.mrpack", index, &[]);
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let outcome = engine
        .install_instance_modpack(
            &ModpackRef {
                path,
                ..ModpackRef::default()
            },
            &ModpackTarget::Create {
                name: String::new(),
            },
            &job(&cancel, &report),
        )
        .await
        .expect("the pack installs");
    let entry = EntryRef::Instance(&outcome.entry);
    let sodium = ["sodium".to_string()];
    assert!(
        engine
            .remove_entry_content(entry, ContentKind::Mod, &sodium, &[])
            .is_err(),
        "a pack's mod is the pack's to remove"
    );

    let result = engine
        .remove_entry_modpack(entry, true)
        .expect("the pack is released");

    assert_eq!(result.released_files, 1);
    assert_eq!((result.removed_files, result.removed_overrides), (0, 0));
    assert!(engine.entry_modpack(entry).expect("status").is_none());
    let record = engine.instances().get(&outcome.entry).expect("instance");
    let data = engine.instances().data_dir(&record);
    assert!(data.join("mods/sodium.jar").is_file());
    assert!(
        data.join("config/cozy.toml").is_file(),
        "a file the pack wrote stays where it is"
    );
    let (listed, _) = engine.entry_content(entry, ContentKind::Mod).expect("list");
    assert!(listed.iter().all(|i| i.origin.is_empty()));
    assert_eq!(
        engine
            .remove_entry_content(entry, ContentKind::Mod, &sodium, &[])
            .expect("the mod is the user's now"),
        1
    );
}

#[tokio::test]
async fn another_launchers_jars_are_tracked_where_a_source_publishes_them() {
    use std::io::Write;
    let home = tempfile::tempdir().expect("temp home");
    let drops = tempfile::tempdir().expect("temp drops");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
    };
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fabric")],
        vec![source],
    );
    let path = drops.path().join("cozy.zip");
    let options = zip::write::SimpleFileOptions::default();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the archive"));
    for (name, body) in [
        ("instance.cfg", "name=Cozy\n"),
        (
            "mmc-pack.json",
            r#"{"components":[
                {"uid":"net.minecraft","version":"1.21"},
                {"uid":"net.fabricmc.fabric-loader","version":"0.16.9"}]}"#,
        ),
        (".minecraft/mods/sodium.jar", "sodium"),
        (".minecraft/mods/homemade.jar", "homemade"),
    ] {
        zip.start_file(name, options).expect("start a member");
        zip.write_all(body.as_bytes()).expect("write a member");
    }
    zip.finish().expect("finish the archive");
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let outcome = engine
        .import_instance(&path.to_string_lossy(), "", &job(&cancel, &report))
        .await
        .expect("the instance imports");

    let (listed, _) = engine
        .entry_content(EntryRef::Instance(&outcome.record.id), ContentKind::Mod)
        .expect("list");
    let source_of = |filename: &str| {
        listed
            .iter()
            .find(|i| i.filename == filename)
            .map(|i| i.source.as_str())
    };
    assert_eq!(source_of("sodium.jar"), Some("fixture"));
    assert_eq!(source_of("homemade.jar"), Some("file"));
    match outcome.warnings.as_slice() {
        [proto::warning::WarningInfo::ImportFilesUntracked { files, .. }] => assert_eq!(
            files,
            &["homemade.jar"],
            "only what no source publishes is reported as untracked"
        ),
        other => panic!("expected one untracked warning, got {other:?}"),
    }
}

#[tokio::test]
async fn a_root_that_fails_does_not_stop_the_batch() {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
    };
    let (engine, id) = instance(home.path(), source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let (installed, failures) = engine
        .add_entry_content(
            EntryRef::Instance(&id),
            &add(&["ghost", "sodium"]),
            &job(&cancel, &report),
        )
        .await
        .expect("the batch runs past the failure");

    assert_eq!(installed.len(), 1, "the good root still installs");
    assert_eq!(failures.len(), 1, "the unknown project is reported, once");
    assert_eq!(failures[0].item, "ghost");
}

#[tokio::test]
async fn what_a_batch_installed_is_what_the_entry_then_lists() {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
    };
    let (engine, id) = instance(home.path(), source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    engine
        .add_entry_content(
            EntryRef::Instance(&id),
            &add(&["sodium"]),
            &job(&cancel, &report),
        )
        .await
        .expect("install");

    let (listed, _) = engine
        .entry_content(EntryRef::Instance(&id), ContentKind::Mod)
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].project_id, "sodium");
    assert!(listed[0].enabled);

    // The index and the disk agree: removing it takes the file with it.
    assert_eq!(
        engine
            .remove_entry_content(
                EntryRef::Instance(&id),
                ContentKind::Mod,
                &["sodium".to_string()],
                &[]
            )
            .expect("remove"),
        1
    );
    let (after, _) = engine
        .entry_content(EntryRef::Instance(&id), ContentKind::Mod)
        .expect("list");
    assert!(after.is_empty(), "the index follows the removal");
}

#[tokio::test]
async fn disabling_an_item_keeps_it_installed() {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
    };
    let (engine, id) = instance(home.path(), source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    engine
        .add_entry_content(
            EntryRef::Instance(&id),
            &add(&["sodium"]),
            &job(&cancel, &report),
        )
        .await
        .expect("install");

    let matched = engine
        .enable_entry_content(
            EntryRef::Instance(&id),
            ContentKind::Mod,
            "sodium",
            false,
            &[],
        )
        .expect("disable");
    assert_eq!(matched, 1);

    let (listed, _) = engine
        .entry_content(EntryRef::Instance(&id), ContentKind::Mod)
        .expect("list");
    assert_eq!(listed.len(), 1, "a disabled item is still installed");
    assert!(!listed[0].enabled);
}

/// An instance with `sodium` installed at the older of two published versions,
/// so the next `update` has somewhere to move it.
async fn updatable(home: &std::path::Path, icon: &str) -> (engine::Engine, String, fixture::Files) {
    let files = fixture::Files::serve().await;
    let mut sodium = fixture::project("sodium", ContentKind::Mod);
    sodium.icon_url = icon.to_string();
    let older = fixture::version("sodium", &files, "1.21", &[]);
    let mut newer = fixture::version("sodium", &files, "1.21", &[]);
    newer.id = "sodium-2".into();
    newer.version_number = "2.0.0".into();
    let source = fixture::Source {
        projects: vec![sodium],
        versions: vec![newer, older.clone()],
    };
    let (engine, id) = instance(home, source).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let mut spec = add(&["sodium"]);
    spec.items[0].version = older.id.clone();
    engine
        .add_entry_content(EntryRef::Instance(&id), &spec, &job(&cancel, &report))
        .await
        .expect("install the older version");
    (engine, id, files)
}

#[tokio::test]
async fn an_update_moves_the_version_and_leaves_the_project_alone() {
    let home = tempfile::tempdir().expect("temp home");
    let icon = "https://example.invalid/sodium.png";
    let (engine, id, _files) = updatable(home.path(), icon).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let updated = engine
        .update_entry_content(
            EntryRef::Instance(&id),
            ContentKind::Mod,
            &["sodium".to_string()],
            &job(&cancel, &report),
        )
        .await
        .expect("update");

    assert_eq!(updated.len(), 1, "the item moves to the newer version");
    assert_eq!(updated[0].version_id, "sodium-2");
    let (listed, _) = engine
        .entry_content(EntryRef::Instance(&id), ContentKind::Mod)
        .expect("list");
    assert_eq!(listed[0].icon_url, icon);
    assert_eq!(listed[0].title, "sodium");
    assert_eq!(listed[0].project_id, "sodium");
}

#[tokio::test]
async fn updating_a_disabled_item_leaves_it_out_of_the_load_dir() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id, _files) = updatable(home.path(), "").await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let entry_dir = home.path().join("instances").join("modded");
    let mirrored = entry_dir.join("data").join("mods").join("sodium.jar");

    engine
        .enable_entry_content(
            EntryRef::Instance(&id),
            ContentKind::Mod,
            "sodium",
            false,
            &[],
        )
        .expect("disable");
    assert!(!mirrored.exists(), "disabling takes it out of the load dir");

    engine
        .update_entry_content(
            EntryRef::Instance(&id),
            ContentKind::Mod,
            &["sodium".to_string()],
            &job(&cancel, &report),
        )
        .await
        .expect("update");

    let (listed, _) = engine
        .entry_content(EntryRef::Instance(&id), ContentKind::Mod)
        .expect("list");
    assert_eq!(listed[0].version_id, "sodium-2");
    assert!(!listed[0].enabled, "the index keeps it disabled");
    assert!(
        entry_dir.join("mods").join("sodium.jar").is_file(),
        "the managed copy moves to the new version"
    );
    assert!(
        !mirrored.exists(),
        "an update must not put a disabled item back where the game loads it"
    );
}

/// An instance ready to launch: a fixture flavor, a Java runtime already on
/// disk, and a signed-in account whose token has not expired.
async fn launchable(home: &std::path::Path) -> (engine::Engine, String) {
    let files = fixture::Files::serve().await;
    let engine = fixture::engine(
        home,
        vec![fixture::Flavor::serving("fabric", &files)],
        vec![],
    );
    fixture::java_runtime(home, 21);
    fixture::account(home, "player");
    let record = engine
        .create_instance("modded", "fabric", "1.21", None, &[])
        .await
        .expect("create the instance");
    (engine, record.id)
}

fn launch<'a>(instance: &'a str, quick_play: Option<QuickPlay>) -> LaunchRequest<'a> {
    LaunchRequest {
        instance,
        account: "",
        session_seq: 1,
        reconcile: true,
        quick_play,
        offline: false,
    }
}

#[tokio::test]
async fn a_launch_materializes_the_client_jar_under_mojangs_own_name() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id) = launchable(home.path()).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let prepared = engine
        .prepare_instance(launch(&id, None), &job(&cancel, &report))
        .await
        .expect("prepare");

    // A modloader booting off the module path filters the vanilla jar out by
    // this exact name, so it is a contract, not a naming preference.
    let jar = home.path().join("meta/versions/1.21/1.21.jar");
    assert!(jar.is_file(), "the client jar lands at {}", jar.display());
    assert!(
        prepared.plan.args.iter().any(|a| a.contains("1.21.jar")) || prepared.plan.program.exists(),
        "the plan names the runtime it just ensured"
    );
}

#[tokio::test]
async fn a_session_gets_its_own_log_file_outside_the_backed_up_tree() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id) = launchable(home.path()).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let mut request = launch(&id, None);
    request.session_seq = 3;
    let prepared = engine
        .prepare_instance(request, &job(&cancel, &report))
        .await
        .expect("prepare");

    assert!(
        prepared.log_file.ends_with("session-3.log"),
        "each concurrent session tails its own file, got {}",
        prepared.log_file.display()
    );
    let instance_dir = home.path().join("instances");
    assert!(
        prepared.log_file.starts_with(&instance_dir)
            && !prepared
                .log_file
                .components()
                .any(|c| c.as_os_str() == "data"),
        "session logs live outside data/, so a backup does not carry them"
    );
    assert!(
        prepared.log_file.parent().is_some_and(|d| d.is_dir()),
        "the session log directory is created before the plan names it"
    );
}

#[tokio::test]
async fn joining_a_world_that_is_not_there_is_refused_before_any_work() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id) = launchable(home.path()).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let refused = engine
        .prepare_instance(
            launch(&id, Some(QuickPlay::World("ghost".into()))),
            &job(&cancel, &report),
        )
        .await
        .map(|_| ())
        .expect_err("no such world");

    assert!(matches!(
        engine::error_info(refused),
        ErrorInfo::WorldNotFound { .. }
    ));
    // The gate sits ahead of the materialize, so nothing was downloaded for a
    // launch that was never going to happen.
    assert!(
        !home.path().join("meta/versions").exists(),
        "the quick-play gate runs before the client jar is ensured"
    );
}

#[tokio::test]
async fn joining_a_realm_by_anything_but_its_id_is_refused() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id) = launchable(home.path()).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let refused = engine
        .prepare_instance(
            launch(&id, Some(QuickPlay::Realm("Our Realm".into()))),
            &job(&cancel, &report),
        )
        .await
        .map(|_| ())
        .expect_err("a realm name is not an id");

    assert!(matches!(
        engine::error_info(refused),
        ErrorInfo::InvalidValue { .. }
    ));
}

#[tokio::test]
async fn what_a_session_joined_is_offered_again_until_it_cannot_be_joined() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id) = launchable(home.path()).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let prepared = engine
        .prepare_instance(launch(&id, None), &job(&cancel, &report))
        .await
        .expect("prepare");
    let args = &prepared.plan.args;
    let log = args
        .iter()
        .position(|a| a == "--quickPlayPath")
        .map(|i| std::path::PathBuf::from(&args[i + 1]))
        .expect("a 1.20+ launch names its quick play log");
    assert_eq!(
        log.parent(),
        prepared.log_file.parent(),
        "the log sits with the session's own files, outside data/"
    );
    assert!(engine.recent_plays().is_empty(), "nothing joined yet");

    let saves = home.path().join("instances/modded/data/saves");
    std::fs::create_dir_all(saves.join("New World")).expect("create the save");
    std::fs::write(
        &log,
        r#"[{"type":"singleplayer","id":"New World","name":"Home","lastPlayedTime":"2026-10-06T10:00:00.000000Z","gamemode":"creative"}]"#,
    )
    .expect("the game writes its log");

    let plays = engine.recent_plays();
    assert_eq!(plays.len(), 1);
    assert_eq!(plays[0].instance, id);
    assert_eq!(plays[0].target, QuickPlay::World("New World".into()));
    assert_eq!(plays[0].name, "Home");

    std::fs::remove_dir_all(saves.join("New World")).expect("delete the save");
    assert!(
        engine.recent_plays().is_empty(),
        "a deleted world is not offered"
    );
}

#[tokio::test]
async fn a_cancelled_launch_stops_at_a_checkpoint() {
    let home = tempfile::tempdir().expect("temp home");
    let (engine, id) = launchable(home.path()).await;
    let cancel = Cancel::new();
    cancel.cancel();
    let report = |_: &ProvisionProgress| {};

    let stopped = engine
        .prepare_instance(launch(&id, None), &job(&cancel, &report))
        .await
        .map(|_| ())
        .expect_err("cancelled");
    assert!(
        engine::is_cancelled(&stopped),
        "a cancellation is not a failure"
    );
}
