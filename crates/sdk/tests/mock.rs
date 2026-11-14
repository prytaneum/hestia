use std::time::Duration;

use hestia::download::Expect;
use hestia::http::Request;
use hestia::process::Output;
use hestia::proto::java::{JavaInstall, JavaInstallDone, JavaInstallParams, JavaRuntime};
use hestia::proto::server::ServerList;
use hestia::proto::Empty;
use hestia::Error;

#[test]
fn unscripted_calls_fail_naming_what_was_asked() {
    let mock = hestia::mock::install();

    let call = hestia::call::<ServerList>(&Empty {}).unwrap_err();
    assert!(matches!(call, Error::Failed(ref m) if m.contains("server.list")));

    let run = hestia::process::exec("git", &["--version"], Duration::from_secs(5)).unwrap_err();
    assert!(matches!(run, Error::Failed(ref m) if m.contains("git")));

    let fetch = hestia::download::download(
        "https://files.example.com/tool.jar",
        Expect::Unhashed,
        "/data/tool.jar",
    )
    .unwrap_err();
    assert!(matches!(fetch, Error::Failed(ref m) if m.contains("files.example.com/tool.jar")));

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

#[test]
fn a_plugin_fetches_a_tool_runs_it_and_records_the_run() {
    let mock = hestia::mock::install();
    mock.file("https://files.example.com/tool.jar", 42);
    mock.exec(
        "java",
        Output {
            status: Some(0),
            stdout: b"built".to_vec(),
            stderr: Vec::new(),
        },
    );

    let file = hestia::download::download(
        "https://files.example.com/tool.jar",
        Expect::Sha1("da39a3ee5e6b4b0d3255bfef95601890afd80709"),
        "/data/tool.jar",
    )
    .unwrap();
    let output =
        hestia::process::exec("java", &["-jar", "/data/tool.jar"], Duration::from_secs(60))
            .unwrap();
    hestia::log::info("built");
    hestia::progress::report(Some(1.0), "done");
    let builds = hestia::data::update("builds", |count: &mut u32| {
        *count += 1;
        *count
    })
    .unwrap();

    assert_eq!(
        (file.size, output.stdout.as_slice(), builds),
        (42, &b"built"[..], 1)
    );
    assert_eq!(mock.downloads()[0].dest, "/data/tool.jar");
    assert_eq!(mock.runs()[0].args, ["-jar", "/data/tool.jar"]);
    assert_eq!(mock.data_value("builds"), Some(1.into()));

    let agent = hestia::process::spawn("agent", &[], &[]).unwrap();
    assert_eq!(
        hestia::process::list().unwrap(),
        std::slice::from_ref(&agent)
    );
    hestia::process::stop(&agent).unwrap();
    assert!(hestia::process::list().unwrap().is_empty());
}

#[test]
fn a_plugin_talks_to_a_local_program_in_the_users_language() {
    let mock = hestia::mock::install();
    mock.incoming("chat-ipc-0", b"ready");
    mock.messages("pt-BR", &[("greeting", "Ola {name}")]);

    assert!(matches!(
        hestia::ipc::open("chat-ipc-9"),
        Err(Error::NotFound(ref m)) if m.contains("chat-ipc-9")
    ));
    let chat = hestia::ipc::open("chat-ipc-0").unwrap();
    let greeting = hestia::messages::Messages::load(Some("pt-BR"))
        .unwrap()
        .format("greeting", &[("name", "Ana")]);
    chat.send(greeting.as_bytes()).unwrap();

    assert_eq!(
        chat.receive(4096, Duration::from_millis(1)).unwrap(),
        b"ready"
    );
    assert!(chat
        .receive(4096, Duration::from_millis(1))
        .unwrap()
        .is_empty());
    assert_eq!(mock.sent("chat-ipc-0"), [b"Ola Ana".to_vec()]);
    chat.close().unwrap();
    assert!(hestia::ipc::connections().unwrap().is_empty());
}
