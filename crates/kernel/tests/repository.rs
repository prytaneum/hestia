//! Repositories end to end: a signed index served over loopback listing
//! author-signed `.hplugin` packages, install from it, updates and what widens
//! them, rollback, revocation, and every way an index or a package is refused.
//! The files are served locally; nothing reaches a network.

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use kernel::{Limits, PackageError, PluginError, Plugins, Sandbox, TrustError};
use proto::plugin::{Choice, PluginSource, PluginState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zip::write::SimpleFileOptions;

#[allow(dead_code)]
mod fixture;
use fixture::{guest, scratch, wasm, Files};

fn plugins(home: &Path) -> Plugins {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        Limits::default(),
    )
    .expect("sandbox")
    .with_services(Arc::new(Files(home.to_owned())));
    Plugins::new(home, Vec::new(), Some(Arc::new(sandbox)))
        .unwrap()
        .with_loopback()
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

fn manifest(id: &str, version: &str, hosts: &[&str]) -> String {
    let mut text = format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\n");
    if !hosts.is_empty() {
        let hosts: Vec<String> = hosts.iter().map(|h| format!("\"{h}\"")).collect();
        text += &format!(
            "[permissions.\"network:connect\"]\nhosts = [{}]\n",
            hosts.join(", ")
        );
    }
    text
}

fn optional_manifest(id: &str, version: &str, hosts: &[&str]) -> String {
    let hosts: Vec<String> = hosts.iter().map(|h| format!("\"{h}\"")).collect();
    format!(
        "id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\n[permissions.\"network:connect\"]\noptional = true\nhosts = [{}]\n[permissions.\"secrets\"]\noptional = true\n",
        hosts.join(", ")
    )
}

/// A plugin author with a signing key of their own.
struct Author(minisign::KeyPair);

impl Author {
    fn new() -> Self {
        Author(minisign::KeyPair::generate_unencrypted_keypair().unwrap())
    }

    fn key(&self) -> String {
        self.0.pk.to_base64()
    }

    fn sign(&self, statement: &str) -> String {
        sign(&self.0, statement.as_bytes(), "hestia-hplugin")
    }
}

fn sign(keys: &minisign::KeyPair, body: &[u8], comment: &str) -> String {
    minisign::sign(
        Some(&keys.pk),
        &keys.sk,
        Cursor::new(body),
        Some(comment),
        None,
    )
    .unwrap()
    .to_string()
}

/// What an author signs. Written out here, not shared with the kernel, so a
/// change of the statement fails this test as a change of the wire format.
fn statement(id: &str, version: &str, manifest: &[u8], wasm: &[u8]) -> String {
    format!(
        "hestia-hplugin-v1\n{id}\n{version}\n{}\n{}\n",
        hex::encode(Sha256::digest(manifest)),
        hex::encode(Sha256::digest(wasm))
    )
}

type Zip = zip::ZipWriter<Cursor<Vec<u8>>>;

fn zip_of(build: impl FnOnce(&mut Zip)) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    build(&mut zip);
    zip.finish().unwrap().into_inner()
}

fn entry(zip: &mut Zip, name: &str, bytes: &[u8]) {
    zip.start_file(name, SimpleFileOptions::default()).unwrap();
    zip.write_all(bytes).unwrap();
}

/// A well-formed `.hplugin`: the manifest, the component and the author's
/// signature over the statement of both.
fn hplugin(author: &Author, id: &str, version: &str, manifest: &str, wasm: &[u8]) -> Vec<u8> {
    let signature = author.sign(&statement(id, version, manifest.as_bytes(), wasm));
    zip_of(|zip| {
        entry(zip, "plugin.toml", manifest.as_bytes());
        entry(zip, "plugin.wasm", wasm);
        entry(zip, "plugin.minisig", signature.as_bytes());
    })
}

/// A repository under one key, publishing signed indexes of author-signed
/// packages to a local site.
struct Publisher {
    name: String,
    keys: minisign::KeyPair,
    base: String,
    site: Site,
    plugins: Vec<Value>,
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
            plugins: Vec::new(),
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

