//! The author's flow end to end, against hand-assembled components: no build,
//! and the kernel's own verifiers decide whether what was written is accepted.

use std::path::Path;

use kernel::{Hplugin, Index, Stability, Table};

use super::inspect::review;
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
fn a_signed_package_is_what_the_kernel_verifies_and_refuses_unstable_imports() {
    let home = tempfile::tempdir().expect("a directory");
    let author = keys();
    let project = source(home.path(), "1.0.0", &[]);
    review(&kernel::table(), &project, false, true);
    let secret = super::key::load(&key_file(home.path(), "author.key", &author)).expect("key");

    let packed = package(&kernel::table(), &project, Some(&secret)).expect("a package");
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
    package(&preview, &unstable, None).expect("an unsigned package may use it");
    let strict = review(&preview, &unstable, false, false);
    assert!(strict.errors[0].contains("unstable"), "{:?}", strict.errors);
    package(&preview, &unstable, Some(&secret)).expect_err("signing refuses it");
}

#[test]
fn publish_writes_an_index_the_kernel_accepts_and_stops_a_widening() {
    let home = tempfile::tempdir().expect("a directory");
    let repository = tempfile::tempdir().expect("a directory");
    let (author, hub) = (keys(), keys());
    let author_key = super::key::load(&key_file(home.path(), "author.key", &author)).expect("key");
    let hub_key = key_file(home.path(), "hub.key", &hub);

    let pack = |version: &str, imports: &[&str]| {
        let project = source(home.path(), version, imports);
        review(&kernel::table(), &project, false, true);
        let packed = package(&kernel::table(), &project, Some(&author_key)).expect("a package");
        let file = home.path().join(&packed.file_name);
        std::fs::write(&file, packed.archive).expect("write");
        file
    };
    let args = |package, version: &str, allow_widen| PublishArgs {
        package,
        repository: repository.path().to_owned(),
        key: hub_key.clone(),
        url: format!("https://example.com/plugins/plug-{version}.hplugin"),
        author: Some(author.pk.to_base64()),
        name: Some("hub".to_owned()),
        allow_widen,
    };
    let accepted = |floor: u64| {
        let body = std::fs::read(repository.path().join("index.json")).expect("an index");
        let signature = std::fs::read_to_string(repository.path().join("index.json.minisig"))
            .expect("a signature");
        let signer = kernel::Signer::parse(&hub.pk.to_base64()).expect("a key");
        Index::verify(&body, &signature, &signer, "hub", floor, NOW).expect("the kernel accepts it")
    };

    publish(&args(pack("1.0.0", &[]), "1.0.0", false), NOW).expect("the first release");
    assert_eq!(accepted(1).plugins[0].releases.len(), 1);

    let wider = pack("1.1.0", &[SECRETS]);
    let refused = publish(&args(wider.clone(), "1.1.0", false), NOW).expect_err("a widening");
    assert!(
        format!("{refused:#}").contains("--allow-widen"),
        "{refused:#}"
    );
    assert_eq!(accepted(1).sequence, 1);

    publish(&args(wider, "1.1.0", true), NOW).expect("allowed to widen");
    let index = accepted(2);
    assert_eq!(index.plugins[0].releases.len(), 2);
    assert!(repository.path().join("plug-1.1.0.hplugin").is_file());
}
