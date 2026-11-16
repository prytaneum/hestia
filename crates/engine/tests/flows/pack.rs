//! A pack format, from the native `replay` fixture: detected by the members it
//! declares, claimed or not by the format, read into a pack and written out
//! through the host.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::Digest;

use proto::provider::Provider;
use proto::transfer::{ExportFormat, ImportFormat};
use proto::warning::WarningInfo;
use serde_json::{json, Value};

use super::*;

const SHA1: &str = fixture::Files::BODY_SHA1;
const PACK: &str = fixture::pack::ID;

fn register(engine: &engine::Engine) {
    engine
        .register_pack(
            fixture::pack::OWNER,
            engine::Standing::Community,
            Arc::new(fixture::pack::Replay::new()),
        )
        .expect("the pack format registers");
}

async fn rig(home: &Path, _files: &fixture::Files) -> engine::Engine {
    engine_with(home, fixture::Source::default()).await
}

pub(super) async fn engine_with(home: &Path, source: fixture::Source) -> engine::Engine {
    let engine = fixture::engine(
        home,
        vec![
            fixture::Flavor::resolving("vanilla"),
            fixture::Flavor::resolving("fabric"),
        ],
        vec![source],
    );
    register(&engine);
    engine
}

/// A replay pack named `file`, naming `mods` as downloads and carrying `bundled` jars and a config under `overrides/`.
pub(super) fn pack_file(
    dir: &Path,
    file: &str,
    (name, version): (&str, &str),
    files: &fixture::Files,
    mods: &[&str],
    bundled: &[&str],
) -> String {
    let path = dir.join(file);
    let options = zip::write::SimpleFileOptions::default();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the pack"));
    let manifest = json!({
        "name": name,
        "versionId": version,
        "gameVersion": "1.21",
        "files": mods
            .iter()
            .map(|jar| json!({ "path": format!("mods/{jar}"), "url": files.url(jar), "sha1": SHA1 }))
            .collect::<Vec<_>>(),
        "trees": { "shared": ["overrides"] },
    });
    zip.start_file("replay.json", options)
        .expect("start the manifest");
    zip.write_all(manifest.to_string().as_bytes())
        .expect("write the manifest");
    for jar in bundled {
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

fn archive(dir: &Path, replay: Value, extra: &[(&str, &str)]) -> PathBuf {
    let bytes: Vec<(&str, &[u8])> = extra
        .iter()
        .map(|(name, body)| (*name, body.as_bytes()))
        .collect();
    archive_of(dir, replay, &bytes)
}

fn archive_of(dir: &Path, replay: Value, extra: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("pack.zip");
    let options = zip::write::SimpleFileOptions::default();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the archive"));
    let replay = replay.to_string();
    for (name, body) in [("replay.json", replay.as_bytes())]
        .into_iter()
        .chain(extra.iter().copied())
    {
        zip.start_file(name, options).expect("start a member");
        zip.write_all(body).expect("write a member");
    }
    zip.finish().expect("finish the archive");
    path
}

fn replay(files: Value) -> Value {
    json!({ "name": "Replayed", "gameVersion": "1.21", "files": files })
}

async fn import(engine: &engine::Engine, path: &Path) -> anyhow::Result<engine::ImportOutcome> {
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    engine
        .import_instance(&path.to_string_lossy(), "", &job(&cancel, &report))
        .await
}

fn data_of(engine: &engine::Engine, outcome: &engine::ImportOutcome) -> PathBuf {
    engine.instances().data_dir(&outcome.record)
}

#[tokio::test]
async fn an_archive_is_the_formats_only_when_it_holds_its_members_and_the_guest_claims_it() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;

    let claimed = engine
        .inspect_archive(&archive(drops.path(), replay(json!([])), &[]).to_string_lossy())
        .await
        .expect("inspect");
    assert_eq!(
        (claimed.name.as_str(), claimed.game_version.as_str()),
        ("Replayed", "1.21")
    );
    assert!(matches!(claimed.format, ImportFormat::Provider(ref p) if p.id == PACK));

    let by_file = archive(drops.path(), replay(json!([])), &[("replay.skip", "")]);
    let mut by_text = replay(json!([]));
    by_text["decline"] = json!(true);
    let by_text = archive(drops.path(), by_text, &[]);
    for (declined, path) in [("a member name", by_file), ("a member's text", by_text)] {
        let refused = engine
            .inspect_archive(&path.to_string_lossy())
            .await
            .expect_err("an archive the guest declines is unrecognised");
        assert!(
            matches!(
                engine::error_info(refused),
                ErrorInfo::ArchiveUnrecognised { .. }
            ),
            "declined on {declined}"
        );
    }

    let undeclared = drops.path().join("other.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&undeclared).unwrap());
    zip.start_file("readme.txt", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.finish().unwrap();
    assert!(engine
        .inspect_archive(&undeclared.to_string_lossy())
        .await
        .is_err());
}

#[tokio::test]
async fn a_read_installs_its_files_and_the_trees_its_sides_take() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let engine = rig(home.path(), &files).await;
    let manifest = json!({
        "name": "Replayed",
        "gameVersion": "1.21",
        "files": [{ "path": "mods/a.jar", "url": files.url("a.jar"), "sha1": SHA1 }],
        "trees": { "shared": ["base"], "client": ["client"], "server": ["server"] },
    });
    let path = archive(
        drops.path(),
        manifest,
        &[
            ("base/options.txt", "shared"),
            ("base/config/a.toml", "a"),
            ("client/options.txt", "client"),
            ("server/server.properties", "server"),
            ("elsewhere/ignored.txt", "not in any tree"),
        ],
    );

    let outcome = import(&engine, &path).await.expect("the pack imports");

    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert!(matches!(outcome.format, ImportFormat::Provider(ref p) if p.id == PACK));
    let data = data_of(&engine, &outcome);
    assert_eq!(
        std::fs::read_to_string(data.join("options.txt")).unwrap(),
        "client",
        "the side's own tree is written after the shared one"
    );
    assert!(data.join("config/a.toml").is_file());
    assert!(
        !data.join("server.properties").exists(),
        "the other side's tree is not this side's"
    );
    assert!(!data.join("ignored.txt").exists());
    assert!(data.join("mods/a.jar").is_file(), "its file is fetched");
}

#[tokio::test]
async fn an_export_goes_through_the_format_and_carries_its_warnings() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;
    let formats = engine.export_formats();
    let offered: Vec<(&str, &str)> = formats
        .iter()
        .map(|info| (info.format.id(), info.extension.as_str()))
        .collect();
    assert_eq!(offered, [("hestia", "hestia"), (PACK, "replay")]);

    let id = engine
        .create_instance("modded", "vanilla", "1.21", None, &[])
        .await
        .expect("create the instance")
        .id;
    let record = engine.instances().require(&id).unwrap();
    std::fs::write(
        engine.instances().data_dir(&record).join("options.txt"),
        "fov:70",
    )
    .unwrap();
    let format = ExportFormat::Provider(Provider::new(PACK, "Replay"));
    let destination = drops.path().join("out.replay");
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let exported = engine
        .export_instance(
            &id,
            &format,
            &destination.to_string_lossy(),
            &[],
            &job(&cancel, &report),
        )
        .await
        .expect("export");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&exported.path).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len())
        .map(|index| zip.by_index(index).unwrap().name().unwrap().to_string())
        .collect();
    assert!(names.contains(&"replay.json".to_string()), "{names:?}");
    assert!(
        names.contains(&"overrides/options.txt".to_string()),
        "{names:?}"
    );
    let mut body = String::new();
    std::io::Read::read_to_string(
        &mut zip.by_name("overrides/options.txt").unwrap(),
        &mut body,
    )
    .unwrap();
    assert_eq!(body, "fov:70");
    assert!(
        matches!(
            exported.warnings.as_slice(),
            [WarningInfo::PackExportWarned { detail, .. }] if detail.contains("references")
        ),
        "{:?}",
        exported.warnings
    );

    let unknown = engine
        .export_instance(
            &id,
            &ExportFormat::Provider(Provider::new("nope", "Nope")),
            &destination.to_string_lossy(),
            &[],
            &job(&cancel, &report),
        )
        .await
        .err()
        .expect("an unknown format");
    assert!(matches!(
        engine::error_info(unknown),
        ErrorInfo::ProviderUnknown { .. }
    ));
}