    fn release(
        &mut self,
        id: &str,
        author: &Author,
        version: &str,
        manifest: &str,
        component: &str,
    ) {
        let archive = hplugin(author, id, version, manifest, &wasm(guest(component)));
        self.list(id, &author.key(), version, archive, None);
    }

    /// Lists `archive` as `version` of `id` under `author_key`, with the digest
    /// it really has unless `claimed` says another.
    fn list(
        &mut self,
        id: &str,
        author_key: &str,
        version: &str,
        archive: Vec<u8>,
        claimed: Option<String>,
    ) {
        self.put(&format!("/{id}-{version}.hplugin"), archive.clone());
        let release = json!({
            "version": version,
            "url": format!("{}/{id}-{version}.hplugin", self.base),
            "sha256": claimed.unwrap_or_else(|| hex::encode(Sha256::digest(&archive))),
            "size": archive.len(),
        });
        let at = self.plugins.iter().position(|l| l["id"] == json!(id));
        let listing = match at {
            Some(at) => &mut self.plugins[at],
            None => {
                self.plugins
                    .push(json!({ "id": id, "author": author_key, "releases": [] }));
                self.plugins.last_mut().unwrap()
            }
        };
        listing["author"] = json!(author_key);
        let releases = listing["releases"].as_array_mut().unwrap();
        releases.retain(|r| r["version"] != json!(version));
        releases.push(release);
    }

    fn drop_release(&mut self, id: &str, version: &str) {
        for listing in self.plugins.iter_mut().filter(|l| l["id"] == json!(id)) {
            let releases = listing["releases"].as_array_mut().unwrap();
            releases.retain(|r| r["version"] != json!(version));
        }
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
            "plugins": self.plugins,
            "revoked": self.revoked,
        })
        .to_string()
        .into_bytes();
        self.put(
            "/index.json.minisig",
            sign(keys, &body, comment).into_bytes(),
        );
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

/// A component whose core module fails validation: it is read as a package
/// but cannot be compiled, so an update to it fails at the link.
fn unlinkable() -> Vec<u8> {
    let module = [
        &b"\0asm\x01\0\0\0"[..],
        &[0x01, 0x04, 0x01, 0x60, 0x00, 0x00],
        &[0x03, 0x02, 0x01, 0x00],
        &[0x0a, 0x05, 0x01, 0x03, 0x00, 0x6a, 0x0b],
    ]
    .concat();
    [
        &b"\0asm\x0d\0\x01\0"[..],
        &[0x01, module.len() as u8],
        &module,
    ]
    .concat()
}

