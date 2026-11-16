//! Repositories end to end: a signed index served over loopback, install from it,
//! updates and what widens them, rollback, revocation, and every way an index or
//! a package is refused. The files are served locally; nothing reaches a network.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kernel::{Limits, Permission, PluginError, Plugins, Sandbox, Services, Table, TrustError};
use proto::plugin::{PluginSource, PluginState, ScopeKind, Tier};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[allow(dead_code)]
mod fixture;
use fixture::{guest, scratch, wasm};

struct Files(PathBuf);

impl Services for Files {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(kernel::sandbox_root(&self.0).join(plugin))
    }

    fn state_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.0.join("plugins").join(plugin))
    }
}

fn gated() -> Table {
    Table::new(
        vec![
            ("hestia:kernel/types", None),
            ("hestia:kernel/log", Some("log:sink")),
            ("hestia:kernel/app", Some("app:read")),
            ("hestia:kernel/lifecycle", None),
        ],
        vec![
            Permission {
                name: "app:read",
                tier: Tier::Unrestricted,
                scope: None,
                prompt: "Read the app info",
            },
            Permission {
                name: "log:sink",
                tier: Tier::Sandboxed,
                scope: Some(ScopeKind::Hosts),
                prompt: "Send its log lines",
            },
        ],
    )
}

fn plugins(home: &Path) -> Plugins {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        Limits::default(),
    )
    .expect("sandbox")
    .with_services(Arc::new(Files(home.to_owned())));
    Plugins::new(home, Vec::new(), Some(Arc::new(sandbox)), gated()).unwrap()
}

type Site = Arc<Mutex<HashMap<String, Vec<u8>>>>;

