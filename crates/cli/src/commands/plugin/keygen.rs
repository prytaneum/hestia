use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context, Result};
use minisign::KeyPair;

use super::check;
use super::repo::CRATE_MANIFEST;
use crate::ui::{self, View};

const TABLE: &str = "[workspace.metadata.hestia]";
const SECRET_FILE: &str = "hestia.key";
const PUBLIC_FILE: &str = "hestia.pub";

/// Writes a key pair into `out` (the current directory by default) and the
/// public key into the repository's `[workspace.metadata.hestia]`.
pub fn run(out: Option<&Path>, unencrypted: bool) -> Result<()> {
    let repo = check::find(None)?;
    let manifest_path = repo.root().join(CRATE_MANIFEST);
    let manifest = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;

    let out = out.unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(out)?;
    let (secret_path, public_path) = (out.join(SECRET_FILE), out.join(PUBLIC_FILE));
    for path in [&secret_path, &public_path] {
        if path.exists() {
            bail!(
                "{} already exists; keygen never overwrites a key",
                path.display()
            );
        }
    }

    with_key(&manifest, "")?;
    let pair = if unencrypted {
        KeyPair::generate_unencrypted_keypair()?
    } else {
        KeyPair::generate_encrypted_keypair(None)?
    };
    let key = pair.pk.to_base64();
    create_new(&secret_path, &pair.sk.to_box(None)?.to_string(), true)?;
    create_new(&public_path, &pair.pk.to_box()?.to_string(), false)?;
    std::fs::write(&manifest_path, with_key(&manifest, &key)?)
        .with_context(|| format!("cannot write {}", manifest_path.display()))?;
    tracing::info!(key = %key, "repository key generated");

    ui::show(View::detail([
        ("Secret key", secret_path.display().to_string()),
        ("Public key", public_path.display().to_string()),
        ("Declared in", manifest_path.display().to_string()),
    ]))?;
    ui::show(View::note(
        "keep the secret key out of the repository and give it to CI as a secret; losing it means readers must remove and re-add the repository",
    ))
}

/// `manifest` with `key = "<key>"` inside `[workspace.metadata.hestia]`:
/// replaces the existing line, else inserts one after the header.
fn with_key(manifest: &str, key: &str) -> Result<String> {
    let mut lines: Vec<&str> = manifest.lines().collect();
    let Some(header) = lines.iter().position(|line| line.trim() == TABLE) else {
        bail!("{CRATE_MANIFEST} has no {TABLE} table; see docs/plugins/repository.md");
    };
    let end = lines[header + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| header + 1 + offset);
    let line = format!("key = \"{key}\"");
    let existing = lines[header + 1..end].iter().position(|line| {
        line.trim_start()
            .strip_prefix("key")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    });
    match existing {
        Some(offset) => lines[header + 1 + offset] = &line,
        None => lines.insert(header + 1, &line),
    }
    let mut text = lines.join("\n");
    if manifest.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

fn create_new(path: &Path, content: &str, secret: bool) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if secret {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = secret;
    options
        .open(path)
        .and_then(|mut file| file.write_all(content.as_bytes()))
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_line_is_replaced_in_place() {
        let manifest =
            "[workspace]\n\n[workspace.metadata.hestia]\nname = \"S\"\nkey = \"old\"\nlabel = \"L\"\n\n[other]\nkey = \"untouched\"\n";

        let text = with_key(manifest, "RWnew").unwrap();

        assert_eq!(
            text,
            "[workspace]\n\n[workspace.metadata.hestia]\nname = \"S\"\nkey = \"RWnew\"\nlabel = \"L\"\n\n[other]\nkey = \"untouched\"\n"
        );
    }

    #[test]
    fn a_missing_key_line_is_inserted_after_the_header() {
        let manifest =
            "[workspace.metadata.hestia]\nname = \"S\"\n\n[other]\nkey = \"untouched\"\n";

        let text = with_key(manifest, "RWnew").unwrap();

        assert_eq!(
            text,
            "[workspace.metadata.hestia]\nkey = \"RWnew\"\nname = \"S\"\n\n[other]\nkey = \"untouched\"\n"
        );
    }

    #[test]
    fn a_missing_table_is_refused() {
        assert!(with_key("[workspace]\n", "RWnew").is_err());
    }
}