#[tokio::test]
async fn a_plugin_installs_from_a_signed_index_and_updates_only_as_far_as_it_was_approved() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let (mut repo, author) = (Publisher::new("acme").await, Author::new());
    repo.release(
        "rel",
        &author,
        "1.0.0",
        &manifest("rel", "1.0.0", &["a.example"]),
        "net",
    );
    repo.publish(1, now() + 3600);
    let added = registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    assert_eq!(registry.repositories().len(), 1);
    assert_eq!(added.id, "acme");

    let inspection = registry.inspect_release("acme", "rel", None).await.unwrap();
    let signer = inspection.signer.clone().unwrap();
    assert_eq!(signer.repository, "acme");
    assert!(!signer.official, "an author's key is never Hestia's");
    let installed = registry
        .install_release(
            "acme",
            "rel",
            None,
            &inspection.approval_digest,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        (installed.state, installed.source),
        (PluginState::Enabled, PluginSource::Repository)
    );

    repo.release(
        "rel",
        &author,
        "1.1.0",
        &manifest("rel", "1.1.0", &["a.example"]),
        "net",
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
    assert_eq!(same.inspection.signer, Some(signer));
    let kept = ["settings.json", "secrets.json", "schedules.json"];
    for name in kept {
        std::fs::write(home.path().join("plugins/rel").join(name), name).unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let secrets = home.path().join("plugins/rel/secrets.json");
        std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let still_there = |when: &str| {
        for name in kept {
            let path = home.path().join("plugins/rel").join(name);
            assert_eq!(
                std::fs::read_to_string(&path).unwrap_or_default(),
                name,
                "{name} {when}"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let secrets = home.path().join("plugins/rel/secrets.json");
            let mode = std::fs::metadata(secrets).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "secrets stay owner-only {when}");
        }
    };
    registry
        .update("rel", &same.inspection.approval_digest, None)
        .await
        .unwrap();
    assert_eq!(installed_version(home.path(), "rel"), "1.1.0");
    still_there("after an update");

    repo.release(
        "rel",
        &author,
        "1.2.0",
        &manifest("rel", "1.2.0", &["a.example", "b.example"]),
        "net",
    );
    repo.publish(3, now() + 3600);
    let wider = registry.inspect_update("rel").await.unwrap();
    assert!(wider.widened, "a new scope entry asks again");
    assert!(matches!(
        registry.update("rel", "stale", None).await,
        Err(PluginError::Changed(_))
    ));
    registry
        .update("rel", &wider.inspection.approval_digest, None)
        .await
        .unwrap();

    let broken = hplugin(
        &author,
        "rel",
        "2.0.0",
        &manifest("rel", "2.0.0", &[]),
        &unlinkable(),
    );
    repo.list("rel", &author.key(), "2.0.0", broken, None);
    repo.publish(4, now() + 3600);
    let unloadable = registry.inspect_update("rel").await.unwrap();
    assert!(matches!(
        registry
            .update("rel", &unloadable.inspection.approval_digest, None)
            .await,
        Err(PluginError::Package(_))
    ));
    repo.drop_release("rel", "2.0.0");
    repo.release(
        "rel",
        &author,
        "2.0.1",
        &manifest("rel", "2.0.1", &[]),
        "trap",
    );
    repo.publish(5, now() + 3600);
    let narrower = registry.inspect_update("rel").await.unwrap();
    assert!(!narrower.widened);
    assert!(matches!(
        registry
            .update("rel", &narrower.inspection.approval_digest, None)
            .await,
        Err(PluginError::Failed { .. })
    ));
    assert_eq!(
        state(&registry, "rel"),
        (PluginState::Enabled, "1.2.0".into()),
        "a failed link and a failed start each roll back"
    );
    assert_eq!(installed_version(home.path(), "rel"), "1.2.0");
    assert!(!home.path().join("plugins/rel.previous").exists());
    still_there("after a rollback");

    repo.drop_release("rel", "2.0.1");
    repo.release(
        "rel",
        &author,
        "1.3.0",
        &manifest("rel", "1.3.0", &["a.example"]),
        "net",
    );
    repo.revoked
        .push(json!({ "id": "rel", "version": "1.2.0", "reason": "stolen key" }));
    repo.publish(6, now() + 3600);
    let found = registry.check_updates().await;
    assert_eq!(found.updates[0].available, "1.3.0");
    assert_eq!(
        (
            found.revoked[0].id.as_str(),
            found.revoked[0].reason.as_str()
        ),
        ("rel", "stolen key")
    );
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
        .update("rel", &fixed.inspection.approval_digest, None)
        .await
        .unwrap();
    assert_eq!(
        state(&restarted, "rel"),
        (PluginState::Enabled, "1.3.0".into())
    );
}

async fn apply(registry: &Plugins, widened: bool) {
    registry.check_updates().await;
    let found = registry.inspect_update("rel").await.unwrap();
    assert_eq!(found.widened, widened);
    registry
        .update("rel", &found.inspection.approval_digest, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn an_update_keeps_an_optional_grant_only_while_the_permission_stays_optional_and_no_wider() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let (mut repo, author) = (Publisher::new("acme").await, Author::new());
    let granted = || {
        registry
            .list()
            .into_iter()
            .find(|p| p.id == "rel")
            .unwrap()
            .optional
    };
    repo.release(
        "rel",
        &author,
        "1.0.0",
        &optional_manifest("rel", "1.0.0", &["a.example"]),
        "net",
    );
    repo.publish(1, now() + 3600);
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    let inspection = registry.inspect_release("acme", "rel", None).await.unwrap();
    let chosen = Choice {
        optional: vec!["network:connect".into()],
        ..Choice::default()
    };
    registry
        .install_release("acme", "rel", None, &inspection.approval_digest, chosen)
        .await
        .unwrap();
    assert_eq!(granted(), ["network:connect"]);

    let mut sequence = 1;
    let mut update = |version: &str, manifest: String, component: &str| {
        sequence += 1;
        repo.release("rel", &author, version, &manifest, component);
        repo.publish(sequence, now() + 3600);
    };
    update(
        "1.1.0",
        optional_manifest("rel", "1.1.0", &["a.example"]),
        "net",
    );
    apply(&registry, false).await;
    assert_eq!(
        granted(),
        ["network:connect"],
        "the same optional grant stays"
    );

    update("2.0.0", manifest("rel", "2.0.0", &[]), "nap");
    apply(&registry, false).await;
    assert!(granted().is_empty());

    update(
        "3.0.0",
        optional_manifest("rel", "3.0.0", &["evil.example"]),
        "net",
    );
    apply(&registry, false).await;
    assert!(
        granted().is_empty(),
        "a permission dropped and added back optional arrives declined, whatever its scope"
    );
    let info = registry
        .set_permission("rel", "network:connect", true)
        .await
        .unwrap();
    assert_eq!(info.optional, ["network:connect"]);

    update(
        "3.1.0",
        optional_manifest("rel", "3.1.0", &["evil.example", "b.example"]),
        "net",
    );
    apply(&registry, true).await;
    assert!(
        granted().is_empty(),
        "an optional grant whose scope widened is declined again"
    );
}

#[tokio::test]
async fn an_update_that_never_finished_is_undone_at_start_and_nothing_else_is_touched() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let (mut repo, author) = (Publisher::new("acme").await, Author::new());
    repo.release(
        "rel",
        &author,
        "1.0.0",
        &manifest("rel", "1.0.0", &["a.example"]),
        "net",
    );
    repo.publish(1, now() + 3600);
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    let inspection = registry.inspect_release("acme", "rel", None).await.unwrap();
    registry
        .install_release(
            "acme",
            "rel",
            None,
            &inspection.approval_digest,
            Default::default(),
        )
        .await
        .unwrap();

    let plugins_dir = home.path().join("plugins");
    let (live, previous) = (plugins_dir.join("rel"), plugins_dir.join("rel.previous"));
    let copy_to_previous = || {
        std::fs::create_dir_all(&previous).unwrap();
        for file in ["plugin.toml", "plugin.wasm", "approval.json"] {
            std::fs::copy(live.join(file), previous.join(file)).unwrap();
        }
        std::fs::write(previous.join("marker"), "").unwrap();
    };
    std::fs::create_dir_all(plugins_dir.join("rel.staging")).unwrap();
    std::fs::create_dir_all(plugins_dir.join(".previous")).unwrap();
    std::fs::write(plugins_dir.join(".previous/keep"), "").unwrap();

    copy_to_previous();
    let crashed = plugins(home.path());
    crashed.restore().await;
    assert!(live.join("marker").exists() && !previous.exists());
    assert_eq!(state(&crashed, "rel").0, PluginState::Enabled);
    assert!(!plugins_dir.join("rel.staging").exists());
    assert!(
        plugins_dir.join(".previous/keep").exists(),
        "a directory that is no plugin's is left alone"
    );

    copy_to_previous();
    std::fs::remove_dir_all(&live).unwrap();
    let only_previous = plugins(home.path());
    only_previous.restore().await;
    assert_eq!(
        state(&only_previous, "rel"),
        (PluginState::Enabled, "1.0.0".into()),
        "a crash that left only the old version still lists it"
    );
    assert!(live.join("marker").exists());

    copy_to_previous();
    only_previous.remove("rel").await.unwrap();
    assert!(!live.exists() && !previous.exists());
    let after_removal = plugins(home.path());
    after_removal.restore().await;
    assert!(
        after_removal.list().iter().all(|p| p.id != "rel"),
        "a removed plugin does not come back from a leftover previous version"
    );
}

#[tokio::test]
async fn an_index_that_does_not_verify_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let (mut repo, author) = (Publisher::new("acme").await, Author::new());
    repo.release(
        "rel",
        &author,
        "1.0.0",
        &manifest("rel", "1.0.0", &[]),
        "net",
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
            "a life of more than 30 days",
            Box::new(|p| p.publish(1, now() + 31 * 24 * 3600)),
            |e| matches!(e, TrustError::Lifetime(_)),
        ),
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
    assert!(matches!(
        registry
            .add_repository("Bad Id", &repo.url(), &repo.key())
            .await,
        Err(PluginError::InvalidId(_))
    ));
}

