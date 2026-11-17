//! The WIT is the API, and a published version never changes: the version and
//! stability the host's tables state are what the WIT says, and each package
//! under its version is the one `crates/sdk/wit/snapshot.txt` recorded.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::probe::Probe;
use kernel::{Gate, Plugins, Stability, Table};
use sha2::{Digest, Sha256};
use wit_parser::Resolve;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn table() -> Table {
    let home = tempfile::tempdir().unwrap();
    Plugins::new(home.path(), vec![Arc::new(Probe)], None)
        .unwrap()
        .table()
        .clone()
}

fn packages() -> [(&'static str, PathBuf); 2] {
    [
        ("hestia:kernel", root().join("sdk/wit/kernel")),
        ("hestia:probe", root().join("kernel/src/api/probe/wit")),
    ]
}

fn described(stability: &wit_parser::Stability) -> String {
    match stability {
        wit_parser::Stability::Unknown => "baseline".to_owned(),
        wit_parser::Stability::Stable { since, deprecated } => {
            let deprecated = deprecated.as_ref().map(ToString::to_string);
            format!("stable since {since} deprecated {deprecated:?}")
        }
        wit_parser::Stability::Unstable { feature, .. } => format!("unstable {feature}"),
    }
}

fn stated(stability: Stability) -> String {
    match stability {
        Stability::Baseline => "baseline".to_owned(),
        Stability::Stable { since, deprecated } => {
            format!("stable since {since} deprecated {deprecated:?}")
        }
        Stability::Unstable { feature } => format!("unstable {feature}"),
    }
}

#[test]
fn the_tables_state_the_version_and_stability_the_wit_declares() {
    let mut resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    for (_, dir) in packages() {
        resolve.push_dir(dir).unwrap();
    }

    let table = table();
    for (name, _) in packages() {
        let package = resolve
            .packages
            .iter()
            .find(|(_, p)| format!("{}:{}", p.name.namespace, p.name.name) == name)
            .map(|(_, package)| package)
            .unwrap_or_else(|| panic!("no WIT package {name}"));
        let version = package.name.version.as_ref().unwrap();
        assert_eq!(
            table.version(name),
            Some(version.to_string().as_str()),
            "{name}"
        );
        assert!(!package.interfaces.is_empty(), "{name} has no interfaces");
        let mut attributed = 0;
        for (interface, id) in &package.interfaces {
            let interface_stability = &resolve.interfaces[*id].stability;
            let full = format!("{name}/{interface}");
            assert!(
                table.interfaces().any(|row| row == full),
                "{full} is in the WIT but not in the table"
            );
            if let wit_parser::Stability::Stable { since, .. } = interface_stability {
                assert!(since <= version, "{full} is since {since}, after {version}");
            }
            assert_eq!(
                stated(table.stability_of(&full)),
                described(interface_stability),
                "{full}"
            );
            attributed += usize::from(described(interface_stability) != "baseline");
            let documented = resolve.interfaces[*id]
                .docs
                .contents
                .as_deref()
                .and_then(|docs| docs.split("Permission `").nth(1))
                .and_then(|rest| rest.split('`').next());
            let gated = match table.gate(&full) {
                Gate::Needs(permission) => Some(permission.name),
                _ => None,
            };
            if name == "hestia:kernel" {
                assert_eq!(
                    documented, gated,
                    "{full}: the documented permission is the table's (the SDK mock reads it)"
                );
            }
            for (function, found) in &resolve.interfaces[*id].functions {
                let own = described(&found.stability);
                assert!(
                    !matches!(found.stability, wit_parser::Stability::Unstable { .. }),
                    "{full}.{function} is @unstable: put it in an interface marked @unstable"
                );
                assert!(
                    own == "baseline" || own == described(interface_stability),
                    "{full}.{function} carries {own} its interface lacks: version attributes \
                     belong on interfaces, so put it in a new interface marked @since"
                );
            }
        }
        for row in table
            .interfaces()
            .filter(|row| row.starts_with(&format!("{name}/")))
        {
            assert!(
                package
                    .interfaces
                    .keys()
                    .any(|interface| format!("{name}/{interface}") == row),
                "{row} is in the table but not in the WIT"
            );
        }
        assert_eq!(
            table
                .stabilities()
                .filter(|(row, _)| row.starts_with(&format!("{name}/")))
                .count(),
            attributed,
            "{name}: a stability row names an interface the WIT does not mark"
        );
    }
}

/// The token stream of a WIT file: comments and layout do not change what a
/// guest links against, every attribute and name does.
fn tokens(wit: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut chars = wit.chars().peekable();
    let flush = |word: &mut String, tokens: &mut Vec<String>| {
        if !word.is_empty() {
            tokens.push(std::mem::take(word));
        }
    };
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('/', Some('/')) => {
                flush(&mut word, &mut tokens);
                while chars.next_if(|next| *next != '\n').is_some() {}
            }
            ('/', Some('*')) => {
                flush(&mut word, &mut tokens);
                chars.next();
                let mut depth = 1;
                while let (Some(c), Some(next)) = (chars.next(), chars.peek().copied()) {
                    match (c, next) {
                        ('/', '*') => depth += 1,
                        ('*', '/') => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        chars.next();
                        break;
                    }
                }
            }
            (c, _) if c.is_alphanumeric() || "_-.%/".contains(c) => word.push(c),
            (c, _) => {
                flush(&mut word, &mut tokens);
                if !c.is_whitespace() {
                    tokens.push(c.to_string());
                }
            }
        }
    }
    flush(&mut word, &mut tokens);
    tokens.join(" ")
}

#[test]
fn a_changed_wit_needs_a_new_version() {
    let snapshot = std::fs::read_to_string(root().join("sdk/wit/snapshot.txt")).unwrap();
    let table = table();
    for (name, dir) in packages() {
        let mut files: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "wit"))
            .collect();
        files.sort();
        let mut hash = Sha256::new();
        for file in &files {
            hash.update(tokens(&std::fs::read_to_string(file).unwrap()));
            hash.update(b"\n");
        }
        let hash: String = hex::encode(hash.finalize());

        let version = table.version(name).unwrap();
        let recorded = snapshot
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(' ');
                Some((fields.next()?, fields.next()?, fields.next()?))
            })
            .find(|(package, recorded, _)| *package == name && *recorded == version)
            .map(|(_, _, hash)| hash)
            .unwrap_or_else(|| {
                panic!("{name} {version} is new: append `{name} {version} {hash}` to sdk/wit/snapshot.txt")
            });
        assert_eq!(
            recorded, hash,
            "{name} changed under version {version}: a published version never changes, so \
             raise the package version (see sdk/wit/README.md) and record it in sdk/wit/snapshot.txt"
        );
    }
}

#[test]
fn only_what_a_guest_links_against_changes_the_hash() {
    let plain = "interface a { f: func() -> string; }";
    let dressed = "// head\ninterface a { /* x /* y */ */ f:func()->string; // tail\n}";
    assert_eq!(tokens(plain), tokens(dressed));
    assert_ne!(
        tokens(plain),
        tokens("@unstable(feature = x) interface a { f: func() -> string; }")
    );
    assert_ne!(
        tokens(plain),
        tokens("interface a { g: func() -> string; }")
    );
}
