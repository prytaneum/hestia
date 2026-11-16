use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub fn scratch() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

pub fn wasm(component: PathBuf) -> Vec<u8> {
    std::fs::read(component).expect("read the guest component")
}

/// A component from `tests/guest/`.
pub fn guest(name: &str) -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| build("guest"))
        .join(format!("{name}.wasm"))
}

/// A component from `tests/guest-services/`.
pub fn service_guest(name: &str) -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| build("guest-services"))
        .join(format!("{name}.wasm"))
}

/// Every guest workspace shares one target directory, so their common
/// dependencies compile once.
fn build(workspace: &str) -> PathBuf {
    let target = scratch().join("guest");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kernel/tests")
        .join(workspace)
        .join("Cargo.toml");
    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            "wasm32-wasip2",
            "--manifest-path",
        ])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("run cargo for the guest plugins");
    assert!(
        output.status.success(),
        "guest plugins failed to build:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    target.join("wasm32-wasip2/release")
}
