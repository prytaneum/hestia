use proto::error::{EntryKind, Field, Reason};
use proto::provider::Provider;

use super::*;

fn provider() -> Service {
    Service::Provider(Provider::new("probe.reach", "Probe Net"))
}

fn down(info: ErrorInfo) -> GuestError {
    GuestError::from(&info)
}

#[test]
fn a_daemon_failure_reaches_the_plugin_as_the_case_it_is() {
    let not_found = down(ErrorInfo::EntryNotFound {
        entry: EntryKind::Server,
        reference: "smp".into(),
    });
    assert!(matches!(not_found, GuestError::NotFound(message) if message.contains("smp")));

    assert!(matches!(
        down(ErrorInfo::UnknownChannel {
            channel: "x.y".into()
        }),
        GuestError::NotFound(_)
    ));
    assert!(matches!(
        down(ErrorInfo::DeveloperModeOff),
        GuestError::Denied(_)
    ));
    assert!(matches!(
        down(ErrorInfo::Offline {
            service: Some(provider())
        }),
        GuestError::Offline(Some(detail)) if detail.contains("Probe Net")
    ));
    assert!(matches!(
        down(ErrorInfo::OfflineMode),
        GuestError::Offline(Some(_))
    ));
    assert_eq!(down(ErrorInfo::SignInRequired), GuestError::SignInRequired);
    assert_eq!(
        down(ErrorInfo::SessionExpired {
            reference: "a".into()
        }),
        GuestError::SignInRequired
    );
    assert!(matches!(
        down(ErrorInfo::EntryRunning {
            entry: EntryKind::Server,
            name: "smp".into()
        }),
        GuestError::Busy(_)
    ));
    assert!(matches!(
        down(ErrorInfo::BackupInProgress { name: "smp".into() }),
        GuestError::Busy(_)
    ));
    assert!(matches!(
        down(ErrorInfo::FieldRequired { field: Field::Name }),
        GuestError::Invalid(_)
    ));
    assert!(matches!(
        down(ErrorInfo::InvalidValue {
            field: Field::Memory,
            reason: Reason::MemoryFormat
        }),
        GuestError::Invalid(_)
    ));
    assert!(matches!(
        down(ErrorInfo::UnsupportedOperation {
            reason: proto::error::Unsupported::WorldsForDatapacksOnly
        }),
        GuestError::Unsupported(_)
    ));
    assert_eq!(
        down(ErrorInfo::ConfigKeyUnset {
            key: "content.curseforge-key".into()
        }),
        GuestError::NotConfigured("content.curseforge-key".into())
    );
    assert_eq!(
        down(ErrorInfo::MissingRequirement {
            flavor: "spigot".into(),
            name: "git".into(),
            url: "https://git-scm.com".into()
        }),
        GuestError::MissingRequirement {
            name: "git".into(),
            url: Some("https://git-scm.com".into())
        }
    );
    assert!(matches!(
        down(ErrorInfo::Upstream {
            service: provider(),
            detail: "502".into()
        }),
        GuestError::Upstream { status: None, .. }
    ));
}

#[test]
fn a_daemon_failure_with_no_closer_case_is_failed_with_its_message() {
    let info = ErrorInfo::Internal {
        detail: "the disk is full".into(),
    };
    assert_eq!(down(info.clone()), GuestError::Failed(info.to_string()));
    assert!(matches!(
        down(ErrorInfo::Io {
            operation: proto::error::IoOp::Write,
            detail: "denied".into()
        }),
        GuestError::Failed(_)
    ));
}

