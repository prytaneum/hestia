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
        CommandArg, CommandContribution, CommandScope, Manifest, ManifestError, Permission,
        Program, ProviderContribution, Setting, SettingKind, ThemeContribution,
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
    fn a_theme_only_package_needs_no_api() {
        let mut theme = valid();
        theme.api = None;
        theme.wasm = None;
        assert_eq!(theme.validate(), Ok(()));
        theme.wasm = Some("plugin.wasm".into());
        assert_eq!(theme.validate(), Err(ManifestError::MissingApi));
    }

    fn hooked(hook: &str, permissions: &[Permission]) -> Manifest {
        let mut manifest = valid();
        manifest.contributes.hooks.push(hook.into());
        manifest.permissions = permissions.to_vec();
        manifest
    }

    #[test]
    fn a_hook_needs_its_permissions_and_must_be_known() {
        assert_eq!(
            hooked("before-launch", &[Permission::LaunchPatch]).validate(),
            Err(ManifestError::HookPermissionMissing {
                hook: "before-launch".into(),
                permission: Permission::AccountRead,
            })
        );
        assert_eq!(
            hooked("before-content-install", &[]).validate(),
            Err(ManifestError::HookPermissionMissing {
                hook: "before-content-install".into(),
                permission: Permission::ContentRead,
            })
        );
        assert_eq!(
            hooked("on-tuesday", &[]).validate(),
            Err(ManifestError::UnknownHook("on-tuesday".into()))
        );
    }

    #[test]
    fn every_hook_validates_and_stays_sandboxed() {
        for (name, needs) in proto::plugin::HOOKS {
            let manifest = hooked(name, needs);
            assert_eq!(manifest.validate(), Ok(()), "{name}");
            assert!(manifest.grant().unrestricted().is_empty(), "{name}");
        }
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
    fn event_patterns_are_a_topic_or_a_trailing_glob() {
        let mut manifest = valid();
        manifest.contributes.events = vec!["server.*".into(), "job".into()];
        assert_eq!(manifest.validate(), Ok(()));
        for bad in ["*", "server*", "a.*.b", ""] {
            manifest.contributes.events = vec![bad.into()];
            assert_eq!(
                manifest.validate(),
                Err(ManifestError::BadEventPattern(bad.into()))
            );
        }
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
    fn network_hosts_are_names_never_addresses() {
        for good in [
            "example.com",
            "*.example.com",
            "localhost",
            "api.v2-x.io",
            "Example.COM.",
        ] {
            let manifest = Manifest {
                network: vec![good.into()],
                ..valid()
            };
            assert_eq!(manifest.validate(), Ok(()), "{good}");
        }
        for bad in [
            "",
            "*",
            "*.com",
            "*.",
            "127.0.0.1",
            "1.2.3.4",
            "[::1]",
            "::1",
            "a b.com",
            "a..com",
            "-a.com",
            "https://example.com",
            "example.com/x",
            "example.com:443",
            "*.*.com",
        ] {
            let manifest = Manifest {
                network: vec![bad.into()],
                ..valid()
            };
            assert_eq!(
                manifest.validate(),
                Err(ManifestError::BadNetworkHost(bad.into())),
                "{bad}"
            );
        }
    }

    fn command(scope: CommandScope, args: &[&str]) -> Manifest {
        let mut manifest = valid();
        manifest.contributes.commands.push(CommandContribution {
            id: "open".into(),
            scope,
            args: args
                .iter()
                .map(|name| CommandArg {
                    name: (*name).into(),
                    ..CommandArg::default()
                })
                .collect(),
            ..CommandContribution::default()
        });
        manifest
    }

    #[test]
    fn command_arguments_are_slugs_declared_once_and_never_the_entry_of_a_scoped_command() {
        assert_eq!(
            command(CommandScope::Top, &["name", "port"]).validate(),
            Ok(())
        );
        assert_eq!(command(CommandScope::Top, &["entry"]).validate(), Ok(()));
        assert!(matches!(
            command(CommandScope::Top, &["Bad Name"]).validate(),
            Err(ManifestError::BadContributionId(_))
        ));
        assert_eq!(
            command(CommandScope::Top, &["name", "name"]).validate(),
            Err(ManifestError::DuplicateId("name".into()))
        );
        assert_eq!(
            command(CommandScope::Server, &["entry"]).validate(),
            Err(ManifestError::ReservedArg("open".into()))
        );
    }

    #[test]
    fn a_scoped_command_may_not_take_a_built_in_verb() {
        for (scope, verb) in [
            (CommandScope::Server, "backup"),
            (CommandScope::Instance, "launch"),
        ] {
            let mut manifest = command(scope, &[]);
            manifest.contributes.commands[0].id = verb.into();
            assert_eq!(
                manifest.validate(),
                Err(ManifestError::ReservedCommand(verb.into()))
            );
        }
        let mut manifest = command(CommandScope::Top, &[]);
        manifest.contributes.commands[0].id = "backup".into();
        assert_eq!(manifest.validate(), Ok(()));
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
                permissions: vec![Permission::NetworkConnect],
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
        hooked.contributes.hooks.push("before-launch".into());
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
}
