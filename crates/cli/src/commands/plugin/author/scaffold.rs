//! `hestia plugin new`: a Rust plugin project that builds, checks and packs.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;
use proto::plugin::Grant;

use super::lock::Lock;
use super::project::{LOCK, MANIFEST};
use crate::ui::{self, View};

const REPOSITORY: &str = "https://github.com/prytaneum/hestia";

#[derive(Args)]
pub struct NewArgs {
    /// The plugin's id: lowercase letters, digits and '-'; also its directory
    id: String,
    /// Depend on the SDK in this checkout of Hestia instead of a release
    #[arg(long, value_name = "DIR")]
    sdk: Option<PathBuf>,
    /// The Hestia release the SDK and the CI's `hestia` come from
    #[arg(long, value_name = "TAG")]
    tag: Option<String>,
}

fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn sdk_dependency(args: &NewArgs, tag: &str) -> Result<String> {
    let features = r#"features = ["lifecycle"]"#;
    Ok(match &args.sdk {
        Some(dir) => {
            let dir = std::fs::canonicalize(dir)
                .with_context(|| format!("cannot read {}", dir.display()))?;
            let dir = dir.join("crates/sdk");
            if !dir.join("Cargo.toml").is_file() {
                bail!("{} holds no crates/sdk", dir.display());
            }
            format!(
                "hestia-sdk = {{ path = {}, {features} }}",
                quote(&dir.to_string_lossy())
            )
        }
        None => format!(
            "hestia-sdk = {{ git = {}, tag = {}, {features} }}",
            quote(REPOSITORY),
            quote(tag)
        ),
    })
}

fn files(args: &NewArgs, tag: &str) -> Result<Vec<(&'static str, String)>> {
    let id = &args.id;
    let api = kernel::table().version("hestia:kernel").unwrap_or_default();
    let grant = Grant {
        contributes: vec![format!("hestia:kernel/lifecycle@{api}")],
        ..Grant::default()
    };
    let lock = Lock::of(&grant, Some(api.to_owned())).render();
    Ok(vec![
        (
            "Cargo.toml",
            format!(
                "[package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n\
                 [workspace]\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\n{}\n\n[profile.release]\nopt-level = \"s\"\nlto = true\nstrip = true\n",
                quote(id),
                sdk_dependency(args, tag)?
            ),
        ),
        (
            "src/lib.rs",
            format!(
                "use hestia::log::{{write, Level}};\nuse hestia::{{Error, Lifecycle}};\n\n\
                 struct Plugin;\n\nimpl Lifecycle for Plugin {{\n    fn init() -> Result<(), Error> {{\n        \
                 write(Level::Info, \"{id} started\");\n        Ok(())\n    }}\n}}\n\nhestia::export!(Plugin);\n"
            ),
        ),
        (
            MANIFEST,
            format!("id = {}\nname = {}\nversion = \"0.1.0\"\n", quote(id), quote(id)),
        ),
        (LOCK, lock),
        (".gitignore", "/target\n/plugin.wasm\n*.hplugin\n".to_owned()),
        (
            ".github/workflows/ci.yml",
            format!(
                "name: ci\non: [push, pull_request]\n\njobs:\n  plugin:\n    runs-on: ubuntu-latest\n    steps:\n      \
                 - uses: actions/checkout@v4\n      - uses: dtolnay/rust-toolchain@stable\n        with:\n          \
                 targets: wasm32-wasip2\n      - run: cargo install --git {REPOSITORY} --tag {tag} --locked cli\n      \
                 - run: hestia plugin test\n      - run: hestia plugin check\n"
            ),
        ),
    ])
}

pub fn run(args: NewArgs) -> Result<()> {
    let tag = args
        .tag
        .clone()
        .unwrap_or_else(|| format!("v{}", common::app::VERSION));
    let files = files(&args, &tag)?;
    let manifest = files
        .iter()
        .find_map(|(name, text)| (*name == MANIFEST).then_some(text))
        .context("no manifest")?;
    kernel::Manifest::parse(manifest)?;
    let dir = Path::new(&args.id);
    if dir.exists() {
        bail!("{} already exists", dir.display());
    }
    for (name, text) in &files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
    }
    ui::show(View::line(format!(
        "created {}; build it with `hestia plugin check`",
        dir.display()
    )))
}