#[tokio::test]
async fn a_reads_jvm_settings_apply_to_the_instance_the_import_creates() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;
    let manifest = json!({
        "name": "Tuned",
        "gameVersion": "1.21",
        "memory": "2048m",
        "jvmArgs": "-XX:+UseG1GC",
    });

    let outcome = import(&engine, &archive(drops.path(), manifest, &[]))
        .await
        .expect("the pack imports");

    let id = &outcome.record.id;
    assert_eq!(
        engine
            .instances()
            .config_get(id, "memory")
            .unwrap()
            .as_deref(),
        Some("2G")
    );
    assert_eq!(
        engine
            .instances()
            .config_get(id, "jvm-args")
            .unwrap()
            .as_deref(),
        Some("-XX:+UseG1GC")
    );
}

#[tokio::test]
async fn a_read_is_offered_the_loader_keys_of_its_format_and_its_loader_is_the_flavor_they_map_to()
{
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;

    let mut manifest = replay(json!([]));
    manifest["echoLoaders"] = json!(true);
    manifest["loader"] = json!("replay-fabric");
    let inspected = engine
        .inspect_archive(&archive(drops.path(), manifest, &[]).to_string_lossy())
        .await
        .expect("inspect");

    assert_eq!(
        inspected.name, "replay-fabric",
        "only the keys flavors declare for this format are offered"
    );
    assert_eq!(
        inspected.loader, "fabric",
        "an inspect names the flavor, as an import does"
    );
}

