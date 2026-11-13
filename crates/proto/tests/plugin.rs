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
    use std::collections::BTreeMap;

    use proto::error::EntryKind;
    use proto::plugin::{
        CommandArg, CommandArgKind, CommandContribution, CommandScope, Invocation, JavaChoiceError,
        JavaProfile, JavaRange, Manifest, ManifestError, PageContribution, PageSlot, Permission,
        PermissionScope, Program, ProgramJava, ProviderContribution, ScheduleContribution,
        ScopeKind, Setting, SettingKind, ThemeContribution, Tier, MAX_SCHEDULES, MAX_SCHEDULE_SECS,
    };

    fn granting(permissions: &[Permission]) -> BTreeMap<Permission, PermissionScope> {
        permissions
            .iter()
            .map(|permission| (*permission, PermissionScope::default()))
            .collect()
    }

    fn scoped(kind: ScopeKind, entries: &[&str]) -> PermissionScope {
        PermissionScope::of(kind, entries.iter().copied())
    }

    fn with_scope(
        mut manifest: Manifest,
        permission: Permission,
        scope: PermissionScope,
    ) -> Manifest {
        manifest.permissions.insert(permission, scope);
        manifest
    }

    fn reaching(hosts: &[&str]) -> Manifest {
        with_scope(
            valid(),
            Permission::NetworkConnect,
            scoped(ScopeKind::Hosts, hosts),
        )
    }

    /// The first error, in rule order.
    trait Verdict {
        fn verdict(&self) -> Result<(), ManifestError>;
    }

    impl Verdict for Manifest {
        fn verdict(&self) -> Result<(), ManifestError> {
            self.validate()
                .map_err(|errors| errors.into_vec().remove(0))
        }
    }

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
        assert_eq!(theme.verdict(), Ok(()));
        theme.wasm = Some("plugin.wasm".into());
        assert_eq!(theme.verdict(), Err(ManifestError::MissingApi));
    }

    fn hooked(hook: &str, permissions: &[Permission]) -> Manifest {
        let mut manifest = valid();
        manifest.contributes.hooks.push(hook.into());
        manifest.permissions = granting(permissions);
        manifest
    }

    #[test]
    fn a_hook_needs_its_permissions_and_must_be_known() {
        assert_eq!(
            hooked("before-launch", &[Permission::LaunchPatch]).verdict(),
            Err(ManifestError::HookPermissionMissing {
                hook: "before-launch".into(),
                permission: Permission::AccountRead,
            })
        );
        assert_eq!(
            hooked("before-content-install", &[]).verdict(),
            Err(ManifestError::HookPermissionMissing {
                hook: "before-content-install".into(),
                permission: Permission::ContentRead,
            })
        );
        assert_eq!(
            hooked("on-tuesday", &[]).verdict(),
            Err(ManifestError::UnknownHook("on-tuesday".into()))
        );
    }

    #[test]
    fn a_declared_launch_property_must_be_one_no_plugin_is_refused() {
        let declaring = |key: &str| {
            let manifest = with_scope(
                valid(),
                Permission::LaunchPatch,
                scoped(ScopeKind::Properties, &[key]),
            );
            manifest.verdict()
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
            assert_eq!(manifest.verdict(), Ok(()), "{name}");
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
                manifest.verdict(),
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
            manifest.verdict(),
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
            manifest.verdict(),
            Err(ManifestError::ThemeNeedsUiCss("dusk".into()))
        );
        manifest
            .permissions
            .insert(Permission::UiCss, PermissionScope::default());
        assert_eq!(manifest.verdict(), Ok(()));
    }

    fn scheduled(schedules: &[(&str, u64)]) -> Manifest {
        let mut manifest = valid();
        manifest.contributes.schedules = schedules
            .iter()
            .map(|(id, every)| ScheduleContribution {
                id: id.to_string(),
                every: *every,
            })
            .collect();
        manifest
    }

    #[test]
    fn a_schedule_ticks_no_more_often_than_every_thirty_seconds() {
        assert_eq!(scheduled(&[("poll", 30)]).verdict(), Ok(()));
        for every in [0, MAX_SCHEDULE_SECS + 1] {
            assert_eq!(
                scheduled(&[("poll", every)]).verdict(),
                Err(ManifestError::BadScheduleInterval { id: "poll".into() })
            );
        }
    }

    #[test]
    fn schedule_ids_are_slugs_and_unique() {
        assert!(matches!(
            scheduled(&[("poll", 60), ("poll", 90)]).verdict(),
            Err(ManifestError::DuplicateId(_))
        ));
        assert!(matches!(
            scheduled(&[("", 60)]).verdict(),
            Err(ManifestError::BadContributionId(_))
        ));
        assert!(matches!(
            scheduled(&[("Poll", 60)]).verdict(),
            Err(ManifestError::BadContributionId(_))
        ));
    }

    #[test]
    fn a_plugin_declares_a_few_schedules_at_most() {
        let ids: Vec<String> = (0..=MAX_SCHEDULES).map(|n| format!("s{n}")).collect();
        let at = |count: usize| {
            let schedules: Vec<(&str, u64)> =
                ids[..count].iter().map(|id| (id.as_str(), 60)).collect();
            scheduled(&schedules).verdict()
        };
        assert_eq!(at(MAX_SCHEDULES), Ok(()));
        assert_eq!(at(MAX_SCHEDULES + 1), Err(ManifestError::TooManySchedules));
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
        assert_eq!(paged(PageSlot::Nav, None, None).verdict(), Ok(()));
        assert_eq!(
            paged(PageSlot::Entry, None, None).verdict(),
            Err(ManifestError::PageNeedsKind("overview".into()))
        );
        assert_eq!(
            paged(PageSlot::Entry, Some(EntryKind::Server), None).verdict(),
            Ok(())
        );
    }

    #[test]
    fn a_page_icon_is_a_name_or_a_path_inside_the_package() {
        assert_eq!(paged(PageSlot::Nav, None, Some("chart")).verdict(), Ok(()));
        assert_eq!(
            paged(PageSlot::Nav, None, Some("assets/icon.svg")).verdict(),
            Ok(())
        );
        for icon in ["../icon.svg", "/icon.svg", "a/../../icon.png"] {
            assert_eq!(
                paged(PageSlot::Nav, None, Some(icon)).verdict(),
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
            assert_eq!(manifest.verdict(), Err(expected));
        }
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "content".into(),
            id: "Not A Slug".into(),
        });
        assert!(matches!(
            manifest.verdict(),
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
            manifest.verdict(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "index".into(),
                permission: Permission::ContentRead
            })
        );
        manifest.permissions = granting(&[Permission::ContentRead, Permission::NetworkConnect]);
        assert_eq!(
            manifest.verdict(),
            Err(ManifestError::ScopeRequired {
                permission: Permission::NetworkConnect,
                kind: ScopeKind::Hosts
            })
        );
        manifest.permissions.insert(
            Permission::NetworkConnect,
            scoped(ScopeKind::Hosts, &["index.example.com"]),
        );
        assert_eq!(manifest.verdict(), Ok(()));
        manifest.contributes.providers[0].kind = "mystery".into();
        assert_eq!(
            manifest.verdict(),
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
        manifest.permissions = granting(&[Permission::ContentRead, Permission::NetworkConnect]);
        manifest.permissions.insert(
            Permission::NetworkConnect,
            scoped(ScopeKind::Hosts, &["meta.quiltmc.org"]),
        );
        assert_eq!(
            manifest.verdict(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "quilt".into(),
                permission: Permission::FlavorProvide
            })
        );
        manifest
            .permissions
            .insert(Permission::FlavorProvide, PermissionScope::default());
        assert_eq!(manifest.verdict(), Ok(()));
        assert_eq!(Permission::FlavorProvide.tier(), Tier::Unrestricted);
    }

    #[test]
    fn a_format_needs_format_provide_and_no_hosts() {
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "format".into(),
            id: "prism".into(),
        });
        manifest.permissions = granting(&[Permission::ContentRead]);
        assert_eq!(
            manifest.verdict(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "prism".into(),
                permission: Permission::FormatProvide
            })
        );
        manifest.permissions = granting(&[Permission::FormatProvide]);
        assert_eq!(manifest.verdict(), Ok(()));
        assert_eq!(Permission::FormatProvide.tier(), Tier::Unrestricted);
    }

    #[test]
    fn a_java_provider_needs_java_provide_and_network_hosts() {
        let mut manifest = valid();
        manifest.contributes.providers.push(ProviderContribution {
            kind: "java".into(),
            id: "zulu".into(),
        });
        manifest.permissions = granting(&[Permission::FlavorProvide, Permission::NetworkConnect]);
        manifest.permissions.insert(
            Permission::NetworkConnect,
            scoped(ScopeKind::Hosts, &["api.azul.com"]),
        );
        assert_eq!(
            manifest.verdict(),
            Err(ManifestError::ProviderPermissionMissing {
                provider: "zulu".into(),
                permission: Permission::JavaProvide
            })
        );
        manifest
            .permissions
            .insert(Permission::JavaProvide, PermissionScope::default());
        assert_eq!(manifest.verdict(), Ok(()));
        assert_eq!(Permission::JavaProvide.tier(), Tier::Unrestricted);
    }

    #[test]
    fn event_patterns_are_a_topic_or_a_trailing_glob() {
        let mut manifest = valid();
        manifest.contributes.events = vec!["server.*".into(), "job".into()];
        assert_eq!(manifest.verdict(), Ok(()));
        for bad in ["*", "server*", "a.*.b", ""] {
            manifest.contributes.events = vec![bad.into()];
            assert_eq!(
                manifest.verdict(),
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
            manifest.verdict(),
            Err(ManifestError::DuplicateId("token".into()))
        );
    }

    #[test]
    fn a_wildcard_over_a_public_suffix_is_refused_by_name() {
        for good in [
            "*.example.com",
            "*.example.co.uk",
            "*.user.github.io",
            "github.io",
        ] {
            let manifest = reaching(&[good]);
            assert_eq!(manifest.verdict(), Ok(()), "{good}");
        }
        for bad in ["*.co.uk", "*.github.io", "*.CO.UK.", "*.s3.amazonaws.com"] {
            let manifest = reaching(&[bad]);
            assert_eq!(
                manifest.verdict(),
                Err(ManifestError::PublicSuffixWildcard(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_wildcard_reaches_one_label_down_and_no_further() {
        use proto::plugin::network_matches;
        assert!(network_matches("*.example.com", "api.example.com"));
        assert!(network_matches("*.Example.com.", "api.example.com"));
        assert!(network_matches("example.com", "example.com"));
        for outside in [
            "example.com",
            "a.b.example.com",
            "evilexample.com",
            ".example.com",
            "api.example.com.evil.net",
        ] {
            assert!(!network_matches("*.example.com", outside), "{outside}");
        }
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
            let manifest = reaching(&[good]);
            assert_eq!(manifest.verdict(), Ok(()), "{good}");
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
            let manifest = reaching(&[bad]);
            assert_eq!(
                manifest.verdict(),
                Err(ManifestError::BadNetworkHost(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn an_unhashed_host_must_be_a_network_host() {
        let mut network = scoped(ScopeKind::Hosts, &["meta.example.com"]);
        network.extend(ScopeKind::Unhashed, ["other.example.com"]);
        let manifest = with_scope(valid(), Permission::NetworkConnect, network);
        assert_eq!(
            manifest.verdict(),
            Err(ManifestError::BadUnhashedHost("other.example.com".into()))
        );
        let mut network = scoped(ScopeKind::Hosts, &["meta.example.com"]);
        network.extend(ScopeKind::Unhashed, ["meta.example.com"]);
        let declared = with_scope(valid(), Permission::NetworkConnect, network);
        assert_eq!(declared.verdict(), Ok(()));
        assert!(declared
            .grant()
            .unhashed()
            .any(|host| host == "meta.example.com"));
    }

    fn permits(program: &Program, args: &[String]) -> bool {
        program.matching(args, &[]).is_ok()
    }

    fn spawning(program: Program) -> Manifest {
        let mut manifest = with_scope(
            valid(),
            Permission::ProcessSpawn,
            scoped(ScopeKind::Programs, &["agent"]),
        );
        manifest.programs = vec![Program {
            id: "agent".into(),
            ..program
        }];
        manifest
    }

    fn path_program(command: &str, args: &[&str]) -> Program {
        Program {
            command: command.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            ..Program::default()
        }
    }

    #[test]
    fn spawning_names_only_programs_the_manifest_defines() {
        let mut manifest = spawning(path_program("tailscale", &[]));
        assert_eq!(manifest.verdict(), Ok(()));
        manifest.programs.clear();
        assert_eq!(
            manifest.verdict(),
            Err(ManifestError::ProgramNotDefined("agent".into()))
        );
        let mut empty = valid();
        empty
            .permissions
            .insert(Permission::ProcessSpawn, PermissionScope::default());
        assert_eq!(
            empty.verdict(),
            Err(ManifestError::ScopeRequired {
                permission: Permission::ProcessSpawn,
                kind: ScopeKind::Programs
            })
        );
    }

    #[test]
    fn a_program_path_names_other_path_programs_once() {
        let git = Program {
            id: "git".into(),
            ..path_program("git", &[])
        };
        let building = |path: &[&str], extra: Vec<Program>| {
            let mut manifest = spawning(Program {
                path: path.iter().map(|id| id.to_string()).collect(),
                ..path_program("agent", &[])
            });
            manifest.programs.extend(extra);
            manifest.verdict()
        };
        assert_eq!(building(&["git"], vec![git.clone()]), Ok(()));
        for (path, extra) in [
            (vec!["git"], vec![]),
            (vec!["agent"], vec![]),
            (vec!["git", "git"], vec![git.clone()]),
            (
                vec!["jvm"],
                vec![Program {
                    id: "jvm".into(),
                    command: String::new(),
                    java: Some(ProgramJava::Major(21)),
                    ..Program::default()
                }],
            ),
        ] {
            assert!(matches!(
                building(&path, extra),
                Err(ManifestError::BadProgramPath { .. })
            ));
        }
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
                manifest.verdict(),
                Err(ManifestError::BadProgramEnv(name.into())),
                "{name}"
            );
        }
        let allowed = spawning(Program {
            env: ["TS_AUTHKEY".to_string(), "JAVA_HOME".to_string()].into(),
            ..path_program("tailscale", &[])
        });
        assert_eq!(allowed.verdict(), Ok(()));
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
                    spawning(program).verdict(),
                    Err(ManifestError::BadProgram(_))
                ),
                "{bad}"
            );
        }
        assert_eq!(
            spawning(path_program("bin/playit", &[])).verdict(),
            Err(ManifestError::ProgramNeedsHash("agent".into()))
        );
        assert!(matches!(
            spawning(hashed("bin/playit", "ABC")).verdict(),
            Err(ManifestError::BadProgram(_))
        ));
        assert_eq!(spawning(hashed("bin/playit", &hash)).verdict(), Ok(()));
    }

    #[test]
    fn a_program_names_exactly_one_of_a_command_and_a_managed_java() {
        let java = |major: u32| Program {
            java: Some(ProgramJava::Major(major)),
            args: vec!["-jar".into(), "*".into()],
            ..Program::default()
        };
        assert_eq!(spawning(java(21)).verdict(), Ok(()));
        assert_eq!(java(21).identity_label(), "Hestia-managed Java 21");

        let both = Program {
            command: "java".into(),
            ..java(21)
        };
        let neither = Program::default();
        let hashed = Program {
            sha256: [("x86_64-unknown-linux-gnu".to_string(), "0".repeat(64))].into(),
            ..java(21)
        };
        for program in [both, neither] {
            assert_eq!(
                spawning(program).verdict(),
                Err(ManifestError::ProgramIdentity("agent".into()))
            );
        }
        for program in [java(0), java(100)] {
            assert_eq!(
                spawning(program).verdict(),
                Err(ManifestError::BadProgramJava("agent".into()))
            );
        }
        assert_eq!(
            spawning(hashed).verdict(),
            Err(ManifestError::BadProgram("agent".into()))
        );
        let shell_arg = Program {
            args: vec!["**".into(), "a".into()],
            ..java(21)
        };
        assert_eq!(
            spawning(shell_arg).verdict(),
            Err(ManifestError::BadProgram("agent".into()))
        );
    }

    #[test]
    fn program_arguments_match_literals_one_token_and_a_trailing_rest() {
        let args =
            |tokens: &[&str]| -> Vec<String> { tokens.iter().map(|t| t.to_string()).collect() };
        let status = path_program("tailscale", &["status", "--json"]);
        assert!(permits(&status, &args(&["status", "--json"])));
        assert!(!permits(&status, &args(&["status"])));
        assert!(!permits(&status, &args(&["status", "--json", "x"])));
        assert!(!permits(&status, &args(&["up", "--json"])));

        let up = path_program("tailscale", &["up", "--authkey", "*"]);
        assert!(permits(&up, &args(&["up", "--authkey", "k"])));
        assert!(!permits(&up, &args(&["up", "--authkey"])));

        let install = path_program("gem", &["install", "**"]);
        assert!(permits(&install, &args(&["install"])));
        assert!(permits(&install, &args(&["install", "a", "b"])));
        assert!(!permits(&install, &args(&["uninstall", "a"])));

        assert!(permits(&path_program("x", &[]), &[]));
        assert!(!permits(&path_program("x", &[]), &args(&["a"])));
        assert_eq!(
            spawning(path_program("x", &["**", "a"])).verdict(),
            Err(ManifestError::BadProgram("agent".into()))
        );
    }

    fn invocation(name: &str, args: &[&str]) -> Invocation {
        Invocation {
            name: name.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            ..Invocation::default()
        }
    }

    fn buildtools(invocations: Vec<Invocation>) -> Program {
        Program {
            command: String::new(),
            java: Some(ProgramJava::Range(JavaRange { min: 17, max: 21 })),
            invocations,
            ..Program::default()
        }
    }

    #[test]
    fn a_program_matches_the_invocation_a_call_fits() {
        let args =
            |tokens: &[&str]| -> Vec<String> { tokens.iter().map(|t| t.to_string()).collect() };
        let program = buildtools(vec![
            invocation("build", &["-jar", "${data}/bt.jar", "--rev", "*"]),
            Invocation {
                env: ["TOKEN".to_string()].into(),
                ..invocation("publish", &["-jar", "${data}/bt.jar", "publish"])
            },
        ]);
        assert_eq!(
            program
                .matching(&args(&["-jar", "${data}/bt.jar", "--rev", "1.21"]), &[])
                .unwrap()
                .name,
            "build"
        );
        assert_eq!(
            program
                .matching(&args(&["-jar", "${data}/bt.jar", "publish"]), &["TOKEN"])
                .unwrap()
                .name,
            "publish"
        );
        assert_eq!(
            program.matching(&args(&["-jar", "${data}/bt.jar", "publish"]), &["OTHER"]),
            Err(proto::plugin::InvocationMismatch::Env("OTHER".into()))
        );
        assert_eq!(
            program.matching(&args(&["-jar", "x.jar"]), &[]),
            Err(proto::plugin::InvocationMismatch::Args)
        );
        assert_eq!(
            program.matching(&args(&["-jar", "${data}/bt.jar", "--rev", "1", "x"]), &[]),
            Err(proto::plugin::InvocationMismatch::Args)
        );
    }

    #[test]
    fn invocations_are_named_once_and_never_mixed_with_top_level_args() {
        assert_eq!(
            spawning(buildtools(vec![
                invocation("build", &["a"]),
                invocation("run", &["b"])
            ]))
            .verdict(),
            Ok(())
        );
        let invalid = |program: Program| {
            matches!(
                spawning(program).verdict(),
                Err(ManifestError::BadProgramInvocations(_))
            )
        };
        assert!(invalid(buildtools(vec![
            invocation("build", &["a"]),
            invocation("build", &["b"])
        ])));
        assert!(invalid(buildtools(vec![invocation("", &["a"])])));
        assert!(invalid(buildtools(vec![invocation("Not A Slug", &["a"])])));
        assert!(invalid(Program {
            args: vec!["top".into()],
            ..buildtools(vec![invocation("build", &["a"])])
        }));
        assert_eq!(
            spawning(buildtools(vec![invocation("build", &["**", "x"])])).verdict(),
            Err(ManifestError::BadProgram("agent".into()))
        );
        assert_eq!(
            spawning(buildtools(vec![Invocation {
                env: ["LD_PRELOAD".to_string()].into(),
                ..invocation("build", &[])
            }]))
            .verdict(),
            Err(ManifestError::BadProgramEnv("LD_PRELOAD".into()))
        );
    }

    #[test]
    fn a_java_program_declares_a_major_a_range_or_the_game_profile() {
        let runtime = |json: &str| serde_json::from_str::<ProgramJava>(json).unwrap();
        assert_eq!(runtime("21"), ProgramJava::Major(21));
        assert_eq!(
            runtime(r#"{"min":17,"max":21}"#),
            ProgramJava::Range(JavaRange { min: 17, max: 21 })
        );
        assert_eq!(
            runtime(r#""profile""#),
            ProgramJava::Profile(JavaProfile::Profile)
        );
        assert!(serde_json::from_str::<ProgramJava>(r#""latest""#).is_err());

        for bad in [
            JavaRange { min: 21, max: 17 },
            JavaRange { min: 0, max: 21 },
            JavaRange { min: 17, max: 100 },
        ] {
            assert_eq!(
                spawning(Program {
                    java: Some(ProgramJava::Range(bad)),
                    command: String::new(),
                    ..Program::default()
                })
                .verdict(),
                Err(ManifestError::BadProgramJava("agent".into()))
            );
        }
        let profile = Program {
            java: Some(ProgramJava::Profile(JavaProfile::Profile)),
            command: String::new(),
            ..Program::default()
        };
        assert_eq!(spawning(profile).verdict(), Ok(()));
    }

    #[test]
    fn a_call_chooses_a_java_major_only_inside_what_the_program_declares() {
        let range = ProgramJava::Range(JavaRange { min: 17, max: 21 });
        assert_eq!(range.choose(Some(17), None), Ok(17));
        assert_eq!(range.choose(Some(21), None), Ok(21));
        assert!(matches!(
            range.choose(Some(22), None),
            Err(JavaChoiceError::OutsideRange { asked: 22, .. })
        ));
        assert!(matches!(
            range.choose(Some(8), Some(17)),
            Err(JavaChoiceError::OutsideRange { asked: 8, .. })
        ));
        assert!(matches!(
            range.choose(None, Some(21)),
            Err(JavaChoiceError::NeedsMajor { .. })
        ));

        let fixed = ProgramJava::Major(21);
        assert_eq!(fixed.choose(None, None), Ok(21));
        assert_eq!(fixed.choose(Some(21), None), Ok(21));
        assert!(fixed.choose(Some(17), None).is_err());

        let profile = ProgramJava::Profile(JavaProfile::Profile);
        assert_eq!(profile.choose(None, Some(21)), Ok(21));
        assert_eq!(
            profile.choose(None, None),
            Err(JavaChoiceError::NoGameVersion)
        );
        assert_eq!(
            profile.choose(Some(21), Some(21)),
            Err(JavaChoiceError::ProfileTakesNone)
        );
    }

    #[test]
    fn a_requirement_names_the_program_and_where_to_get_it() {
        let requiring = |name: &str, url: &str| Program {
            requires: Some(proto::minecraft::Requirement {
                name: name.into(),
                url: url.into(),
            }),
            ..path_program("git", &[])
        };
        assert_eq!(
            spawning(requiring("Git", "https://git-scm.com/downloads")).verdict(),
            Ok(())
        );
        for (name, url) in [
            ("", "https://git-scm.com"),
            ("Git", "http://git-scm.com"),
            ("Git", "git-scm.com"),
            ("Git", "https://git scm.com"),
        ] {
            assert_eq!(
                spawning(requiring(name, url)).verdict(),
                Err(ManifestError::BadProgramRequires("agent".into())),
                "{name} {url}"
            );
        }
    }

    #[test]
    fn every_error_of_a_program_is_collected() {
        let mut manifest = spawning(Program {
            command: "sh".into(),
            java: Some(ProgramJava::Major(0)),
            env: ["PATH".to_string()].into(),
            invocations: vec![invocation("a", &[])],
            ..Program::default()
        });
        manifest.programs[0].requires = Some(proto::minecraft::Requirement::default());
        let errors = manifest.validate().unwrap_err().into_vec();
        for expected in [
            ManifestError::ProgramIdentity("agent".into()),
            ManifestError::BadProgramJava("agent".into()),
            ManifestError::BadProgramInvocations("agent".into()),
            ManifestError::BadProgramRequires("agent".into()),
        ] {
            assert!(errors.contains(&expected), "{expected:?} in {errors:?}");
        }
    }

    #[test]
    fn a_changed_identity_or_invocation_changes_the_entry() {
        let base = buildtools(vec![invocation("build", &["a"])]);
        let entry = Program {
            id: "bt".into(),
            ..base.clone()
        }
        .entry();
        for changed in [
            Program {
                java: Some(ProgramJava::Range(JavaRange { min: 17, max: 25 })),
                ..base.clone()
            },
            Program {
                java: Some(ProgramJava::Major(17)),
                ..base.clone()
            },
            Program {
                invocations: vec![invocation("build", &["a", "**"])],
                ..base.clone()
            },
            Program {
                invocations: vec![invocation("make", &["a"])],
                ..base.clone()
            },
            Program {
                invocations: vec![invocation("build", &["a"]), invocation("run", &["b"])],
                ..base.clone()
            },
            Program {
                invocations: vec![Invocation {
                    env: ["X".to_string()].into(),
                    ..invocation("build", &["a"])
                }],
                ..base.clone()
            },
        ] {
            let changed = Program {
                id: "bt".into(),
                ..changed
            };
            assert_ne!(changed.entry(), entry, "{changed:?}");
        }
        assert_eq!(
            entry,
            r#"bt: Hestia-managed Java 17-21 | build: args ["a"]"#
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
            command(CommandScope::Top, &["name", "port"]).verdict(),
            Ok(())
        );
        assert_eq!(command(CommandScope::Top, &["entry"]).verdict(), Ok(()));
        assert!(matches!(
            command(CommandScope::Top, &["Bad Name"]).verdict(),
            Err(ManifestError::BadContributionId(_))
        ));
        assert_eq!(
            command(CommandScope::Top, &["name", "name"]).verdict(),
            Err(ManifestError::DuplicateId("name".into()))
        );
        assert_eq!(
            command(CommandScope::Server, &["entry"]).verdict(),
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
                manifest.verdict(),
                Err(ManifestError::ReservedCommand(verb.into()))
            );
        }
        let mut manifest = command(CommandScope::Top, &[]);
        manifest.contributes.commands[0].id = "backup".into();
        assert_eq!(manifest.verdict(), Ok(()));
    }

    #[test]
    fn a_command_id_is_a_few_slugs_and_unique_as_a_whole() {
        let with_id = |id: &str| {
            let mut manifest = command(CommandScope::Server, &[]);
            manifest.contributes.commands[0].id = id.into();
            manifest
        };
        assert_eq!(with_id("lists show").verdict(), Ok(()));
        for bad in ["", "a  b", " a", "a ", "a b c d", "Bad word", "a/b"] {
            assert_eq!(
                with_id(bad).verdict(),
                Err(ManifestError::BadCommandId(bad.into())),
                "{bad:?}"
            );
        }
        assert_eq!(
            with_id("backup now").verdict(),
            Err(ManifestError::ReservedCommand("backup now".into()))
        );
        let mut twice = with_id("lists show");
        twice.contributes.commands.push(CommandContribution {
            id: "lists show".into(),
            scope: CommandScope::Server,
            ..CommandContribution::default()
        });
        assert_eq!(
            twice.verdict(),
            Err(ManifestError::DuplicateId("lists show".into()))
        );
        twice.contributes.commands[1].id = "lists".into();
        assert_eq!(twice.verdict(), Ok(()));
    }

    #[test]
    fn a_words_argument_is_alone_and_last() {
        let words = |name: &str| CommandArg {
            name: name.into(),
            kind: CommandArgKind::Words,
            ..CommandArg::default()
        };
        let plain = |name: &str| CommandArg {
            name: name.into(),
            ..CommandArg::default()
        };
        let with_args = |args: Vec<CommandArg>| {
            let mut manifest = command(CommandScope::Top, &[]);
            manifest.contributes.commands[0].args = args;
            manifest
        };
        assert_eq!(
            with_args(vec![plain("a"), words("names")]).verdict(),
            Ok(())
        );
        for args in [
            vec![words("names"), plain("a")],
            vec![words("a"), words("b")],
        ] {
            assert_eq!(
                with_args(args).verdict(),
                Err(ManifestError::BadWordsArg("open".into()))
            );
        }
    }

    #[test]
    fn a_plugin_names_the_entry_files_it_reads() {
        let reading = |files: &[&str]| {
            with_scope(
                valid(),
                Permission::EntryRead,
                scoped(ScopeKind::Files, files),
            )
        };
        assert_eq!(
            reading(&["whitelist.json", "config/a.json"]).verdict(),
            Ok(())
        );
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "./a",
            "a//b",
            "a\\b",
            "C:x",
            "*.json",
            "a?",
            "server.properties",
            "Server.Properties",
        ] {
            assert_eq!(
                reading(&[bad]).verdict(),
                Err(ManifestError::BadEntryFile(bad.into())),
                "{bad:?}"
            );
        }
        assert_eq!(
            with_scope(
                valid(),
                Permission::ContentRead,
                scoped(ScopeKind::Files, &["ops.json"])
            )
            .verdict(),
            Err(ManifestError::ScopeNotAccepted {
                permission: Permission::ContentRead,
                kind: ScopeKind::Files
            })
        );
    }

    #[test]
    fn a_new_entry_file_widens_the_grant() {
        let reading = |files: &[&str]| {
            with_scope(
                valid(),
                Permission::EntryRead,
                scoped(ScopeKind::Files, files),
            )
            .grant()
        };
        let installed = reading(&["ops.json"]);
        let wider = reading(&["ops.json", "whitelist.json"]);
        assert_eq!(
            wider.widening_over(&installed).permissions,
            [(
                Permission::EntryRead,
                scoped(ScopeKind::Files, &["whitelist.json"])
            )]
            .into()
        );
        assert!(!installed.widens(&wider));
        assert_ne!(
            wider.approval_digest(&Default::default()),
            installed.approval_digest(&Default::default())
        );
    }

    #[test]
    fn api_must_share_the_host_major_and_not_be_newer() {
        for api in ["^2.0.0", "^1.1.0", "^1.0.1"] {
            let mut manifest = valid();
            manifest.api = Some(api.into());
            assert!(
                matches!(manifest.verdict(), Err(ManifestError::ApiMismatch { .. })),
                "{api}"
            );
        }
        let mut manifest = valid();
        manifest.api = Some("1.0.0".into());
        assert!(matches!(manifest.verdict(), Err(ManifestError::BadApi(_))));
    }

    #[test]
    fn independent_mistakes_are_all_reported() {
        let manifest = Manifest {
            id: "Bad Id".into(),
            version: "one".into(),
            api: None,
            permissions: [
                (
                    Permission::NetworkConnect,
                    scoped(ScopeKind::Hosts, &["10.0.0.1"]),
                ),
                (
                    Permission::EntryRead,
                    scoped(ScopeKind::Files, &["../ops.json"]),
                ),
                (Permission::ContentRead, scoped(ScopeKind::Files, &["a"])),
                (Permission::IpcConnect, PermissionScope::default()),
            ]
            .into(),
            contributes: proto::plugin::Contributions {
                hooks: vec!["on-tuesday".into()],
                ..Default::default()
            },
            ..valid()
        };

        let errors = manifest.validate().unwrap_err();

        for expected in [
            ManifestError::BadId("Bad Id".into()),
            ManifestError::BadVersion("one".into()),
            ManifestError::MissingApi,
            ManifestError::BadNetworkHost("10.0.0.1".into()),
            ManifestError::BadEntryFile("../ops.json".into()),
            ManifestError::ScopeNotAccepted {
                permission: Permission::ContentRead,
                kind: ScopeKind::Files,
            },
            ManifestError::ScopeRequired {
                permission: Permission::IpcConnect,
                kind: ScopeKind::Endpoints,
            },
            ManifestError::UnknownHook("on-tuesday".into()),
        ] {
            assert!(
                errors.contains(&expected),
                "{expected:?} missing from {errors}"
            );
        }
        assert_eq!(errors.to_string().lines().count(), errors.iter().count());
    }

    #[test]
    fn widening_a_manifest_changes_its_grant() {
        let base = valid();
        let spawns = |program: Program| {
            let mut manifest = with_scope(
                valid(),
                Permission::ProcessSpawn,
                scoped(ScopeKind::Programs, &["ts"]),
            );
            manifest.programs = vec![Program {
                id: "ts".into(),
                command: "tailscale".into(),
                ..program
            }];
            manifest
        };
        let widened: Vec<Manifest> = vec![
            Manifest {
                permissions: granting(&[Permission::NetworkConnect]),
                ..base.clone()
            },
            reaching(&["example.com"]),
            spawns(Program::default()),
            spawns(Program {
                env: ["TOKEN".to_string()].into(),
                ..Program::default()
            }),
            spawns(Program {
                args: vec!["status".into()],
                ..Program::default()
            }),
            spawns(Program {
                sha256: [("x86_64-unknown-linux-gnu".to_string(), "0".repeat(64))].into(),
                ..Program::default()
            }),
            spawns(Program {
                path: vec!["git".into()],
                ..Program::default()
            }),
            spawns(Program {
                command: String::new(),
                java: Some(ProgramJava::Major(21)),
                ..Program::default()
            }),
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
            .map(|manifest| manifest.grant().approval_digest(&Default::default()))
            .collect();
        for (index, digest) in digests.iter().enumerate() {
            assert_ne!(*digest, base.grant().approval_digest(&Default::default()));
            assert!(!digests[..index].contains(digest));
        }
    }
}

#[test]
fn a_build_is_judged_against_the_host_it_will_run_on() {
    use proto::plugin::{api_supported_by, is_api_version};

    assert!(api_supported_by("^1.0.0", "1.2.0"));
    assert!(!api_supported_by("^1.2.0", "1.0.0"));
    assert!(!api_supported_by("^1.0.0", "2.0.0"));
    assert!(api_supported_by("^2.0.0", "2.0.0"));
    assert!(!api_supported_by("^1.0.0", "next"));
    assert!(is_api_version("2.0.0") && !is_api_version("next"));
}