async fn serve() -> (String, Site) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let site = Site::default();
    let files = site.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let files = files.clone();
            tokio::spawn(async move {
                let mut request = vec![0u8; 4096];
                let read = stream.read(&mut request).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let body = files.lock().unwrap().get(&path).cloned();
                let head = match &body {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_owned()
                    }
                };
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body.unwrap_or_default()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (base, site)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn manifest(id: &str, version: &str, hosts: &[&str]) -> String {
    let mut text = format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\n");
    if !hosts.is_empty() {
        let hosts: Vec<String> = hosts.iter().map(|h| format!("\"{h}\"")).collect();
        text += &format!(
            "[permissions.\"log:sink\"]\nhosts = [{}]\n",
            hosts.join(", ")
        );
    }
    text
}

/// A repository under one key, publishing signed indexes to a local site.
struct Publisher {
    name: String,
    keys: minisign::KeyPair,
    base: String,
    site: Site,
    releases: Vec<Value>,
    revoked: Vec<Value>,
}

impl Publisher {
    async fn new(name: &str) -> Self {
        let (base, site) = serve().await;
        Publisher {
            name: name.to_owned(),
            keys: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
            base,
            site,
            releases: Vec::new(),
            revoked: Vec::new(),
        }
    }

    fn key(&self) -> String {
        self.keys.pk.to_base64()
    }

    fn url(&self) -> String {
        format!("{}/index.json", self.base)
    }

    fn put(&self, path: &str, body: Vec<u8>) {
        self.site.lock().unwrap().insert(path.to_owned(), body);
    }

    fn release(&mut self, id: &str, version: &str, manifest: &str, component: &str) {
        self.release_with(id, version, manifest, component, None);
    }

    /// `claimed` replaces the manifest digest the index lists.
    fn release_with(
        &mut self,
        id: &str,
        version: &str,
        manifest: &str,
        component: &str,
        claimed: Option<String>,
    ) {
        let component = wasm(guest(component));
        self.put(
            &format!("/{id}-{version}.toml"),
            manifest.as_bytes().to_vec(),
        );
        self.put(&format!("/{id}-{version}.wasm"), component.clone());
        self.releases
            .retain(|r| (&r["id"], &r["version"]) != (&json!(id), &json!(version)));
        self.releases.push(json!({
            "id": id,
            "version": version,
            "manifest": {
                "url": format!("{}/{id}-{version}.toml", self.base),
                "sha256": claimed.unwrap_or_else(|| hex(manifest.as_bytes())),
            },
            "component": {
                "url": format!("{}/{id}-{version}.wasm", self.base),
                "sha256": hex(&component),
            },
        }));
    }

    fn drop_release(&mut self, id: &str, version: &str) {
        self.releases
            .retain(|r| (&r["id"], &r["version"]) != (&json!(id), &json!(version)));
    }

    fn publish(&self, sequence: u64, expires: u64) {
        self.publish_as(&self.name, sequence, expires, &self.keys);
    }

    fn publish_as(&self, repository: &str, sequence: u64, expires: u64, keys: &minisign::KeyPair) {
        let comment = format!("hestia-index {repository} {sequence} {expires}");
        self.publish_with(repository, sequence, expires, keys, &comment);
    }

    fn publish_with(
        &self,
        repository: &str,
        sequence: u64,
        expires: u64,
        keys: &minisign::KeyPair,
        comment: &str,
    ) {
        let body = json!({
            "repository": repository,
            "sequence": sequence,
            "expires": expires,
            "plugins": self.releases,
            "revoked": self.revoked,
        })
        .to_string()
        .into_bytes();
        let signature = minisign::sign(
            Some(&keys.pk),
            &keys.sk,
            Cursor::new(&body),
            Some(comment),
            None,
        )
        .unwrap();
        self.put("/index.json.minisig", signature.to_string().into_bytes());
        self.put("/index.json", body);
    }
}

fn trust(result: Result<impl std::fmt::Debug, PluginError>) -> TrustError {
    match result {
        Err(PluginError::Trust(reason)) => reason,
        other => panic!("expected a trust refusal, got {other:?}"),
    }
}

fn state(plugins: &Plugins, id: &str) -> (PluginState, String) {
    let found = plugins.list().into_iter().find(|p| p.id == id).unwrap();
    (found.state, found.version)
}

fn installed_version(home: &Path, id: &str) -> String {
    let text = std::fs::read_to_string(home.join("plugins").join(id).join("plugin.toml")).unwrap();
    text.lines()
        .find_map(|line| line.strip_prefix("version = "))
        .unwrap()
        .trim_matches('"')
        .to_owned()
}

#[tokio::test]
async fn a_plugin_installs_from_a_signed_index_and_updates_only_as_far_as_it_was_approved() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let mut repo = Publisher::new("acme").await;
    repo.release(
        "rel",
        "1.0.0",
        &manifest("rel", "1.0.0", &["a.example"]),
        "hello",
    );
    repo.publish(1, now() + 3600);
    let added = registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    assert_eq!(registry.repositories().len(), 1);
    assert_eq!(added.id, "acme");

    let inspection = registry.inspect_release("acme", "rel", None).await.unwrap();
    let installed = registry
        .install_release("acme", "rel", None, &inspection.approval_digest)
        .await
        .unwrap();
    assert_eq!(
        (installed.state, installed.source),
        (PluginState::Enabled, PluginSource::Repository)
    );

    repo.release(
        "rel",
        "1.1.0",
        &manifest("rel", "1.1.0", &["a.example"]),
        "hello",
    );
    repo.publish(2, now() + 3600);
    let found = registry.check_updates().await;
    assert!(found.failed.is_empty());
    assert_eq!(
        (
            found.updates[0].installed.as_str(),
            found.updates[0].available.as_str()
        ),
        ("1.0.0", "1.1.0")
    );
    let same = registry.inspect_update("rel").await.unwrap();
    assert!(!same.widened, "the same grant needs no new approval");
    registry
        .update("rel", &same.inspection.approval_digest)
        .await
        .unwrap();
    assert_eq!(installed_version(home.path(), "rel"), "1.1.0");

    repo.release(
        "rel",
        "1.2.0",
        &manifest("rel", "1.2.0", &["a.example", "b.example"]),
        "hello",
    );
    repo.publish(3, now() + 3600);
    let wider = registry.inspect_update("rel").await.unwrap();
    assert!(wider.widened, "a new scope entry asks again");
    assert!(matches!(
        registry.update("rel", "stale").await,
        Err(PluginError::Changed(_))
    ));
    registry
        .update("rel", &wider.inspection.approval_digest)
        .await
        .unwrap();

    repo.release("rel", "2.0.0", &manifest("rel", "2.0.0", &[]), "trap");
    repo.publish(4, now() + 3600);
    let narrower = registry.inspect_update("rel").await.unwrap();
    assert!(!narrower.widened);
    assert!(matches!(
        registry
            .update("rel", &narrower.inspection.approval_digest)
            .await,
        Err(PluginError::Failed { .. })
    ));
    assert_eq!(
        state(&registry, "rel"),
        (PluginState::Enabled, "1.2.0".into())
    );
    assert_eq!(installed_version(home.path(), "rel"), "1.2.0");
    assert!(!home.path().join("plugins/rel.previous").exists());

    repo.drop_release("rel", "2.0.0");
    repo.release(
        "rel",
        "1.3.0",
        &manifest("rel", "1.3.0", &["a.example"]),
        "hello",
    );
    repo.revoked
        .push(json!({ "id": "rel", "version": "1.2.0", "reason": "stolen key" }));
    repo.publish(5, now() + 3600);
    let found = registry.check_updates().await;
    assert_eq!(found.updates[0].available, "1.3.0");
    assert_eq!(state(&registry, "rel").0, PluginState::Failed);
    assert!(matches!(
        registry.enable("rel").await,
        Err(PluginError::Trust(TrustError::Revoked { .. }))
    ));
    let restarted = plugins(home.path());
    restarted.restore().await;
    assert_eq!(
        state(&restarted, "rel").0,
        PluginState::Failed,
        "revoked stays off"
    );

    let fixed = restarted.inspect_update("rel").await.unwrap();
    restarted
        .update("rel", &fixed.inspection.approval_digest)
        .await
        .unwrap();
    assert_eq!(
        state(&restarted, "rel"),
        (PluginState::Enabled, "1.3.0".into())
    );

    let (live, previous) = (
        home.path().join("plugins/rel"),
        home.path().join("plugins/rel.previous"),
    );
    std::fs::create_dir_all(&previous).unwrap();
    for file in ["plugin.toml", "plugin.wasm", "approval.json"] {
        std::fs::copy(live.join(file), previous.join(file)).unwrap();
    }
    std::fs::write(previous.join("marker"), "").unwrap();
    let crashed = plugins(home.path());
    crashed.restore().await;
    assert!(live.join("marker").exists() && !previous.exists());
    assert_eq!(state(&crashed, "rel").0, PluginState::Enabled);
}