#[tokio::test]
async fn a_package_is_trusted_only_as_its_author_signed_it_and_the_index_listed_it() {
    let home = tempfile::tempdir().unwrap();
    let (mut repo, author, stranger) = (Publisher::new("acme").await, Author::new(), Author::new());
    let (soon, net) = (now() + 3600, wasm(guest("net")));
    let signed =
        |id: &str, version: &str| hplugin(&author, id, version, &manifest(id, version, &[]), &net);

    repo.list(
        "liar",
        &author.key(),
        "1.0.0",
        signed("liar", "1.0.0"),
        Some("0".repeat(64)),
    );
    repo.list(
        "stranger",
        &author.key(),
        "1.0.0",
        hplugin(
            &stranger,
            "stranger",
            "1.0.0",
            &manifest("stranger", "1.0.0", &[]),
            &net,
        ),
        None,
    );
    repo.list(
        "swap",
        &author.key(),
        "1.0.0",
        signed("swap", "9.9.9"),
        None,
    );
    repo.list(
        "moved",
        &author.key(),
        "2.0.0",
        signed("moved", "1.0.0"),
        None,
    );
    let tampered = {
        let toml = manifest("tampered", "1.0.0", &[]);
        let signature = author.sign(&statement("tampered", "1.0.0", toml.as_bytes(), &net));
        zip_of(|zip| {
            entry(zip, "plugin.toml", toml.as_bytes());
            entry(zip, "plugin.wasm", &wasm(guest("chatty")));
            entry(zip, "plugin.minisig", signature.as_bytes());
        })
    };
    repo.list("tampered", &author.key(), "1.0.0", tampered, None);

    let toml = manifest("shape", "1.0.0", &[]);
    let signature = author.sign(&statement("shape", "1.0.0", toml.as_bytes(), &net));
    let well_formed = |zip: &mut Zip| {
        entry(zip, "plugin.toml", toml.as_bytes());
        entry(zip, "plugin.wasm", &net);
        entry(zip, "plugin.minisig", signature.as_bytes());
    };
    let shapes: Vec<(&str, Vec<u8>)> = vec![
        ("not a zip", b"PK nothing".to_vec()),
        (
            "an extra entry",
            zip_of(|z| {
                well_formed(z);
                entry(z, "extra", b"x")
            }),
        ),
        (
            "a missing signature",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                entry(z, "plugin.wasm", &net);
                entry(z, "other", b"x")
            }),
        ),
        (
            "a path with a directory",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                entry(z, "plugin.wasm", &net);
                entry(z, "dir/plugin.minisig", signature.as_bytes())
            }),
        ),
        (
            "a path that climbs out",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                entry(z, "plugin.wasm", &net);
                entry(z, "../plugin.minisig", signature.as_bytes())
            }),
        ),
        (
            "a backslash path",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                entry(z, "plugin.wasm", &net);
                entry(z, "a\\plugin.minisig", signature.as_bytes())
            }),
        ),
        (
            "a directory entry",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                entry(z, "plugin.wasm", &net);
                z.add_directory("plugin.minisig/", SimpleFileOptions::default())
                    .unwrap()
            }),
        ),
        (
            "a symlink",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                entry(z, "plugin.wasm", &net);
                z.add_symlink(
                    "plugin.minisig",
                    "/etc/passwd",
                    SimpleFileOptions::default(),
                )
                .unwrap()
            }),
        ),
        (
            "a component past its cap",
            zip_of(|z| {
                entry(z, "plugin.toml", toml.as_bytes());
                z.start_file(
                    "plugin.wasm",
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
                z.write_all(&vec![0u8; (64 << 20) + 1]).unwrap();
                entry(z, "plugin.minisig", signature.as_bytes());
            }),
        ),
    ];
    for (case, archive) in &shapes {
        repo.list(
            &format!("shape-{}", case.len()),
            &author.key(),
            "1.0.0",
            archive.clone(),
            None,
        );
    }
    repo.publish(1, soon);

    let registry = plugins(home.path());
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    let refused = |id: &str| {
        let registry = &registry;
        let id = id.to_owned();
        async move { trust(registry.inspect_release("acme", &id, None).await) }
    };
    assert!(matches!(refused("liar").await, TrustError::Digest(_)));
    assert!(matches!(
        refused("stranger").await,
        TrustError::PackageSignature
    ));
    assert!(
        matches!(refused("tampered").await, TrustError::PackageSignature),
        "a component swapped after signing"
    );
    assert!(matches!(
        refused("swap").await,
        TrustError::PackageSignature
    ));
    assert!(
        matches!(refused("moved").await, TrustError::PackageSignature),
        "a signature of one version does not carry to another"
    );
    for (case, _) in &shapes {
        let refusal = refused(&format!("shape-{}", case.len())).await;
        assert!(
            matches!(refusal, TrustError::Archive(_)),
            "{case}: {refusal}"
        );
    }
}

