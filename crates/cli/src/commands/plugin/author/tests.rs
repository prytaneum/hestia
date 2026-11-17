//! The author's flow end to end, against hand-assembled components: no build,
//! and the kernel's own verifiers decide whether what was written is accepted.

use std::path::Path;

use kernel::{Hplugin, Index, Manifest, Shape, Stability, Table};

use super::inspect::review;
use super::lock::{quote, Lock};
use super::pack::package;
use super::project::{Source, LOCK};
use super::publish::{publish, PublishArgs};

const NOW: u64 = 1_800_000_000;

fn leb(length: usize) -> u8 {
    u8::try_from(length).expect("a short name")
}

/// A component whose top level imports and exports these interfaces: all the
/// shape a grant is derived from.
fn component(imports: &[&str], exports: &[&str]) -> Vec<u8> {
    let name = |name: &str| [&[0, leb(name.len())][..], name.as_bytes()].concat();
    let section = |id: u8, body: Vec<u8>| [&[id, leb(body.len())][..], &body].concat();
    let mut wasm = vec![0, 0x61, 0x73, 0x6d, 0x0d, 0, 1, 0];
    wasm.extend(section(7, vec![1, 0x42, 0]));
    let imported: Vec<u8> = imports
        .iter()
        .flat_map(|import| [name(import), vec![0x05, 0]].concat())
        .collect();
    wasm.extend(section(10, [&[leb(imports.len())][..], &imported].concat()));
    let exported: Vec<u8> = exports
        .iter()
        .flat_map(|export| [name(export), vec![0x05, 0, 0]].concat())
        .collect();
    wasm.extend(section(11, [&[leb(exports.len())][..], &exported].concat()));
    wasm
}

fn manifest(version: &str) -> Vec<u8> {
    format!("id = \"plug\"\nname = \"Plug\"\nversion = \"{version}\"\n").into_bytes()
}

const LIFECYCLE: &str = "hestia:kernel/lifecycle@0.1.0";
const SECRETS: &str = "hestia:kernel/secrets@0.1.0";

fn source(dir: &Path, version: &str, imports: &[&str]) -> Source {
    Source {
        dir: dir.to_owned(),
        manifest: manifest(version),
        wasm: component(imports, &[LIFECYCLE]),
    }
}

fn keys() -> minisign::KeyPair {
    minisign::KeyPair::generate_unencrypted_keypair().expect("a key pair")
}

fn key_file(dir: &Path, name: &str, pair: &minisign::KeyPair) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, pair.sk.to_box(None).expect("a key box").to_string()).expect("write");
    path
}

#[test]
fn check_holds_the_build_to_the_lockfile_and_shows_what_differs() {
    let home = tempfile::tempdir().expect("a directory");
    let table = kernel::table();
    let bare = source(home.path(), "1.0.0", &[]);

    let missing = review(&table, &bare, false, false);
    assert!(missing.errors[0].contains(LOCK), "{:?}", missing.errors);

    assert!(review(&table, &bare, false, true).errors.is_empty());
    assert!(review(&table, &bare, false, false).errors.is_empty());

    let grown = source(home.path(), "1.0.0", &[SECRETS]);
    let differs = review(&table, &grown, false, false);
    let diff = differs.errors.join("\n");
    assert!(diff.contains("+ \"secrets\" = {}"), "{diff}");

    let broken = Source {
        manifest: b"id = 3".to_vec(),
        wasm: b"not a component".to_vec(),
        ..source(home.path(), "1.0.0", &[])
    };
    assert_eq!(review(&table, &broken, false, false).errors.len(), 2);
}