#[tokio::test]
async fn an_index_or_package_that_does_not_verify_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let mut repo = Publisher::new("acme").await;
    repo.release(
        "rel",
        "1.0.0",
        &manifest("rel", "1.0.0", &["a.example"]),
        "hello",
    );
    let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let (soon, later) = (now() + 3600, now() + 7200);

    type Publish<'a> = Box<dyn Fn(&Publisher) + 'a>;
    type Case<'a> = (&'a str, Publish<'a>, fn(&TrustError) -> bool);
    let cases: Vec<Case<'_>> = vec![
        (
            "signed by another key",
            Box::new(|p| p.publish_as("acme", 1, soon, &other)),
            |e| matches!(e, TrustError::BadSignature),
        ),
        (
            "the signature of another subject",
            Box::new(|p| p.publish_with("acme", 1, soon, &p.keys, "hestia-index acme 9 1")),
            |e| matches!(e, TrustError::Subject),
        ),
        (
            "the index of another repository",
            Box::new(|p| p.publish_as("other", 1, soon, &p.keys)),
            |e| matches!(e, TrustError::Subject),
        ),
        ("expired", Box::new(|p| p.publish(1, 1)), |e| {
            matches!(e, TrustError::Expired)
        }),
        (
            "tampered after signing",
            Box::new(|p| {
                p.publish(1, soon);
                let mut body = p.site.lock().unwrap().remove("/index.json").unwrap();
                let at = body.windows(5).position(|w| w == b"acme\"").unwrap();
                body[at] = b'A';
                p.put("/index.json", body);
            }),
            |e| matches!(e, TrustError::BadSignature),
        ),
    ];
    for (case, publish, expected) in cases {
        publish(&repo);
        let registry = plugins(home.path());
        let refusal = trust(
            registry
                .add_repository("acme", &repo.url(), &repo.key())
                .await,
        );
        assert!(expected(&refusal), "{case}: {refusal}");
        assert!(registry.repositories().is_empty(), "{case}");
    }

    repo.publish(5, soon);
    let key = repo.key();
    let registry = plugins(home.path())
        .with_official_keys(&[key.as_str()])
        .unwrap();
    assert!(matches!(
        trust(
            registry
                .add_repository("acme", &repo.url(), &repo.key())
                .await
        ),
        TrustError::OfficialKey
    ));

    let registry = plugins(home.path());
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    repo.publish(4, soon);
    let found = registry.check_updates().await;
    assert!(
        found.failed[0].detail.contains("older"),
        "an older index than one seen is a replay: {:?}",
        found.failed
    );
    repo.publish(5, later);
    assert!(registry.check_updates().await.failed.is_empty());

    repo.release_with(
        "liar",
        "1.0.0",
        &manifest("liar", "1.0.0", &[]),
        "hello",
        Some("0".repeat(64)),
    );
    repo.release("swap", "1.0.0", &manifest("swap", "9.9.9", &[]), "hello");
    repo.publish(6, later);
    assert!(matches!(
        trust(registry.install_release("acme", "liar", None, "x").await),
        TrustError::Digest(_)
    ));
    assert!(matches!(
        trust(registry.install_release("acme", "swap", None, "x").await),
        TrustError::Malformed(_)
    ));

    let inspection = registry.inspect_release("acme", "rel", None).await.unwrap();
    registry
        .install_release("acme", "rel", None, &inspection.approval_digest)
        .await
        .unwrap();
    registry.remove_repository("acme").await.unwrap();
    let rekeyed = Publisher {
        releases: repo.releases.clone(),
        ..Publisher::new("acme").await
    };
    rekeyed.publish(7, later);
    registry
        .add_repository("acme", &rekeyed.url(), &rekeyed.key())
        .await
        .unwrap();
    assert!(matches!(
        trust(registry.inspect_update("rel").await),
        TrustError::Signer(_)
    ));
}