#[tokio::test]
async fn an_update_keeps_the_repository_and_the_author_that_vouched_for_the_install() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let (mut repo, author, successor) =
        (Publisher::new("acme").await, Author::new(), Author::new());
    let soon = now() + 3600;
    repo.release(
        "rel",
        &author,
        "1.0.0",
        &manifest("rel", "1.0.0", &["a.example"]),
        "net",
    );
    repo.publish(1, soon);
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    let inspection = registry.inspect_release("acme", "rel", None).await.unwrap();
    registry
        .install_release(
            "acme",
            "rel",
            None,
            &inspection.approval_digest,
            Default::default(),
        )
        .await
        .unwrap();

    repo.release(
        "rel",
        &successor,
        "1.1.0",
        &manifest("rel", "1.1.0", &["a.example"]),
        "net",
    );
    repo.publish(2, soon);
    assert!(
        matches!(
            trust(registry.inspect_update("rel").await),
            TrustError::AuthorKey(_)
        ),
        "an index that now names another author does not move the plugin to them"
    );

    registry.remove_repository("acme").await.unwrap();
    let rekeyed = Publisher {
        plugins: repo.plugins.clone(),
        ..Publisher::new("acme").await
    };
    rekeyed.publish(1, soon);
    registry
        .add_repository("acme", &rekeyed.url(), &rekeyed.key())
        .await
        .unwrap();
    assert!(matches!(
        trust(registry.inspect_update("rel").await),
        TrustError::RepositoryKey(_)
    ));
    let found = registry.check_updates().await;
    assert!(
        found.updates.is_empty() && found.revoked.is_empty(),
        "a repository under another key neither updates nor revokes what the first installed"
    );
}

