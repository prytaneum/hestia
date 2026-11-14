//! A pack format contributed by a plugin, from the `replay-pack` fixture:
//! detected by the members it declares, claimed or not by the guest, read into
//! a pack whose files are vetted before anything is fetched, and written out
//! through the host.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine::{PluginRecord, PluginRuntime};
use proto::plugin::{Manifest, Permission, PermissionScope, ScopeKind};
use proto::provider::Provider;
use proto::transfer::{ExportFormat, ImportFormat};
use proto::warning::WarningInfo;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::content_source;
use super::launch_hooks::NoHost;
use super::*;

const SHA1: &str = fixture::Files::BODY_SHA1;
const PACK: &str = "replay-pack.replay";

fn plugin_record(manifest: &Manifest, wasm: &[u8], hosts: &[&str]) -> PluginRecord {
    let mut grant = manifest.grant();
    grant.permissions.insert(
        Permission::NetworkConnect,
        PermissionScope::of(ScopeKind::Hosts, hosts.iter().copied()),
    );
    PluginRecord {
        id: manifest.id.clone(),
        wasm_sha256: Some(
            Sha256::digest(wasm)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ),
        grant,
        ..PluginRecord::default()
    }
}

async fn contributions(home: &Path, hosts: &[&str]) -> (Manifest, engine::Contributions) {
    let manifest: Manifest = toml::from_str(fixture::pack::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    let wasm = std::fs::read(fixture::pack::component()).expect("replay-pack component");
    let runtime = PluginRuntime::new(
        Arc::new(NoHost),
        home.join("plugin-cache"),
        Arc::new(engine::ProcessSupervisor::new(home.join("processes"))),
    )
    .expect("runtime");
    let data = home.join("replay-pack-data");
    std::fs::create_dir_all(&data).expect("data dir");
    let settings = engine::PluginSettings::new(home.join("replay-pack-settings.json"), Vec::new());
    let record = plugin_record(&manifest, &wasm, hosts);
    let plugin = Arc::new(runtime.load(&record, &wasm, data, settings).expect("load"));
    let connected = engine::Contributions::connect(&plugin, &manifest, false).await;
    (manifest, connected)
}

async fn rig(home: &Path, files: &fixture::Files) -> engine::Engine {
    let engine = fixture::engine(home, vec![fixture::Flavor::resolving("vanilla")], vec![]);
    let (manifest, connected) = contributions(home, &["127.0.0.1"]).await;
    connected.register(&engine, &manifest.id);
    let (plugin, index) = content_source::load(home, files, &["127.0.0.1"]);
    engine::Contributions::connect(&plugin, &index, false)
        .await
        .register(&engine, &index.id);
    engine
}

fn archive(dir: &Path, replay: Value, extra: &[(&str, &str)]) -> PathBuf {
    let path = dir.join("pack.zip");
    let options = zip::write::SimpleFileOptions::default();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the archive"));
    let replay = replay.to_string();
    for (name, body) in [("replay.json", replay.as_str())]
        .into_iter()
        .chain(extra.iter().copied())
    {
        zip.start_file(name, options).expect("start a member");
        zip.write_all(body.as_bytes()).expect("write a member");
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
        .import_instance(
            &path.to_string_lossy(),
            "",
            &CallChain::default(),
            &job(&cancel, &report),
        )
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

    let declined = archive(drops.path(), replay(json!([])), &[("replay.skip", "")]);
    let refused = engine
        .inspect_archive(&declined.to_string_lossy())
        .await
        .expect_err("an archive the guest declines is unrecognised");
    assert!(matches!(
        engine::error_info(refused),
        ErrorInfo::ArchiveUnrecognised { .. }
    ));

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
async fn a_read_that_delegates_resolves_through_the_source_that_owns_the_manifest() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let engine = rig(home.path(), &files).await;
    let manifest = json!({
        "name": "Delegated",
        "gameVersion": "1.21",
        "resolve": {
            "source": "static-index.index",
            "manifest": { "files": [{ "path": "mods/b.jar", "url": "b.jar", "sha1": SHA1 }] },
        },
        "trees": { "shared": ["overrides"] },
    });
    let path = archive(drops.path(), manifest, &[("overrides/config/b.toml", "b")]);

    let outcome = import(&engine, &path).await.expect("the pack imports");

    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    let data = data_of(&engine, &outcome);
    assert!(data.join("mods/b.jar").is_file(), "the source's file");
    assert!(data.join("config/b.toml").is_file(), "the pack's own tree");
    let pack = engine
        .entry_modpack(EntryRef::Instance(&outcome.record.id))
        .expect("status")
        .expect("a pack is recorded");
    assert_eq!(pack.source, "static-index.index");
    assert_eq!(pack.name, "Delegated", "the pack names itself");
}

#[tokio::test]
async fn a_delegation_to_a_source_that_is_not_there_is_refused() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;
    let manifest = json!({
        "name": "Lost",
        "gameVersion": "1.21",
        "resolve": { "source": "nowhere", "manifest": {} },
    });

    import(&engine, &archive(drops.path(), manifest, &[]))
        .await
        .err()
        .expect("no such source");
    assert!(engine.instances().list().is_empty());
}

#[tokio::test]
async fn an_answer_naming_a_path_outside_the_entry_or_a_file_the_rule_refuses_is_refused_whole() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let files = fixture::Files::serve().await;
    let engine = rig(home.path(), &files).await;
    let good = json!({ "path": "mods/a.jar", "url": files.url("a.jar"), "sha1": SHA1 });

    let cases = [
        (
            "an escaping path",
            json!({ "path": "../evil.jar", "url": files.url("a.jar"), "sha1": SHA1 }),
        ),
        (
            "an absolute path",
            json!({ "path": "/etc/evil.jar", "url": files.url("a.jar"), "sha1": SHA1 }),
        ),
        (
            "a host the plugin was not granted",
            json!({ "path": "mods/c.jar", "url": "https://evil.example.org/c.jar", "sha1": SHA1 }),
        ),
        (
            "no checksum",
            json!({ "path": "mods/c.jar", "url": files.url("c.jar") }),
        ),
        (
            "a malformed checksum",
            json!({ "path": "mods/c.jar", "url": files.url("c.jar"), "sha1": "abc" }),
        ),
    ];
    for (name, bad) in cases {
        let path = archive(drops.path(), replay(json!([good, bad])), &[]);
        let error = import(&engine, &path)
            .await
            .err()
            .unwrap_or_else(|| panic!("{name} must be refused"));
        assert!(error.to_string().contains("refused"), "{name}: {error}");
        assert!(
            engine.instances().list().is_empty(),
            "{name}: nothing is created from a refused answer"
        );
    }
    assert!(!home.path().join("evil.jar").exists());
}

