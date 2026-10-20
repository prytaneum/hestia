//! Drift guard: `API_VERSION` is the version of the WIT package, typed once there.

use std::fs;
use std::path::Path;

#[test]
fn api_version_matches_the_wit_package() {
    let wit = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sdk/wit/plugin.wit");
    let source = fs::read_to_string(wit).unwrap();
    let declared = source
        .lines()
        .find_map(|line| line.strip_prefix("package hestia:plugin@"))
        .and_then(|rest| rest.strip_suffix(';'))
        .expect("plugin.wit declares its package version");
    assert_eq!(proto::plugin::API_VERSION, declared);
}