#[test]
fn a_package_is_signed_for_the_kernel_and_refuses_unstable_imports() {
    let home = tempfile::tempdir().expect("a directory");
    let author = keys();
    let project = source(home.path(), "1.0.0", &[]);
    review(&kernel::table(), &project, false, true);
    let secret = super::key::load(&key_file(home.path(), "author.key", &author)).expect("key");

    let packed = package(&kernel::table(), &project, &secret).expect("a package");
    let archive = Hplugin::read(&packed.archive).expect("three entries");
    let signer = kernel::Signer::parse(&author.pk.to_base64()).expect("a key");
    archive.verify(&signer, "plug", "1.0.0").expect("signed");
    assert!(String::from_utf8_lossy(&archive.manifest).contains("api = \"0.1.0\""));

    let preview = Table::new(
        vec![
            ("hestia:kernel/lifecycle", None),
            ("hestia:kernel/preview", None),
        ],
        vec![],
    )
    .versioned("hestia:kernel", "0.1.0")
    .stability(
        "hestia:kernel/preview",
        Stability::Unstable { feature: "preview" },
    );
    let unstable = source(home.path(), "1.0.0", &["hestia:kernel/preview@0.1.0"]);
    review(&preview, &unstable, true, true);
    let strict = review(&preview, &unstable, false, false);
    assert!(strict.errors[0].contains("unstable"), "{:?}", strict.errors);
    package(&preview, &unstable, &secret).expect_err("packing refuses it");
}

/// A repository with one author and one plugin, and what publishing needs.
struct Hub {
    home: tempfile::TempDir,
    repository: tempfile::TempDir,
    author: minisign::KeyPair,
    hub: minisign::KeyPair,
}

impl Hub {
    fn new() -> Self {
        Hub {
            home: tempfile::tempdir().expect("a directory"),
            repository: tempfile::tempdir().expect("a directory"),
            author: keys(),
            hub: keys(),
        }
    }

    fn pack(&self, version: &str, imports: &[&str]) -> std::path::PathBuf {
        let secret =
            super::key::load(&key_file(self.home.path(), "author.key", &self.author)).expect("key");
        let project = source(self.home.path(), version, imports);
        review(&kernel::table(), &project, false, true);
        let packed = package(&kernel::table(), &project, &secret).expect("a package");
        let file = self.home.path().join(&packed.file_name);
        std::fs::write(&file, packed.archive).expect("write");
        file
    }

    fn args(&self, package: Option<std::path::PathBuf>, version: &str) -> PublishArgs {
        PublishArgs {
            url: package
                .as_ref()
                .map(|_| format!("https://example.com/plugins/plug-{version}.hplugin")),
            package,
            repository: self.repository.path().to_owned(),
            key: key_file(self.home.path(), "hub.key", &self.hub),
            author: Some(self.author.pk.to_base64()),
            name: Some("hub".to_owned()),
            host: vec!["example.com".to_owned()],
            allow_widen: false,
            project: self.home.path().to_owned(),
            refresh: false,
            revoke: None,
            reason: None,
        }
    }

    fn accepted(&self, floor: u64) -> Index {
        let body = std::fs::read(self.repository.path().join("index.json")).expect("an index");
        let signature = std::fs::read_to_string(self.repository.path().join("index.json.minisig"))
            .expect("a signature");
        let signer = kernel::Signer::parse(&self.hub.pk.to_base64()).expect("a key");
        Index::verify(&body, &signature, &signer, "hub", floor, NOW).expect("the kernel accepts it")
    }
}

#[test]
fn publish_writes_an_index_the_kernel_accepts_and_stops_a_widening() {
    let hub = Hub::new();
    let first = hub.pack("1.0.0", &[]);
    publish(&hub.args(Some(first), "1.0.0"), NOW).expect("the first release");
    assert_eq!(hub.accepted(1).plugins[0].releases.len(), 1);

    let wider = hub.pack("1.1.0", &[SECRETS]);
    let refused = publish(&hub.args(Some(wider.clone()), "1.1.0"), NOW).expect_err("a widening");
    assert!(
        format!("{refused:#}").contains("--allow-widen"),
        "{refused:#}"
    );
    assert_eq!(hub.accepted(1).sequence, 1);

    let allowed = PublishArgs {
        allow_widen: true,
        ..hub.args(Some(wider), "1.1.0")
    };
    publish(&allowed, NOW).expect("allowed to widen");
    let index = hub.accepted(2);
    assert_eq!(index.plugins[0].releases.len(), 2);
    assert!(hub.repository.path().join("plug-1.1.0.hplugin").is_file());
}

