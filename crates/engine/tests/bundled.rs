//! Seeding the plugins an installer bundles, against the real `probe`
//! component compiled and linked by the real sandbox, signed with a test key.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use engine::{
    Approver, CallChain, HostCalls, HostError, PluginRuntime, Plugins, ProcessSupervisor,
};
use minisign::KeyPair;
use proto::plugin::{Permission, API_VERSION};
use proto::repository::build_comment;
use serde_json::Value;
use zip::write::SimpleFileOptions;

use crate::fixture::probe;

struct NoChannels;

#[async_trait]
impl HostCalls for NoChannels {
    fn permission(&self, _channel: &str) -> Option<Permission> {
        None
    }

    async fn call(
        &self,
        _plugin: &str,
        _chain: &CallChain,
        _channel: &str,
        _payload: Value,
    ) -> Result<Value, HostError> {
        Ok(Value::Null)
    }

    async fn run_job(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError> {
        self.call(plugin, chain, channel, payload).await
    }
}

fn write_bundle(dir: &Path, pair: &KeyPair, id: &str, version: &str, wasm: &[u8]) {
    let manifest = format!(
        "id = \"{id}\"\nversion = \"{version}\"\napi = \"^{API_VERSION}\"\nwasm = \"plugin.wasm\"\n"
    );
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [("plugin.toml", manifest.as_bytes()), ("plugin.wasm", wasm)] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(body).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    let file = format!("{id}-{version}.hplugin");
    std::fs::write(dir.join(&file), &bytes).unwrap();
    let signature = minisign::sign(
        Some(&pair.pk),
        &pair.sk,
        bytes.as_slice(),
        Some(&build_comment(id, version, &file)),
        None,
    )
    .unwrap();
    std::fs::write(dir.join(format!("{file}.minisig")), signature.to_string()).unwrap();
}

#[test]
fn a_fresh_data_home_gets_every_bundled_plugin_and_a_removed_one_stays_removed() {
    let home = tempfile::tempdir().unwrap();
    let bundle = home.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let official = KeyPair::generate_unencrypted_keypair().unwrap();
    let wasm = std::fs::read(probe::component()).unwrap();
    write_bundle(&bundle, &official, "alpha", "1.0.0", &wasm);
    write_bundle(&bundle, &official, "beta", "1.0.0", &wasm);

    let supervisor = Arc::new(ProcessSupervisor::new(home.path().join("processes")));
    let runtime =
        PluginRuntime::new(Arc::new(NoChannels), home.path().join("cache"), supervisor).unwrap();
    let plugins = Plugins::new(home.path().join("plugins"));
    let keys = [official.pk.to_base64()];
    let seed = || {
        plugins.seed_bundled(&bundle, &keys, |wasm| {
            runtime.check(wasm).map_err(Into::into)
        })
    };

    let first = seed();

    assert_eq!(first.installed, ["alpha", "beta"]);
    assert!(first.refused.is_empty());
    for record in plugins.list() {
        assert_eq!(record.approved_by, Approver::Official);
        assert!(record.enabled);
        assert!(plugins.read_package(&record.id).unwrap().wasm.is_some());
    }

    plugins.remove("alpha").unwrap();
    let second = seed();

    assert!(second.installed.is_empty());
    assert_eq!(
        plugins
            .list()
            .into_iter()
            .map(|record| record.id)
            .collect::<Vec<_>>(),
        ["beta"]
    );
}

#[test]
fn a_component_the_sandbox_rejects_is_refused_and_not_installed() {
    let home = tempfile::tempdir().unwrap();
    let bundle = home.path().join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let official = KeyPair::generate_unencrypted_keypair().unwrap();
    write_bundle(&bundle, &official, "alpha", "1.0.0", b"not a component");

    let supervisor = Arc::new(ProcessSupervisor::new(home.path().join("processes")));
    let runtime =
        PluginRuntime::new(Arc::new(NoChannels), home.path().join("cache"), supervisor).unwrap();
    let plugins = Plugins::new(home.path().join("plugins"));

    let seeded = plugins.seed_bundled(&bundle, &[official.pk.to_base64()], |wasm| {
        runtime.check(wasm).map_err(Into::into)
    });

    assert_eq!(seeded.refused.len(), 1);
    assert!(plugins.list().is_empty());
}

#[test]
fn a_staged_build_upgrades_the_official_plugin_it_replaces() {
    let home = tempfile::tempdir().unwrap();
    let (bundle, staged) = (home.path().join("bundle"), home.path().join("staged"));
    for dir in [&bundle, &staged] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let official = KeyPair::generate_unencrypted_keypair().unwrap();
    let wasm = std::fs::read(probe::component()).unwrap();
    write_bundle(&bundle, &official, "alpha", "1.0.0", &wasm);
    write_bundle(&staged, &official, "alpha", "1.1.0", &wasm);

    let supervisor = Arc::new(ProcessSupervisor::new(home.path().join("processes")));
    let runtime =
        PluginRuntime::new(Arc::new(NoChannels), home.path().join("cache"), supervisor).unwrap();
    let plugins = Plugins::new(home.path().join("plugins"));
    let keys = [official.pk.to_base64()];
    let seed = |dir: &Path| {
        plugins.seed_bundled(dir, &keys, |wasm| runtime.check(wasm).map_err(Into::into))
    };
    seed(&bundle);

    let upgraded = seed(&staged);

    assert_eq!(upgraded.upgraded, ["alpha"]);
    assert_eq!(plugins.read_manifest("alpha").unwrap().version, "1.1.0");
}