#[tokio::test]
async fn a_pack_in_a_plugins_format_is_refused_by_the_single_file_importer() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;
    let record = engine
        .create_instance("modded", "vanilla", "1.21", None, &[])
        .await
        .expect("create the instance");
    let path = archive(drops.path(), replay(json!([])), &[]);
    let spec = ContentAddSpec {
        kind: ContentKind::Mod,
        items: vec![ContentAddItem {
            path: path.to_string_lossy().into_owned(),
            ..ContentAddItem::default()
        }],
        ..ContentAddSpec::default()
    };
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let done = engine
        .add_entry_content(
            EntryRef::Instance(&record.id),
            &spec,
            &job(&cancel, &report),
        )
        .await
        .expect("the batch runs");

    assert!(done.items.is_empty(), "{:?}", done.items);
    assert_eq!(done.failures.len(), 1, "{:?}", done.failures);

    let inspected = engine.inspect_content_file(&path.to_string_lossy()).await;
    assert!(!inspected.valid);
    assert!(inspected.reason.contains("modpack"), "{}", inspected.reason);
}

#[tokio::test]
async fn an_export_is_told_the_size_and_sha512_of_every_pool_file_it_can_reference() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let engine = rig(home.path(), &files).await;
    let manifest = replay(json!([
        { "path": "mods/a.jar", "url": files.url("a.jar"), "sha1": SHA1 },
    ]));
    let imported = import(&engine, &archive(drops.path(), manifest, &[]))
        .await
        .expect("the pack imports");
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let exported = engine
        .export_instance(
            &imported.record.id,
            &ExportFormat::Provider(Provider::new(PACK, "Replay")),
            &drops.path().join("out.replay").to_string_lossy(),
            &[],
            &job(&cancel, &report),
        )
        .await
        .expect("export");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&exported.path).unwrap()).unwrap();
    let mut pool = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("pool.json").unwrap(), &mut pool).unwrap();
    let pool: Value = serde_json::from_str(&pool).unwrap();
    let sha512: String = sha2::Sha512::digest(fixture::Files::BODY)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        pool,
        json!([{
            "path": "mods/a.jar",
            "referenceable": true,
            "size": fixture::Files::BODY.len(),
            "sha512": sha512,
        }])
    );
}

async fn install(engine: &engine::Engine, path: String) -> anyhow::Result<engine::ModpackOutcome> {
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    engine
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
}

#[tokio::test]
async fn a_pack_read_off_disk_tracks_whatever_a_source_recognises() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let mut pack_version = fixture::version("cozy", &files, "1.21", &[]);
    pack_version.files[0].artifact.filename = "cozy.zip".to_string();
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
        ..Default::default()
    };
    let engine = engine_with(home.path(), source).await;
    let path = pack_file(
        drops.path(),
        "cozy.zip",
        ("Cozy", "1.0.0"),
        &files,
        &["sodium.jar"],
        &["iris.jar", "homemade.jar"],
    );

    let outcome = install(&engine, path).await.expect("the pack installs");

    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    assert_eq!(outcome.pack.source, "fixture", "the archive is a release");
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
    assert_eq!(tracked("sodium.jar"), ("fixture", "1.0.0"));
    assert_eq!(tracked("iris.jar"), ("fixture", "1.0.0"));
    assert_eq!(tracked("homemade.jar"), ("file", ""));
    let record = engine.instances().get(&outcome.entry).expect("instance");
    assert!(engine
        .instances()
        .data_dir(&record)
        .join("mods/iris.jar")
        .is_file());
}