#[test]
fn a_provider_failure_is_attributed_to_the_provider() {
    let offline = GuestError::Offline(Some("no route".into())).into_info("probe", Some(provider()));
    assert_eq!(
        offline,
        ErrorInfo::Offline {
            service: Some(provider())
        }
    );

    let upstream = GuestError::Upstream {
        status: Some(503),
        retry_after: Some(30),
        message: "the service is down".into(),
    }
    .into_info("probe", Some(provider()));
    assert_eq!(
        upstream,
        ErrorInfo::Upstream {
            service: provider(),
            detail: "the service is down (retry in 30s)".into()
        }
    );

    let limited = GuestError::RateLimited(Some(7)).into_info("probe", Some(provider()));
    let ErrorInfo::Upstream { service, detail } = limited else {
        panic!("{limited:?}");
    };
    assert_eq!(service, provider());
    assert!(detail.contains("rate limiting") && detail.contains("7s"));
}

#[test]
fn a_failure_with_no_provider_to_blame_names_the_plugin() {
    assert_eq!(
        GuestError::Offline(None).into_info("probe", None),
        ErrorInfo::Offline { service: None }
    );
    let ErrorInfo::PluginCallFailed { id, detail } = (GuestError::Upstream {
        status: Some(500),
        retry_after: None,
        message: "boom".into(),
    })
    .into_info("probe", None) else {
        panic!("an upstream failure of no provider is the plugin's");
    };
    assert_eq!((id.as_str(), detail.as_str()), ("probe", "boom"));
}

#[test]
fn a_user_can_act_on_what_the_plugin_reports() {
    assert_eq!(
        GuestError::SignInRequired.into_info("probe", None),
        ErrorInfo::PluginSignInRequired {
            plugin: "probe".into()
        }
    );
    assert_eq!(
        GuestError::NotConfigured("api-key".into()).into_info("probe", None),
        ErrorInfo::PluginSettingInvalid {
            plugin: "probe".into(),
            key: "api-key".into(),
            detail: "it has no value".into()
        }
    );
    let ErrorInfo::PluginCallFailed { detail, .. } = (GuestError::MissingRequirement {
        name: "git".into(),
        url: Some("https://git-scm.com".into()),
    })
    .into_info("probe", None) else {
        panic!("a missing requirement is reported as the plugin's failure");
    };
    assert!(detail.contains("git") && detail.contains("https://git-scm.com"));
}

#[test]
fn every_case_survives_the_trip_through_the_wire_type() {
    let cases = [
        GuestError::NotFound("a".into()),
        GuestError::Invalid("b".into()),
        GuestError::Denied("c".into()),
        GuestError::Unsupported("d".into()),
        GuestError::Offline(None),
        GuestError::Offline(Some("e".into())),
        GuestError::Upstream {
            status: Some(404),
            retry_after: Some(3),
            message: "f".into(),
        },
        GuestError::Upstream {
            status: None,
            retry_after: None,
            message: String::new(),
        },
        GuestError::RateLimited(None),
        GuestError::RateLimited(Some(9)),
        GuestError::SignInRequired,
        GuestError::NotConfigured("g".into()),
        GuestError::MissingRequirement {
            name: "h".into(),
            url: None,
        },
        GuestError::Busy("i".into()),
        GuestError::Failed("j".into()),
    ];
    for case in cases {
        let wire = PluginError::from(case.clone());
        assert_eq!(GuestError::from(wire), case);
    }
}

#[test]
fn a_program_that_is_not_installed_is_a_missing_requirement_with_its_name_and_url() {
    assert_eq!(
        GuestError::from(ProgramError::Missing {
            name: "Git".into(),
            url: Some("https://git-scm.com/downloads".into()),
        }),
        GuestError::MissingRequirement {
            name: "Git".into(),
            url: Some("https://git-scm.com/downloads".into())
        }
    );
    assert!(matches!(
        GuestError::from(ProgramError::Unapproved("git".into())),
        GuestError::Denied(message) if message.contains("approve")
    ));
    assert!(matches!(
        GuestError::from(ProgramError::Denied("no".into())),
        GuestError::Denied(_)
    ));
    assert!(matches!(
        GuestError::from(IpcError::NotFound("no".into())),
        GuestError::NotFound(_)
    ));
}
