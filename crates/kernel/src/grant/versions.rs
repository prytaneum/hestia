//! The version gates: the API a package was built against, and the interfaces
//! it uses that are not part of the stable baseline.

use proto::warning::WarningInfo;
use semver::Version;

use super::table::Stability;
use super::{package_of, unversioned, GrantError, Table, KERNEL_PACKAGE};
use crate::manifest::Manifest;
use crate::shape::Shape;

fn release(version: &Version) -> (u64, u64, u64) {
    (version.major, version.minor, version.patch)
}

/// A plugin runs on a host at its version or a later release of the same line,
/// as wasmtime's linker matches: `0.y` is a line of its own until 1.0, `0.0.z`
/// is exact, a pre-release never matches and build metadata is ignored.
fn provided(wanted: &Version, host: &Version) -> bool {
    if !wanted.pre.is_empty() {
        return false;
    }
    match (wanted.major, wanted.minor) {
        (0, 0) => release(wanted) == release(host),
        (0, minor) => host.major == 0 && host.minor == minor && wanted.patch <= host.patch,
        (major, _) => host.major == major && release(wanted) <= release(host),
    }
}

fn refused(what: &str, asked: &Version, host: &Version) -> GrantError {
    GrantError::Api {
        what: what.to_owned(),
        asked: asked.to_string(),
        host: host.to_string(),
    }
}

/// Refuses a package built against an API this host does not provide, one
/// whose `api` is not the version it imports, one that uses an `@unstable`
/// interface without being a dev install, or one that uses an interface from
/// after the version it names. What it uses that is deprecated comes back as
/// warnings. A package without `api` is held to the versions its imports name.
pub fn check_versions(
    table: &Table,
    manifest: &Manifest,
    shape: &Shape,
    dev: bool,
) -> Result<Vec<WarningInfo>, GrantError> {
    let declared = manifest
        .api
        .as_deref()
        .and_then(|api| Version::parse(api).ok());
    let kernel = table
        .version(KERNEL_PACKAGE)
        .and_then(|host| Version::parse(host).ok());
    if let (Some(api), Some(host)) = (&declared, &kernel) {
        if !provided(api, host) {
            return Err(refused("plugin.toml", api, host));
        }
    }
    let mut warnings = Vec::new();
    for name in shape.imports.iter().chain(&shape.exports) {
        let interface = unversioned(name);
        let Some(package) = package_of(interface) else {
            continue;
        };
        let wanted = name
            .split_once('@')
            .and_then(|(_, text)| Version::parse(text).ok());
        let host = table
            .version(package)
            .and_then(|host| Version::parse(host).ok());
        if let (Some(wanted), Some(host)) = (&wanted, &host) {
            if !provided(wanted, host) {
                return Err(refused(name, wanted, host));
            }
            if let Some(api) = declared.as_ref().filter(|_| package == KERNEL_PACKAGE) {
                if release(api) != release(wanted) {
                    return Err(GrantError::ApiMismatch {
                        api: api.to_string(),
                        import: name.clone(),
                    });
                }
            }
        }
        match table.stability_of(interface) {
            Stability::Baseline => {}
            Stability::Unstable { feature } if !dev => {
                return Err(GrantError::Unstable {
                    interface: interface.to_owned(),
                    feature: feature.to_owned(),
                })
            }
            Stability::Unstable { .. } => {}
            Stability::Stable { since, deprecated } => {
                if let (Some(wanted), Ok(since_version)) = (&wanted, Version::parse(since)) {
                    if since_version > *wanted {
                        return Err(GrantError::Since {
                            interface: interface.to_owned(),
                            since: since.to_owned(),
                            api: wanted.to_string(),
                        });
                    }
                }
                if let Some(deprecated_in) = deprecated {
                    tracing::debug!(interface, deprecated_in, "a deprecated interface is used");
                    warnings.push(WarningInfo::PluginImportDeprecated {
                        interface: interface.to_owned(),
                        deprecated_in: deprecated_in.to_owned(),
                    });
                }
            }
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table::new(
            vec![
                ("hestia:kernel/app", None),
                ("hestia:kernel/preview", None),
                ("hestia:kernel/added", None),
                ("hestia:kernel/legacy", None),
            ],
            vec![],
        )
        .versioned(KERNEL_PACKAGE, "0.2.1")
        .stability(
            "hestia:kernel/preview",
            Stability::Unstable { feature: "preview" },
        )
        .stability(
            "hestia:kernel/added",
            Stability::Stable {
                since: "0.2.1",
                deprecated: None,
            },
        )
        .stability(
            "hestia:kernel/legacy",
            Stability::Stable {
                since: "0.2.0",
                deprecated: Some("0.2.1"),
            },
        )
    }

    fn run(api: Option<&str>, imports: &[&str], dev: bool) -> Result<Vec<WarningInfo>, GrantError> {
        run_shape(api, imports, &[], dev)
    }

    fn run_shape(
        api: Option<&str>,
        imports: &[&str],
        exports: &[&str],
        dev: bool,
    ) -> Result<Vec<WarningInfo>, GrantError> {
        let api = api.map_or(String::new(), |api| format!("api = \"{api}\"\n"));
        let manifest =
            Manifest::parse(&format!("id = \"t\"\nname = \"t\"\nversion = \"1\"\n{api}")).unwrap();
        let shape = Shape {
            imports: imports.iter().map(|name| name.to_string()).collect(),
            exports: exports.iter().map(|name| name.to_string()).collect(),
        };
        check_versions(&table(), &manifest, &shape, dev)
    }

    #[test]
    fn what_the_host_cannot_link_or_the_api_contradicts_is_refused() {
        let app = |version: &str| format!("hestia:kernel/app@{version}");
        assert!(run(None, &[&app("0.2.1+build")], false).is_ok());
        for (api, import) in [
            (None, "0.2.2"),
            (None, "0.2.0-rc.1"),
            (Some("0.2.1"), "0.2.0"),
        ] {
            let error = run(api, &[&app(import)], false).unwrap_err();
            assert!(
                matches!(
                    error,
                    GrantError::Api { .. } | GrantError::ApiMismatch { .. }
                ),
                "{import}: {error}"
            );
        }
        let error = run_shape(None, &[], &[&app("0.3.0")], false).unwrap_err();
        assert!(matches!(error, GrantError::Api { .. }), "{error}");
    }

    #[test]
    fn an_unstable_import_is_for_a_dev_install_only() {
        let preview = "hestia:kernel/preview@0.2.0";
        let error = run(None, &[preview], false).unwrap_err();
        assert!(matches!(error, GrantError::Unstable { .. }), "{error}");
        assert!(run(None, &[preview], true).is_ok());
    }

    #[test]
    fn a_package_cannot_import_what_came_after_the_version_it_names() {
        let error = run(None, &["hestia:kernel/added@0.2.0"], false).unwrap_err();
        assert!(matches!(error, GrantError::Since { .. }), "{error}");
        let added = "hestia:kernel/added@0.2.1";
        assert!(run(Some("0.2.1"), &[added], false).is_ok());
        assert!(run(None, &[added], false).is_ok());
    }

    #[test]
    fn a_deprecated_import_is_a_warning_not_a_refusal() {
        let warnings = run(Some("0.2.0"), &["hestia:kernel/legacy@0.2.0"], false).unwrap();
        assert_eq!(
            warnings,
            [WarningInfo::PluginImportDeprecated {
                interface: "hestia:kernel/legacy".into(),
                deprecated_in: "0.2.1".into(),
            }]
        );
    }
}
