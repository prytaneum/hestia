//! An archive format contributed by a plugin, from the `replay-format`
//! fixture: detected by its marker, read into a blueprint, and landed by
//! copying exactly the members the guest lists.

use std::io::Write;
use std::sync::Arc;

use engine::{PluginRecord, PluginRuntime};
use proto::format::FormatBlueprint;
use proto::plugin::Manifest;
use proto::transfer::ImportFormat;
use sha2::{Digest, Sha256};

use super::launch_hooks::NoHost;
use super::*;

async fn load(home: &std::path::Path) -> (engine::Engine, Manifest, engine::Contributions) {
    let engine = fixture::engine(home, vec![fixture::Flavor::resolving("vanilla")], vec![]);
    let manifest: Manifest = toml::from_str(fixture::format::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    let wasm = std::fs::read(fixture::format::component()).expect("replay-format component");
    let runtime = PluginRuntime::new(
        Arc::new(NoHost),
        home.join("plugin-cache"),
        Arc::new(engine::ProcessSupervisor::new(home.join("processes"))),
    )
    .expect("runtime");
    let record = PluginRecord {
        id: manifest.id.clone(),
        wasm_sha256: Some(
            Sha256::digest(&wasm)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ),
        grant: manifest.grant(),
        ..PluginRecord::default()
    };
    let data = home.join("replay-format-data");
    std::fs::create_dir_all(&data).expect("data dir");
    let settings =
        engine::PluginSettings::new(home.join("replay-format-settings.json"), Vec::new());
    let plugin = Arc::new(runtime.load(&record, &wasm, data, settings).expect("load"));
    let contributions = engine::Contributions::connect(&plugin, &manifest, false).await;
    (engine, manifest, contributions)
}

async fn rig(home: &std::path::Path) -> engine::Engine {
    let (engine, manifest, contributions) = load(home).await;
    contributions.register(&engine, &manifest.id);
    engine
}

fn archive(dir: &std::path::Path, flag: &str) -> std::path::PathBuf {
    let path = dir.join("pack.zip");
    let options = zip::write::SimpleFileOptions::default();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("create the archive"));
    let manifest = format!("name=Replayed\nversion=1.21\nmemory=3G\nflag={flag}\n");
    for (name, body) in [
        ("replay.cfg", manifest.as_str()),
        ("files/options.txt", "fov:70"),
        ("junk/skip.txt", "not listed"),
    ] {
        zip.start_file(name, options).expect("start a member");
        zip.write_all(body.as_bytes()).expect("write a member");
    }
    zip.finish().expect("finish the archive");
    path
}

async fn import(
    engine: &engine::Engine,
    path: &std::path::Path,
) -> anyhow::Result<engine::ImportOutcome> {
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

#[tokio::test]
async fn an_instance_imports_through_a_format_a_plugin_contributes() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path()).await;
    let path = archive(drops.path(), "");

    let info = engine
        .inspect_archive(&path.to_string_lossy())
        .await
        .expect("inspect");
    assert_eq!(
        (info.name.as_str(), info.game_version.as_str()),
        ("Replayed", "1.21")
    );
    assert!(matches!(info.format, ImportFormat::Provider(ref p) if p.id == "replay-format.replay"));

    let outcome = import(&engine, &path).await.expect("the instance imports");

    let data = engine.instances().data_dir(&outcome.record);
    assert_eq!(
        std::fs::read_to_string(data.join("options.txt")).unwrap(),
        "fov:70"
    );
    assert!(
        !data.join("skip.txt").exists(),
        "an unlisted member is not copied"
    );
    assert_eq!(outcome.record.jvm.memory.as_deref(), Some("3G"));
}

#[tokio::test]
async fn an_answer_with_an_escaping_or_unlisted_path_is_refused_whole() {
    let (home, drops) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let engine = rig(home.path()).await;

    for flag in ["escape", "ghost"] {
        let error = import(&engine, &archive(drops.path(), flag))
            .await
            .err()
            .unwrap_or_else(|| panic!("`{flag}` must be refused"));
        assert!(error.to_string().contains("refused"), "{error}");
        assert!(
            engine.instances().list().is_empty(),
            "the half-made instance is removed"
        );
        assert!(!home.path().join("evil.txt").exists());
    }
}

#[tokio::test]
async fn a_write_answer_is_vetted_like_a_land_answer() {
    let home = tempfile::tempdir().unwrap();
    let (_engine, _, contributions) = load(home.path()).await;
    let mut formats = contributions.into_formats();
    assert_eq!(formats.len(), 1);
    let format = formats.pop().expect("connected");
    let files = vec!["options.txt".to_string()];
    let named = |name: &str| FormatBlueprint {
        name: name.into(),
        ..FormatBlueprint::default()
    };

    let written = format
        .write(&named("ok"), files.clone())
        .await
        .expect("write");
    assert_eq!(written.mapping[0].archive_path, "files/options.txt");
    assert!(format.write(&named("escape"), files).await.is_err());
}
