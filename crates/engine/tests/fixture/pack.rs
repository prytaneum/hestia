//! Builds the `replay-format` fixture plugin once per test binary.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub const MANIFEST: &str = include_str!("plugins/replay-format/plugin.toml");

pub fn component() -> &'static Path {
    static COMPONENT: OnceLock<PathBuf> = OnceLock::new();
    COMPONENT.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixture/plugins/replay-format/Cargo.toml");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins");
        let output = Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target_dir)
            .output()
            .expect("run cargo");
        assert!(
            output.status.success(),
            "building the replay-format plugin failed (is the target installed? \
             `rustup target add wasm32-wasip2`):\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target_dir.join("wasm32-wasip2/release/replay_format.wasm")
    })
}