#[test]
fn publish_refuses_what_the_repository_or_the_package_cannot_vouch_for() {
    let hub = Hub::new();
    let first = hub.pack("1.0.0", &[]);
    publish(&hub.args(Some(first.clone()), "1.0.0"), NOW).expect("the first release");
    let second = hub.pack("1.1.0", &[]);
    let refuse = |args: PublishArgs, expected: &str| {
        let error = format!("{:#}", publish(&args, NOW).expect_err(expected));
        assert!(error.contains(expected), "{expected}: {error}");
    };

    refuse(
        PublishArgs {
            author: Some(keys().pk.to_base64()),
            ..hub.args(Some(second.clone()), "1.1.0")
        },
        "another author key",
    );
    refuse(hub.args(Some(first), "1.0.0"), "already published");
    refuse(
        PublishArgs {
            host: Vec::new(),
            url: Some("https://elsewhere.example/plug-1.1.0.hplugin".to_owned()),
            ..hub.args(Some(second.clone()), "1.1.0")
        },
        "--host elsewhere.example",
    );
    for (url, expected) in [
        ("https://example.com/index.json", "file name"),
        ("https://example.com/p.hplugin?x=1", "query"),
        ("https://example.com", "file name"),
    ] {
        refuse(
            PublishArgs {
                url: Some(url.to_owned()),
                ..hub.args(Some(second.clone()), "1.1.0")
            },
            expected,
        );
    }
    let mut named = hub.args(None, "");
    named.name = Some("Not Valid".to_owned());
    named.refresh = true;
    std::fs::remove_file(hub.repository.path().join("index.json")).expect("remove");
    refuse(named, "not a valid repository name");

    let fresh = Hub::new();
    publish(&fresh.args(Some(fresh.pack("1.0.0", &[])), "1.0.0"), NOW).expect("a release");
    let index = fresh.repository.path().join("index.json");
    let tampered = std::fs::read_to_string(&index)
        .expect("an index")
        .replace("\"hosts\": [", "\"hosts\": [\"evil.example\", ");
    std::fs::write(&index, tampered).expect("write");
    let mut next = fresh.args(Some(fresh.pack("1.1.0", &[])), "1.1.0");
    next.host.clear();
    refuse(next, "not signed by this key");

    let stale = Hub::new();
    publish(&stale.args(Some(stale.pack("1.0.0", &[])), "1.0.0"), NOW).expect("a release");
    std::fs::write(
        stale.repository.path().join("plug-1.0.0.hplugin"),
        b"swapped",
    )
    .expect("write");
    refuse(
        stale.args(Some(stale.pack("1.1.0", &[])), "1.1.0"),
        "not the package the index lists",
    );

    let locked = Hub::new();
    let package = locked.pack("1.0.0", &[]);
    std::fs::write(locked.home.path().join(LOCK), "api = \"0.1.0\"\n").expect("write");
    refuse(
        locked.args(Some(package), "1.0.0"),
        "asks for something other than plugin.grant",
    );
}