#[tokio::test]
async fn an_override_tree_that_leaves_the_pack_is_refused() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;

    for tree in ["..", "../up", "/abs", "a/../b", "."] {
        let manifest = json!({
            "name": "Replayed",
            "gameVersion": "1.21",
            "trees": { "shared": [tree] },
        });
        let path = archive(drops.path(), manifest, &[("up/x.txt", "x")]);
        let error = import(&engine, &path)
            .await
            .err()
            .unwrap_or_else(|| panic!("`{tree}` must be refused"));
        assert!(error.to_string().contains("refused"), "{tree}: {error}");
        assert!(engine.instances().list().is_empty());
    }
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
    assert_eq!(
        offered,
        [("hestia", "hestia"), ("mrpack", "mrpack"), (PACK, "replay")]
    );

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
        ErrorInfo::ExportFormatUnknown { .. }
    ));
}

#[tokio::test]
async fn an_export_naming_a_file_it_does_not_hold_is_refused() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path(), &fixture::Files::serve().await).await;
    let id = engine
        .create_instance("escape", "vanilla", "1.21", None, &[])
        .await
        .expect("create the instance")
        .id;
    let destination = drops.path().join("out.replay");
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let error = engine
        .export_instance(
            &id,
            &ExportFormat::Provider(Provider::new(PACK, "Replay")),
            &destination.to_string_lossy(),
            &[],
            &job(&cancel, &report),
        )
        .await
        .err()
        .expect("the guest named a file outside the instance");

    assert!(error.to_string().contains("refused"), "{error}");
    assert!(!destination.exists(), "no archive is left behind");
}