/// A key naming the same ed25519 key under another minisign key id.
fn other_key_id(key: &str) -> String {
    format!("{}AAAA{}", &key[..4], &key[8..])
}

#[tokio::test]
async fn a_key_that_stands_for_hestia_is_never_a_repository_key_and_an_index_cannot_make_one() {
    let home = tempfile::tempdir().unwrap();
    let (mut repo, hestia, community) =
        (Publisher::new("acme").await, Author::new(), Author::new());
    repo.release(
        "rel",
        &hestia,
        "1.0.0",
        &manifest("rel", "1.0.0", &["a.example"]),
        "net",
    );
    repo.release(
        "mine",
        &community,
        "1.0.0",
        &manifest("mine", "1.0.0", &["a.example"]),
        "net",
    );
    repo.publish(1, now() + 3600);
    let official = hestia.key();
    let registry = plugins(home.path())
        .with_official_keys(&[official.as_str(), repo.key().as_str()])
        .unwrap();
    for key in [repo.key(), other_key_id(&repo.key())] {
        assert!(
            matches!(
                trust(registry.add_repository("acme", &repo.url(), &key).await),
                TrustError::OfficialKey
            ),
            "{key}"
        );
    }

    let registry = plugins(home.path())
        .with_official_keys(&[official.as_str()])
        .unwrap();
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();
    let signer = |id: &'static str| {
        let registry = &registry;
        async move {
            registry
                .inspect_release("acme", id, None)
                .await
                .unwrap()
                .signer
                .unwrap()
        }
    };
    assert!(signer("rel").await.official, "Hestia's key signed it");
    assert!(
        !signer("mine").await.official,
        "an index lists keys but never decides which are Hestia's"
    );
}

