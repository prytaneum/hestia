//! The mock host denies what the grant lacks, in permission and in scope.

use hestia::mock::{self, Program, Scope};
use hestia::{download, ipc, programs, secrets, Error};

fn denied(result: Result<impl std::fmt::Debug, Error>, permission: &str) {
    match result {
        Err(Error::Denied(named)) => assert_eq!(named, permission),
        other => panic!("expected denied({permission}), got {other:?}"),
    }
}

#[test]
fn a_call_the_grant_lacks_answers_denied_in_a_unit_test() {
    denied(secrets::get("token"), "secrets");
    mock::grant(&["secrets"], Scope::default());
    secrets::set("token", "t").unwrap();
    assert_eq!(secrets::get("token").unwrap().as_deref(), Some("t"));
    assert!(mock::calls().contains(&"hestia:kernel/secrets#get".to_owned()));
}

#[test]
fn a_scope_miss_is_denied_as_on_the_host() {
    let sum = "a".repeat(64);
    mock::grant(
        &["network:connect", "process:spawn", "ipc:connect"],
        Scope {
            hosts: vec!["example.com".into(), "*.cdn.example.org".into()],
            unhashed: vec!["example.com".into()],
            programs: vec![Program {
                path: "/bin/tool".into(),
                args: vec!["--ok".into()],
                os: None,
            }],
            endpoints: vec!["/run/svc.sock".into()],
        },
    );

    assert_eq!(
        download::fetch("https://example.com/a", "a.bin", None).unwrap(),
        0
    );
    assert!(download::fetch("https://x.cdn.example.org/a", "a.bin", Some(&sum)).is_ok());
    for (url, sha) in [
        ("https://other.org/a", Some(sum.as_str())),
        ("https://a.b.cdn.example.org/a", Some(sum.as_str())),
        ("https://x.cdn.example.org/a", None),
        ("http://example.com/a", None),
        ("https://user@example.com/a", None),
    ] {
        denied(download::fetch(url, "a.bin", sha), "network:connect");
    }
    for path in ["", "../a", "/data/a", "/etc/a"] {
        assert!(matches!(
            download::fetch("https://example.com/a", path, None),
            Err(Error::Invalid(_))
        ));
    }

    assert!(programs::run("/bin/tool", &["--ok".into()]).is_ok());
    denied(programs::run("/bin/tool", &[]), "process:spawn");
    denied(
        programs::run("/bin/other", &["--ok".into()]),
        "process:spawn",
    );

    assert!(ipc::connect("/run/svc.sock").is_ok());
    denied(ipc::connect("/run/other.sock"), "ipc:connect");
}
