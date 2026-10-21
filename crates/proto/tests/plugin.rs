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

mod manifest {
    use proto::plugin::{
        Manifest, ManifestError, Permission, Program, ProviderContribution, Setting, SettingKind,
        ThemeContribution,
    };

    fn valid() -> Manifest {
        Manifest {
            id: "tunnel".into(),
            version: "1.2.3".into(),
            api: Some("^1.0.0".into()),
            wasm: Some("plugin.wasm".into()),
            ..Manifest::default()
        }
    }

    #[test]
    fn a_well_formed_manifest_validates() {
        assert_eq!(valid().validate(), Ok(()));
    }

    #[test]
    fn a_theme_only_package_needs_no_api() {
        let mut theme = valid();
        theme.api = None;
        theme.wasm = None;
        assert_eq!(theme.validate(), Ok(()));
        theme.wasm = Some("plugin.wasm".into());
        assert_eq!(theme.validate(), Err(ManifestError::MissingApi));
    }

    #[test]
    fn rejects_paths_that_leave_the_package() {
        for path in [
            "../x.wasm",
            "/etc/x.wasm",
            "a/../../x",
            "C:/x.wasm",
            "a\\b",
            "a//b",
            "",
        ] {
            let mut manifest = valid();
            manifest.wasm = Some(path.into());
            assert_eq!(
                manifest.validate(),
                Err(ManifestError::PathEscapes(path.into())),
                "{path}"
            );
        }
        let mut manifest = valid();
        manifest.contributes.themes.push(ThemeContribution {
            id: "dusk".into(),
            file: "../dusk.css".into(),
        });
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::PathEscapes(_))
        ));
    }

    #[test]
    fn rejects_reserved_and_malformed_ids() {
        for (id, reserved) in [
            ("server", true),
            ("Bad", false),
            ("a", false),
            ("-ab", false),
        ] {
            let mut manifest = valid();
            manifest.id = id.into();
            let expected = if reserved {
                ManifestError::ReservedId(id.into())
            } else {
                ManifestError::BadId(id.into())
            };
            assert_eq!(manifest.validate(), Err(expected));
        }
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "content".into(),
            id: "Not A Slug".into(),
        });
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::BadContributionId(_))
        ));
    }

    #[test]
    fn rejects_a_repeated_id_within_a_list() {
        let mut manifest = valid();
        manifest.programs = ["a", "a"]
            .map(|id| Program {
                id: id.into(),
                ..Program::default()
            })
            .into();
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::DuplicateId("a".into()))
        );
    }

    #[test]
    fn rejects_a_setting_key_declared_twice() {
        let mut manifest = valid();
        manifest.settings = [SettingKind::String, SettingKind::Secret]
            .map(|kind| Setting {
                key: "token".into(),
                kind,
            })
            .into();
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::DuplicateId("token".into()))
        );
    }

    #[test]
    fn api_must_share_the_host_major_and_not_be_newer() {
        for api in ["^2.0.0", "^1.1.0", "^1.0.1"] {
            let mut manifest = valid();
            manifest.api = Some(api.into());
            assert!(
                matches!(manifest.validate(), Err(ManifestError::ApiMismatch { .. })),
                "{api}"
            );
        }
        let mut manifest = valid();
        manifest.api = Some("1.0.0".into());
        assert!(matches!(manifest.validate(), Err(ManifestError::BadApi(_))));
    }

    #[test]
    fn widening_a_manifest_changes_its_grant() {
        let base = valid();
        let widened: Vec<Manifest> = vec![
            Manifest {
                permissions: vec![Permission::Network],
                ..base.clone()
            },
            Manifest {
                network: vec!["example.com".into()],
                ..base.clone()
            },
            Manifest {
                programs: vec![Program {
                    id: "ts".into(),
                    command: "tailscale".into(),
                    ..Program::default()
                }],
                ..base.clone()
            },
            Manifest {
                programs: vec![Program {
                    id: "ts".into(),
                    command: "tailscale".into(),
                    env: ["TOKEN".to_string()].into(),
                    ..Program::default()
                }],
                ..base.clone()
            },
            Manifest {
                settings: vec![Setting {
                    key: "key".into(),
                    kind: SettingKind::Secret,
                }],
                ..base.clone()
            },
        ];
        let mut hooked = base.clone();
        hooked.contributes.hooks.push("launch".into());
        let digests: Vec<String> = widened
            .iter()
            .chain([&hooked])
            .map(|manifest| manifest.grant().digest())
            .collect();
        for (index, digest) in digests.iter().enumerate() {
            assert_ne!(*digest, base.grant().digest());
            assert!(!digests[..index].contains(digest));
        }
    }

    #[test]
    fn a_grant_ignores_declaration_order() {
        let mut a = valid();
        a.permissions = vec![Permission::Network, Permission::ContentRead];
        let mut b = valid();
        b.permissions = vec![Permission::ContentRead, Permission::Network];
        assert_eq!(a.grant().digest(), b.grant().digest());
    }
}
