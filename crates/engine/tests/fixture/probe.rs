//! Builds the `probe` plugin once per test run, for `plugins` and `flows`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub fn component() -> &'static Path {
    static COMPONENT: OnceLock<PathBuf> = OnceLock::new();
    COMPONENT.get_or_init(|| {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture/plugins/probe/Cargo.toml");
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
            "building the probe plugin failed (is the target installed? \
             `rustup target add wasm32-wasip2`):\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target_dir.join("wasm32-wasip2/release/probe.wasm")
    })
}
