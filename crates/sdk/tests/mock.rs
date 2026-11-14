use hestia::http::Request;
use hestia::proto::java::{JavaInstall, JavaInstallDone, JavaInstallParams, JavaRuntime};
use hestia::proto::server::ServerList;
use hestia::proto::Empty;
use hestia::Error;

#[test]
fn unscripted_calls_fail_naming_what_was_asked() {
    let mock = hestia::mock::install();

    let call = hestia::call::<ServerList>(&Empty {}).unwrap_err();
    assert!(matches!(call, Error::Failed(ref m) if m.contains("server.list")));

    let request = Request::get("https://api.example.com/v1")
        .send()
        .unwrap_err();
    assert!(
        matches!(request, Error::Failed(ref m) if m.contains("Get") && m.contains("https://api.example.com/v1"))
    );
    assert_eq!(mock.calls().len(), 1);
    assert_eq!(mock.requests().len(), 1);
}

#[test]
fn a_plugin_installs_a_runtime_and_remembers_a_token() {
    let mock = hestia::mock::install();
    mock.done::<JavaInstall>(&JavaInstallDone {
        runtime: JavaRuntime::default(),
        already_installed: false,
    });

    let params = JavaInstallParams {
        major: 21,
        ..JavaInstallParams::default()
    };
    hestia::run_job::<JavaInstall>(&params).unwrap();
    let calls = mock.calls();
    assert!(calls[0].job);
    assert_eq!(calls[0].params["major"], 21);

    hestia::settings::set("token", &"s3cret").unwrap();
    assert_eq!(
        hestia::settings::get::<String>("token").unwrap().as_deref(),
        Some("s3cret")
    );
    hestia::settings::clear("token").unwrap();
    assert_eq!(hestia::settings::get::<String>("token").unwrap(), None);
}