fn needing(id: &str, version: &str, needs: &[(&str, &str)]) -> String {
    let mut text = format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\n");
    if !needs.is_empty() {
        text += "[dependencies]\n";
        for (dependency, range) in needs {
            text += &format!("{dependency} = \"{range}\"\n");
        }
    }
    text
}

fn release(repo: &mut Publisher, author: &Author, id: &str, version: &str, needs: &[(&str, &str)]) {
    repo.release(id, author, version, &needing(id, version, needs), "hello");
}

#[tokio::test]
async fn an_install_brings_what_the_plugin_needs_under_one_approval() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let (mut repo, author) = (Publisher::new("acme").await, Author::new());
    release(&mut repo, &author, "lib", "1.0.0", &[]);
    release(&mut repo, &author, "tool", "1.0.0", &[("lib", "^1.0")]);
    release(&mut repo, &author, "orphan", "1.0.0", &[("ghost", "^1.0")]);
    release(&mut repo, &author, "ping", "1.0.0", &[("pong", "^1.0")]);
    release(&mut repo, &author, "pong", "1.0.0", &[("ping", "^1.0")]);
    repo.publish(1, now() + 3600);
    registry
        .add_repository("acme", &repo.url(), &repo.key())
        .await
        .unwrap();

    assert!(matches!(
        registry.inspect_release("acme", "orphan", None).await,
        Err(PluginError::MissingDependency { dependency, .. }) if dependency == "ghost"
    ));
    assert!(matches!(
        registry.inspect_release("acme", "ping", None).await,
        Err(PluginError::Package(PackageError::DependencyCycle(_)))
    ));

    let inspection = registry
        .inspect_release("acme", "tool", None)
        .await
        .unwrap();
    let planned: Vec<_> = inspection.plan.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(planned, ["lib"]);
    assert_eq!(
        inspection.plan[0].signer.as_ref().unwrap().repository,
        "acme"
    );

    release(&mut repo, &author, "lib", "1.1.0", &[]);
    repo.publish(2, now() + 3600);
    assert!(
        matches!(
            registry
                .install_release(
                    "acme",
                    "tool",
                    None,
                    &inspection.approval_digest,
                    Default::default(),
                )
                .await,
            Err(PluginError::Changed(_))
        ),
        "a plan that moved since the prompt is not installed"
    );
    assert!(registry.list().is_empty(), "nothing was installed");

    let inspection = registry
        .inspect_release("acme", "tool", None)
        .await
        .unwrap();
    let installed = registry
        .install_release(
            "acme",
            "tool",
            None,
            &inspection.approval_digest,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(installed.state, PluginState::Enabled);
    assert_eq!(
        state(&registry, "lib"),
        (PluginState::Enabled, "1.1.0".into())
    );
    assert!(matches!(
        registry.remove("lib").await,
        Err(PluginError::Required { dependents, .. }) if dependents == ["tool"]
    ));

    release(
        &mut repo,
        &author,
        "tool",
        "1.1.0",
        &[("lib", "^1.0"), ("extra", "^1.0")],
    );
    release(&mut repo, &author, "extra", "1.0.0", &[]);
    repo.publish(3, now() + 3600);
    let update = registry.inspect_update("tool").await.unwrap();
    let planned: Vec<_> = update
        .inspection
        .plan
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!((update.widened, planned), (true, vec!["extra"]));
    assert!(matches!(
        registry.update("tool", "stale", None).await,
        Err(PluginError::Changed(_))
    ));
    registry
        .update("tool", &update.inspection.approval_digest, None)
        .await
        .unwrap();
    assert_eq!(
        state(&registry, "tool"),
        (PluginState::Enabled, "1.1.0".into())
    );
    assert_eq!(
        state(&registry, "extra"),
        (PluginState::Enabled, "1.0.0".into())
    );

    release(&mut repo, &author, "extra", "1.1.0", &[("tool", "^1.0")]);
    repo.publish(4, now() + 3600);
    assert!(
        matches!(
            registry.inspect_update("extra").await,
            Err(PluginError::Package(PackageError::DependencyCycle(_)))
        ),
        "an update that loops back through an installed plugin is refused"
    );

    release(&mut repo, &author, "lib", "2.0.0", &[]);
    repo.publish(5, now() + 3600);
    assert!(
        matches!(
            registry.inspect_update("lib").await,
            Err(PluginError::Rejected { dependents, .. }) if dependents == ["tool"]
        ),
        "an update that leaves a dependent's range is refused at inspect"
    );

    release(&mut repo, &author, "first", "1.0.0", &[]);
    repo.release(
        "zbad",
        &author,
        "1.0.0",
        &needing("zbad", "1.0.0", &[]),
        "trap",
    );
    release(
        &mut repo,
        &author,
        "combo",
        "1.0.0",
        &[("first", "^1.0"), ("zbad", "^1.0")],
    );
    repo.publish(6, now() + 3600);
    let sandboxed = kernel::sandbox_root(home.path()).join("first");
    std::fs::create_dir_all(&sandboxed).unwrap();
    std::fs::write(sandboxed.join("written"), "").unwrap();
    let inspection = registry
        .inspect_release("acme", "combo", None)
        .await
        .unwrap();
    assert!(matches!(
        registry
            .install_release(
                "acme",
                "combo",
                None,
                &inspection.approval_digest,
                Default::default(),
            )
            .await,
        Err(PluginError::Failed { plugin, .. }) if plugin == "zbad"
    ));
    let listed = |id: &str| registry.list().iter().any(|p| p.id == id);
    assert!(
        !["first", "zbad", "combo"].into_iter().any(listed),
        "a plan whose member does not start leaves nothing installed"
    );
    assert!(!sandboxed.exists(), "an undone member's data goes with it");
    let pending = home.path().join("plugins/pending.json");
    assert!(!pending.exists());

    let inspection = registry
        .inspect_release("acme", "first", None)
        .await
        .unwrap();
    registry
        .install_release(
            "acme",
            "first",
            None,
            &inspection.approval_digest,
            Default::default(),
        )
        .await
        .unwrap();
    std::fs::create_dir_all(&sandboxed).unwrap();
    let stored = |id: &str| -> String {
        let text =
            std::fs::read_to_string(home.path().join("plugins").join(id).join("approval.json"));
        serde_json::from_str::<Value>(&text.unwrap()).unwrap()["digest"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let record = |digest: String| {
        std::fs::write(
            &pending,
            json!({ "plugin": "tool", "digest": digest, "members": ["first"] }).to_string(),
        )
        .unwrap();
    };
    record(stored("tool"));
    plugins(home.path()).restore().await;
    assert!(
        sandboxed.exists() && !pending.exists(),
        "a plan whose plugin is stored under its digest finished: only the record goes"
    );
    assert!(home.path().join("plugins/first").exists());
    record("stale".into());
    let restarted = plugins(home.path());
    restarted.restore().await;
    assert!(
        !restarted.list().iter().any(|p| p.id == "first")
            && !sandboxed.exists()
            && !pending.exists(),
        "an install that never finished is undone before anything starts"
    );
    assert_eq!(
        state(&restarted, "tool"),
        (PluginState::Enabled, "1.1.0".into())
    );
}