#[tokio::test]
async fn a_pack_removed_keeping_its_content_hands_it_to_the_user() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let source = fixture::Source {
        projects: vec![fixture::project("sodium", ContentKind::Mod)],
        versions: vec![fixture::version("sodium", &files, "1.21", &[])],
        ..Default::default()
    };
    let engine = engine_with(home.path(), source).await;
    let path = pack_file(
        drops.path(),
        "cozy.zip",
        ("Cozy", "1.0.0"),
        &files,
        &["sodium.jar"],
        &[],
    );
    let outcome = install(&engine, path).await.expect("the pack installs");
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
    assert!(data.join("config/cozy.toml").is_file());
    assert_eq!(
        engine
            .remove_entry_content(entry, ContentKind::Mod, &sodium, &[])
            .expect("the mod is the user's now"),
        1
    );
}

#[tokio::test]
async fn a_pack_no_source_published_updates_from_a_newer_file() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let engine = engine_with(home.path(), fixture::Source::default()).await;
    let pack = |file: &str, name: &str, version: &str, mods: &[&str]| {
        pack_file(drops.path(), file, (name, version), &files, mods, &[])
    };
    let outcome = install(
        &engine,
        pack("v1.zip", "Cozy", "1.0.0", &["old.jar", "kept.jar"]),
    )
    .await
    .expect("the pack installs");
    let entry = EntryRef::Instance(&outcome.entry);
    assert!(
        engine
            .entry_modpack_update(entry, "")
            .await
            .expect("check")
            .is_none(),
        "no source stands behind this pack"
    );
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let other = pack("other.zip", "Rustic", "3.0.0", &["kept.jar"]);
    let preview = engine
        .preview_entry_modpack_file(entry, Path::new(&other))
        .await
        .expect("preview");
    assert!(!preview.same_pack);
    let refused = engine
        .update_entry_modpack(
            entry,
            &ModpackMove {
                path: other,
                ..ModpackMove::default()
            },
            &job(&cancel, &report),
        )
        .await;
    assert!(refused.is_err(), "another pack needs saying so");

    let newer = pack("v2.zip", "cozy", "2.0.0", &["kept.jar", "new.jar"]);
    let preview = engine
        .preview_entry_modpack_file(entry, Path::new(&newer))
        .await
        .expect("preview");
    assert!(preview.same_pack, "a name matches whatever its case");
    assert!(!preview.downgrade);
    let updated = engine
        .update_entry_modpack(
            entry,
            &ModpackMove {
                path: newer,
                ..ModpackMove::default()
            },
            &job(&cancel, &report),
        )
        .await
        .expect("the pack updates from the file");

    assert_eq!(updated.entry, outcome.entry, "the same entry, in place");
    assert_eq!(updated.pack.version_number, "2.0.0");
    let (listed, _) = engine.entry_content(entry, ContentKind::Mod).expect("list");
    let mut mods: Vec<&str> = listed.iter().map(|i| i.filename.as_str()).collect();
    mods.sort_unstable();
    assert_eq!(mods, ["kept.jar", "new.jar"]);
}

#[tokio::test]
async fn a_disabled_file_in_an_override_tree_is_a_disabled_pool_item_that_can_be_enabled() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;
    let mut manifest = replay(json!([]));
    manifest["trees"] = json!({ "shared": ["overrides"] });
    let extra = [
        ("overrides/mods/sodium.jar.disabled", &b"sodium"[..]),
        ("overrides/mods/lithium.jar", &b"lithium"[..]),
        ("overrides/config/notes.txt.disabled", &b"notes"[..]),
    ];
    let imported = import(&engine, &archive_of(drops.path(), manifest, &extra))
        .await
        .expect("the pack imports");
    let entry = EntryRef::Instance(&imported.record.id);
    let data = data_of(&engine, &imported);

    let (items, _) = engine.entry_content(entry, ContentKind::Mod).unwrap();
    let state = |name: &str| items.iter().find(|item| item.filename == name);
    assert!(!state("sodium.jar").expect("listed").enabled);
    assert!(state("lithium.jar").expect("listed").enabled);
    assert!(!data.join("mods/sodium.jar").exists());
    assert!(!data.join("mods/sodium.jar.disabled").exists());
    assert!(
        data.join("config/notes.txt.disabled").is_file(),
        "a disabled file outside the pool stays a plain file"
    );

    let matched = engine
        .enable_entry_content(entry, ContentKind::Mod, "sodium.jar", true, &[])
        .unwrap();
    assert_eq!(matched, 1);
    assert!(data.join("mods/sodium.jar").is_file());
}