#[test]
fn a_refresh_and_a_revocation_sign_an_index_the_kernel_accepts() {
    let hub = Hub::new();
    publish(&hub.args(Some(hub.pack("1.0.0", &[])), "1.0.0"), NOW).expect("a release");
    publish(&hub.args(Some(hub.pack("1.1.0", &[])), "1.1.0"), NOW).expect("a release");

    let before = hub.accepted(2);
    let later = NOW + 20 * 24 * 3600;
    let refresh = PublishArgs {
        refresh: true,
        ..hub.args(None, "")
    };
    publish(&refresh, later).expect("a refresh");
    let refreshed = Index::verify(
        &std::fs::read(hub.repository.path().join("index.json")).expect("index"),
        &std::fs::read_to_string(hub.repository.path().join("index.json.minisig"))
            .expect("signature"),
        &kernel::Signer::parse(&hub.hub.pk.to_base64()).expect("a key"),
        "hub",
        3,
        later,
    )
    .expect("the kernel accepts it");
    assert!(refreshed.expires > before.expires);

    let revoke = PublishArgs {
        revoke: Some("plug@1.1.0".to_owned()),
        reason: Some("broken".to_owned()),
        ..hub.args(None, "")
    };
    publish(&revoke, later).expect("a revocation");
    let body = std::fs::read(hub.repository.path().join("index.json")).expect("index");
    let signature = std::fs::read_to_string(hub.repository.path().join("index.json.minisig"))
        .expect("signature");
    let signer = kernel::Signer::parse(&hub.hub.pk.to_base64()).expect("a key");
    let revoked = Index::verify(&body, &signature, &signer, "hub", 4, later).expect("accepted");
    assert!(revoked.revocation("plug", "1.1.0").is_some());
    assert!(revoked.release("plug", "1.1.0").is_none(), "never served");
    assert!(revoked.release("plug", "1.0.0").is_some());
    let unknown = PublishArgs {
        revoke: Some("plug@9.9.9".to_owned()),
        ..hub.args(None, "")
    };
    publish(&unknown, later).expect_err("only a published release can be revoked");
}

#[test]
fn the_lockfile_is_the_same_on_every_platform_and_pins_what_a_review_needs() {
    let manifest = Manifest::parse(
        r#"
id = "plug"
name = "Plug"
version = "1.0.0"
[permissions."network:connect"]
hosts = ["a.example", "*.cdn.example"]
unhashed = ["a.example"]
[permissions."process:spawn"]
programs = [
  { path = '/usr/bin/tool', args = ['--say', 'he said "hi"', 'a\b'], os = 'linux' },
  { path = 'C:\tools\t.exe', os = 'windows' },
]
[permissions."ipc:connect"]
endpoints = ['/run/a.sock', '\\.\pipe\a']
[permissions."secrets"]
optional = true
[[schedules]]
id = "nightly"
every = "1d"
[dependencies]
base = "^0.1"
"#,
    )
    .expect("a manifest");
    let shape = Shape {
        imports: [
            "wasi:http/types@0.2.6",
            "wasi:http/outgoing-handler@0.2.6",
            "hestia:kernel/programs@0.1.0",
            "hestia:kernel/ipc@0.1.0",
            "hestia:kernel/secrets@0.1.0",
            "hestia:servers/files@0.1.0",
        ]
        .map(String::from)
        .to_vec(),
        exports: [LIFECYCLE, "hestia:kernel/on-schedule@0.1.0"]
            .map(String::from)
            .to_vec(),
    };
    assert_eq!(quote("a\u{7f}\"\\"), r#""a\u007F\"\\""#);
    let mut grant =
        kernel::derive_everywhere(&kernel::table(), &manifest, &shape).expect("a grant");
    grant
        .permissions
        .iter_mut()
        .find(|p| p.permission == "ipc:connect")
        .expect("ipc")
        .entries = true;
    let rendered = Lock::of(&manifest, &grant, &shape).render();
    let expected = r##"# plugin.grant - written by `hestia plugin check --update-grant`, committed, never edited by hand
api = "0.1.0"
requires = ["hestia:servers"]
imports = ["hestia:servers/files@0.1.0"]

[permissions]
"ipc:connect" = { entries = true, endpoints = ["/run/a.sock", "\\\\.\\pipe\\a"] }
"network:connect" = { hosts = ["*.cdn.example", "a.example"], unhashed = ["a.example"] }
"process:spawn" = { programs = [{ path = "/usr/bin/tool", args = ["--say", "he said \"hi\"", "a\\b"], os = "linux" }, { path = "C:\\tools\\t.exe", args = [], os = "windows" }] }
"secrets" = { optional = true }

[dependencies]
"base" = "^0.1"

[schedules]
"nightly" = "1d"

[contributes]
exports = ["hestia:kernel/lifecycle@0.1.0", "hestia:kernel/on-schedule@0.1.0"]
"##;
    assert_eq!(rendered, expected);
    let parsed: Lock = toml::from_str(&rendered).expect("the file is TOML");
    assert_eq!(parsed, Lock::of(&manifest, &grant, &shape));
}
