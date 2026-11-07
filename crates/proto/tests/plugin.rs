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
    use proto::error::EntryKind;
    use proto::plugin::{
        CommandArg, CommandContribution, CommandScope, Manifest, ManifestError, PageContribution,
        PageSlot, Permission, Program, ProviderContribution, Setting, SettingKind,
        ThemeContribution, Tier,
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
    fn a_declared_launch_property_must_be_one_no_plugin_is_refused() {
        let declaring = |key: &str| {
            let mut manifest = valid();
            manifest.launch.properties.push(key.into());
            manifest.validate()
        };
        assert_eq!(declaring("my.mod.flag"), Ok(()));
        for key in [
            "java.library.path",
            "Java.Library.Path",
            "java.system.class.loader",
            "java.security.manager",
            "jdk.module.path",
            "log4j2.configurationFile",
            "java.class.path",
            "my.agent.path",
            "",
            "has space",
            "a=b",
        ] {
            assert_eq!(
                declaring(key),
                Err(ManifestError::BadLaunchProperty(key.into())),
                "{key}"
            );
        }
    }

    #[test]
    fn every_hook_validates_and_stays_sandboxed() {
        for point in proto::hook::POINTS {
            let (name, needs) = (point.name, point.permissions);
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
            ..Default::default()
        });
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::PathEscapes(_))
        ));
    }

    #[test]
    fn a_theme_needs_the_ui_css_permission() {
        let mut manifest = valid();
        manifest.contributes.themes.push(ThemeContribution {
            id: "dusk".into(),
            name: "Dusk".into(),
            file: "themes/dusk.css".into(),
            ..Default::default()
        });
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ThemeNeedsUiCss("dusk".into()))
        );
        manifest.permissions.push(Permission::UiCss);
        assert_eq!(manifest.validate(), Ok(()));
    }

    fn paged(slot: PageSlot, kind: Option<EntryKind>, icon: Option<&str>) -> Manifest {
        let mut manifest = valid();
        manifest.contributes.pages.push(PageContribution {
            id: "overview".into(),
            title: "Overview".into(),
            icon: icon.map(str::to_string),
            slot,
            kind,
        });
        manifest
    }

    #[test]
    fn an_entry_page_needs_a_kind() {
        assert_eq!(paged(PageSlot::Nav, None, None).validate(), Ok(()));
        assert_eq!(
            paged(PageSlot::Entry, None, None).validate(),
            Err(ManifestError::PageNeedsKind("overview".into()))
        );
        assert_eq!(
            paged(PageSlot::Entry, Some(EntryKind::Server), None).validate(),
            Ok(())
        );
    }

    #[test]
    fn a_page_icon_is_a_name_or_a_path_inside_the_package() {
        assert_eq!(paged(PageSlot::Nav, None, Some("chart")).validate(), Ok(()));
        assert_eq!(
            paged(PageSlot::Nav, None, Some("assets/icon.svg")).validate(),
            Ok(())
        );
        for icon in ["../icon.svg", "/icon.svg", "a/../../icon.png"] {
            assert_eq!(
                paged(PageSlot::Nav, None, Some(icon)).validate(),
                Err(ManifestError::PathEscapes(icon.into())),
                "{icon}"
            );
        }
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
    fn a_provider_needs_content_read_and_network_hosts() {
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "content".into(),
            id: "index".into(),
        });
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "index".into(),
                permission: Permission::ContentRead
            })
        );
        manifest.permissions = vec![Permission::ContentRead, Permission::NetworkConnect];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProviderNeedsHosts("index".into()))
        );
        manifest.network = vec!["index.example.com".into()];
        assert_eq!(manifest.validate(), Ok(()));
        manifest.contributes.providers[0].kind = "mystery".into();
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::UnknownProviderKind("mystery".into()))
        );
    }

    #[test]
    fn a_flavor_needs_flavor_provide_and_network_hosts() {
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "flavor".into(),
            id: "quilt".into(),
        });
        manifest.permissions = vec![Permission::ContentRead, Permission::NetworkConnect];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "quilt".into(),
                permission: Permission::FlavorProvide
            })
        );
        manifest.permissions = vec![Permission::FlavorProvide, Permission::NetworkConnect];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProviderNeedsHosts("quilt".into()))
        );
        manifest.network = vec!["meta.quiltmc.org".into()];
        assert_eq!(manifest.validate(), Ok(()));
        assert_eq!(Permission::FlavorProvide.tier(), Tier::Unrestricted);
    }

    #[test]
    fn a_format_needs_format_provide_and_no_hosts() {
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "format".into(),
            id: "prism".into(),
        });
        manifest.permissions = vec![Permission::ContentRead];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "prism".into(),
                permission: Permission::FormatProvide
            })
        );
        manifest.permissions = vec![Permission::FormatProvide];
        assert_eq!(manifest.validate(), Ok(()));
        assert_eq!(Permission::FormatProvide.tier(), Tier::Unrestricted);
    }

    #[test]
    fn a_java_provider_needs_java_provide_and_network_hosts() {
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "java".into(),
            id: "zulu".into(),
        });
        manifest.permissions = vec![Permission::FlavorProvide, Permission::NetworkConnect];
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "zulu".into(),
                permission: Permission::JavaProvide
            })
        );
        manifest.permissions = vec![Permission::JavaProvide, Permission::NetworkConnect];
        manifest.network = vec!["api.azul.com".into()];
        assert_eq!(manifest.validate(), Ok(()));
        assert_eq!(Permission::JavaProvide.tier(), Tier::Unrestricted);
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
                ..Setting::default()
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

    fn spawning(program: Program) -> Manifest {
        Manifest {
            permissions: vec![Permission::ProcessSpawn],
            programs: vec![Program {
                id: "agent".into(),
                ..program
            }],
            ..valid()
        }
    }

    fn path_program(command: &str, args: &[&str]) -> Program {
        Program {
            command: command.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            ..Program::default()
        }
    }

    #[test]
    fn a_program_needs_the_spawn_permission() {
        let manifest = Manifest {
            permissions: vec![],
            ..spawning(path_program("tailscale", &[]))
        };
        assert_eq!(
            manifest.validate(),
            Err(ManifestError::ProgramPermissionMissing)
        );
        assert_eq!(spawning(path_program("tailscale", &[])).validate(), Ok(()));
    }

    #[test]
    fn a_program_declaring_a_loader_variable_is_rejected() {
        for name in [
            "LD_PRELOAD",
            "ld_library_path",
            "DYLD_INSERT_LIBRARIES",
            "PATH",
            "Path",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "JDK_JAVA_OPTIONS",
            "1BAD",
            "A=B",
            "",
        ] {
            let manifest = spawning(Program {
                env: [name.to_string()].into(),
                ..path_program("tailscale", &[])
            });
            assert_eq!(
                manifest.validate(),
                Err(ManifestError::BadProgramEnv(name.into())),
                "{name}"
            );
        }
        let allowed = spawning(Program {
            env: ["TS_AUTHKEY".to_string(), "JAVA_HOME".to_string()].into(),
            ..path_program("tailscale", &[])
        });
        assert_eq!(allowed.validate(), Ok(()));
    }

    #[test]
    fn a_program_is_a_plain_name_or_a_hashed_file_inside_data() {
        let hash = "0".repeat(64);
        let hashed = |command: &str, hash: &str| Program {
            sha256: [("x86_64-unknown-linux-gnu".to_string(), hash.to_string())].into(),
            ..path_program(command, &[])
        };
        for bad in [
            "sh", "bash", "cmd.exe", "bin/sh", "../x", "/bin/ls", "-x", "a b",
        ] {
            let program = if bad.contains('/') {
                hashed(bad, &hash)
            } else {
                path_program(bad, &[])
            };
            assert!(
                matches!(
                    spawning(program).validate(),
                    Err(ManifestError::BadProgram(_))
                ),
                "{bad}"
            );
        }
        assert_eq!(
            spawning(path_program("bin/playit", &[])).validate(),
            Err(ManifestError::ProgramNeedsHash("agent".into()))
        );
        assert!(matches!(
            spawning(hashed("bin/playit", "ABC")).validate(),
            Err(ManifestError::BadProgram(_))
        ));
        assert_eq!(spawning(hashed("bin/playit", &hash)).validate(), Ok(()));
    }

    #[test]
    fn program_arguments_match_literals_one_token_and_a_trailing_rest() {
        let args =
            |tokens: &[&str]| -> Vec<String> { tokens.iter().map(|t| t.to_string()).collect() };
        let status = path_program("tailscale", &["status", "--json"]);
        assert!(status.permits(&args(&["status", "--json"])));
        assert!(!status.permits(&args(&["status"])));
        assert!(!status.permits(&args(&["status", "--json", "x"])));
        assert!(!status.permits(&args(&["up", "--json"])));

        let up = path_program("tailscale", &["up", "--authkey", "*"]);
        assert!(up.permits(&args(&["up", "--authkey", "k"])));
        assert!(!up.permits(&args(&["up", "--authkey"])));

        let install = path_program("gem", &["install", "**"]);
        assert!(install.permits(&args(&["install"])));
        assert!(install.permits(&args(&["install", "a", "b"])));
        assert!(!install.permits(&args(&["uninstall", "a"])));

        assert!(path_program("x", &[]).permits(&[]));
        assert!(!path_program("x", &[]).permits(&args(&["a"])));
        assert_eq!(
            spawning(path_program("x", &["**", "a"])).validate(),
            Err(ManifestError::BadProgram("agent".into()))
        );
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
                    ..Setting::default()
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
