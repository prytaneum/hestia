//! `pack` reads the plugin API a package needs from the SDK's version, so it
//! must be the WIT package version, which `proto` already pins `API_VERSION` to.

#[test]
fn sdk_version_is_the_plugin_api_version() {
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        hestia::proto::plugin::API_VERSION
    );
}
