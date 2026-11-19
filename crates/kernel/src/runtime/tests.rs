use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use wasmtime::component::Resource;
use wasmtime_wasi_http::{Error as HttpError, WasiBody, WasiHttpHooks};

use super::bindings::hestia::kernel::ipc::HostConnection;
use super::bindings::hestia::kernel::types::PluginError;
use super::bindings::hestia::kernel::{download, ipc, programs, secrets, settings};
use super::download::Staged;
use super::host::{Call, State};
use super::http::Hooks;
use super::limits::MemoryCap;
use super::quota::Quota;
use super::reach::Reach;
use super::shared::Shared;
use super::Services;
use crate::grant::digest;
#[cfg(unix)]
use crate::grant::Reserved;
use crate::vetting::{Hosts, Policy};
use proto::plugin::Program;

struct Data(PathBuf);

impl Services for Data {
    fn data_dir(&self, _plugin: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }

    fn state_dir(&self, _plugin: &str) -> Option<PathBuf> {
        Some(self.0.join("state"))
    }
}

fn state(data: &Path, reach: Reach) -> State {
    state_online(data, reach, true)
}

fn state_online(data: &Path, reach: Reach, online: bool) -> State {
    State::new(
        "tester",
        online,
        MemoryCap::new(1 << 20),
        Shared::new(Arc::new(Data(data.to_owned()))),
        Call::default(),
        Arc::new(reach),
    )
}

fn reaching(hosts: &[&str], loopback_unhashed: bool) -> Reach {
    let hosts = Hosts::new(hosts, &[])
        .unwrap()
        .with_loopback(loopback_unhashed);
    Reach {
        policy: Policy::new(hosts),
        ..Reach::default()
    }
}

fn denied(result: Result<impl std::fmt::Debug, PluginError>, permission: &str) {
    match result {
        Err(PluginError::Denied(named)) if named == permission => {}
        other => panic!("expected denied({permission}), got {other:?}"),
    }
}

async fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 4096];
                let read = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
                let response = match path.as_str() {
                    "/away" => "HTTP/1.1 302 Found\r\nLocation: https://evil.example/f\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    _ => "HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\npayload",
                };
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.ok();
            });
        }
    });
    port
}

const PAYLOAD_SHA256: &str = "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5";

async fn fetch(
    plugin: &mut State,
    url: &str,
    path: &str,
    sha256: Option<&str>,
) -> Result<u64, PluginError> {
    download::Host::fetch(
        plugin,
        url.to_owned(),
        path.to_owned(),
        sha256.map(str::to_owned),
    )
    .await
}

#[tokio::test]
async fn a_download_goes_only_to_granted_hosts_and_is_written_whole_or_not_at_all() {
    let data = tempfile::tempdir().unwrap();
    let port = serve().await;
    let mut plugin = state(data.path(), reaching(&["files.example"], false));
    let local = format!("http://127.0.0.1:{port}");
    let sha = Some(PAYLOAD_SHA256);

    let refused = [
        ("https://evil.example/f".to_owned(), sha),
        ("https://93.184.216.34/f".to_owned(), sha),
        ("http://files.example/f".to_owned(), sha),
        ("https://files.example/f".to_owned(), None),
        (format!("{local}/away"), sha),
        (format!("{local}/f"), None),
    ];
    for (url, sha) in refused {
        denied(fetch(&mut plugin, &url, "f", sha).await, "network:connect");
    }
    assert!(!data.path().join("f").exists() && !data.path().join("f.part").exists());

    let unportable = [
        "../f",
        "/etc/f",
        "a/../../f",
        "",
        "con",
        "NUL.txt",
        "a/Aux.tar.gz/f",
        "com1",
        "lpt\u{b9}",
        "conout$",
        "f:stream",
        "f::$DATA",
        "a\\f",
        "f.",
        "f ",
        "a./f",
        "d /f",
        "f*",
        "f\n",
    ];
    for path in unportable {
        let result = fetch(&mut plugin, &format!("{local}/f"), path, sha).await;
        assert!(
            matches!(result, Err(PluginError::Invalid(_))),
            "{path:?}: {result:?}"
        );
    }

    for path in ["console/com10.d/nulled", ".hidden", "a//b/./c"] {
        let saved = fetch(&mut plugin, &format!("{local}/f"), path, sha).await;
        assert_eq!(saved.unwrap(), 7, "{path}");
    }

    let wrong = "0".repeat(64);
    let result = fetch(&mut plugin, &format!("{local}/f"), "f", Some(&wrong)).await;
    assert!(matches!(result, Err(PluginError::Failed(_))), "{result:?}");
    assert!(!data.path().join("f").exists() && !data.path().join("f.part").exists());

    let upper = PAYLOAD_SHA256.to_uppercase();
    let saved = fetch(&mut plugin, &format!("{local}/f"), "sub/f", Some(&upper)).await;
    assert_eq!(saved.unwrap(), 7);
    assert_eq!(
        std::fs::read(data.path().join("sub/f")).unwrap(),
        b"payload"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_download_never_follows_a_link_out_of_the_plugins_data() {
    let root = tempfile::tempdir().unwrap();
    let (data, outside) = (root.path().join("data"), root.path().join("outside"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let port = serve().await;
    let mut plugin = state(&data, reaching(&["files.example"], false));
    let url = format!("http://127.0.0.1:{port}/f");
    for link in [outside.as_path(), Path::new("../outside")] {
        let _ = std::fs::remove_file(data.join("out"));
        std::os::unix::fs::symlink(link, data.join("out")).unwrap();
        let result = fetch(&mut plugin, &url, "out/new/f", Some(PAYLOAD_SHA256)).await;
        assert!(matches!(result, Err(PluginError::Invalid(_))), "{result:?}");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0, "{link:?}");
    }

    let (staged, file) = Staged::create(&data, "held/f".into()).unwrap();
    drop(file);
    std::fs::rename(data.join("held"), data.join("aside")).unwrap();
    std::os::unix::fs::symlink("../outside", data.join("held")).unwrap();
    std::fs::write(outside.join("f.part"), b"theirs").unwrap();
    assert!(staged.commit().is_err());
    assert!(
        !outside.join("f").exists(),
        "a directory swapped for a link after the file was staged is not followed"
    );
    assert_eq!(std::fs::read(outside.join("f.part")).unwrap(), b"theirs");
}

#[tokio::test]
async fn a_download_that_is_dropped_leaves_no_staging_file() {
    let data = tempfile::tempdir().unwrap();
    let (staged, file) = Staged::create(data.path(), "a/f".into()).unwrap();
    drop(file);
    assert!(data.path().join("a/f.part").exists());
    drop(staged);
    assert!(!data.path().join("a/f.part").exists());
}

#[tokio::test]
async fn nothing_leaves_the_machine_while_offline() {
    let data = tempfile::tempdir().unwrap();
    let port = serve().await;
    let mut plugin = state_online(data.path(), reaching(&["files.example"], false), false);
    let result = fetch(
        &mut plugin,
        &format!("http://127.0.0.1:{port}/f"),
        "f",
        None,
    )
    .await;
    assert!(matches!(result, Err(PluginError::Offline)), "{result:?}");
    let reach = Arc::new(reaching(&["files.example"], false));
    let mut hooks = Hooks {
        plugin: "tester".into(),
        online: false,
        reach,
    };
    let local = format!("http://127.0.0.1:{port}/f");
    let sent = hooks.send_request(request(&local), None, Box::new(async { Ok(()) }));
    let result = Box::into_pin(sent).await.map(|_| ());
    assert!(
        matches!(result, Err(HttpError::ConnectionRefused)),
        "{result:?}"
    );
}

fn request(url: &str) -> http::Request<WasiBody> {
    use http_body_util::{BodyExt, Empty};
    http::Request::get(url)
        .body(
            Empty::<bytes::Bytes>::new()
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .unwrap()
}

async fn send(reach: &Arc<Reach>, url: &str) -> Result<http::StatusCode, HttpError> {
    let mut hooks = Hooks {
        plugin: "tester".into(),
        online: true,
        reach: reach.clone(),
    };
    let sent = hooks.send_request(request(url), None, Box::new(async { Ok(()) }));
    Box::into_pin(sent)
        .await
        .map(|(response, _)| response.status())
}

#[tokio::test]
async fn wasi_http_reaches_any_granted_host_through_the_plugins_policy_and_leaves_redirects_to_the_guest(
) {
    let port = serve().await;
    let reach = Arc::new(reaching(&["files.example"], false));

    for url in [
        "https://evil.example/f",
        "https://93.184.216.34/f",
        "https://user:pw@files.example/f",
        "http://files.example/f",
    ] {
        let result = send(&reach, url).await;
        assert!(
            matches!(result, Err(HttpError::HttpRequestDenied)),
            "{url}: {result:?}"
        );
    }

    let local = format!("http://127.0.0.1:{port}");
    let redirected = send(&reach, &format!("{local}/away")).await.unwrap();
    assert_eq!(redirected, 302, "the guest follows it, and is vetted again");
    assert_eq!(send(&reach, &format!("{local}/f")).await.unwrap(), 200);

    let nothing = Arc::new(Reach::default());
    let result = send(&nothing, &format!("{local}/f")).await;
    assert!(matches!(result, Err(HttpError::HttpRequestDenied)));
}

#[cfg(unix)]
const SCRIPT: &str = "#!/bin/sh\necho \"$1\"\n( while :; do :; done ) &\nsetsid sleep 30 &\n";
#[cfg(unix)]
const SCRIPT_ARGS: &[&str] = &["--list"];
#[cfg(windows)]
const SCRIPT_ARGS: &[&str] = &["/c", "start /b ping -n 60 127.0.0.1"];

fn tool(dir: &Path) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tool = dir.join("tool");
        std::fs::write(&tool, SCRIPT).unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        tool
    }
    #[cfg(windows)]
    {
        let _ = dir;
        PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join(r"System32\cmd.exe")
    }
}

fn granted(program: &Path, sha256: String) -> Reach {
    Reach {
        programs: vec![Program {
            path: program.to_str().unwrap().to_owned(),
            args: SCRIPT_ARGS.iter().map(|a| a.to_string()).collect(),
            sha256,
        }],
        ..Reach::default()
    }
}

async fn run(
    plugin: &mut State,
    program: &str,
    args: &[&str],
) -> Result<programs::Output, PluginError> {
    let args = args.iter().map(|arg| arg.to_string()).collect();
    programs::Host::run(plugin, program.to_owned(), args).await
}

#[tokio::test]
async fn a_program_runs_only_as_declared_and_only_while_it_is_the_one_approved() {
    let dir = tempfile::tempdir().unwrap();
    let tool = tool(dir.path());
    let path = tool.to_str().unwrap().to_owned();
    let pinned = digest(&tool).unwrap();

    let mut plugin = state(dir.path(), granted(&tool, pinned));
    let ran = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        run(&mut plugin, &path, SCRIPT_ARGS),
    )
    .await
    .expect("a process the program left behind must not hold the call")
    .unwrap();
    assert_eq!(ran.status, 0);
    #[cfg(unix)]
    assert_eq!(ran.stdout, b"--list\n");

    denied(run(&mut plugin, &path, &["--extra"]).await, "process:spawn");
    denied(run(&mut plugin, &path, &[]).await, "process:spawn");
    denied(
        run(&mut plugin, "/bin/sh", &["-c", "id"]).await,
        "process:spawn",
    );

    let mut moved = state(dir.path(), granted(&tool, "0".repeat(64)));
    denied(run(&mut moved, &path, SCRIPT_ARGS).await, "process:spawn");

    // Windows: the mounted `/data` handle denies delete sharing, so the
    // directory cannot vanish under a live plugin.
    #[cfg(unix)]
    {
        let gone = dir.path().join("gone");
        let mut homeless = state(&gone, granted(&tool, digest(&tool).unwrap()));
        std::fs::remove_dir_all(&gone).unwrap();
        let result = run(&mut homeless, &path, SCRIPT_ARGS).await;
        assert!(matches!(result, Err(PluginError::Failed(_))), "{result:?}");
    }
}

async fn ping(plugin: &mut State, endpoint: String) {
    let connection = ipc::Host::connect(plugin, endpoint).await.unwrap();
    let again = Resource::new_borrow(connection.rep());
    let invalid = plugin.read(again, 0).await;
    assert!(
        matches!(invalid, Err(PluginError::Invalid(_))),
        "{invalid:?}"
    );
    plugin
        .write(Resource::new_borrow(connection.rep()), b"ping".to_vec())
        .await
        .unwrap();
    let mut answer = Vec::new();
    loop {
        let read = plugin
            .read(Resource::new_borrow(connection.rep()), 64)
            .await
            .unwrap();
        if read.is_empty() {
            break;
        }
        answer.extend(read);
    }
    assert_eq!(answer, b"re:ping", "an empty read is the peer closing");
    HostConnection::drop(plugin, connection).unwrap();
    assert_eq!(plugin.connections, 0);
}

async fn echo_once(mut stream: impl AsyncRead + AsyncWrite + Unpin) {
    let mut asked = [0u8; 4];
    stream.read_exact(&mut asked).await.unwrap();
    stream
        .write_all(&[b"re:".as_slice(), &asked].concat())
        .await
        .unwrap();
}

#[tokio::test]
async fn a_local_endpoint_is_reached_only_when_the_grant_lists_it() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let listed = dir.path().join("peer.sock").to_str().unwrap().to_owned();
    #[cfg(windows)]
    let listed = format!(r"\\.\pipe\tester-{}", std::process::id());
    let mut plugin = state(
        dir.path(),
        Reach {
            endpoints: vec![listed.clone()],
            ..Reach::default()
        },
    );
    for other in [
        dir.path().join("other.sock").to_str().unwrap().to_owned(),
        format!("{listed}/../peer.sock"),
        String::new(),
    ] {
        denied(ipc::Host::connect(&mut plugin, other).await, "ipc:connect");
    }
    #[cfg(unix)]
    {
        let listener = tokio::net::UnixListener::bind(&listed).unwrap();
        tokio::spawn(async move { echo_once(listener.accept().await.unwrap().0).await });
    }
    #[cfg(windows)]
    {
        let server = tokio::net::windows::named_pipe::ServerOptions::new()
            .first_pipe_instance(true)
            .create(&listed)
            .unwrap();
        tokio::spawn(async move {
            server.connect().await.unwrap();
            echo_once(server).await
        });
    }
    ping(&mut plugin, listed).await;
}

#[cfg(unix)]
#[tokio::test]
async fn an_endpoint_that_leads_to_hestia_is_refused_when_connecting() {
    let dir = tempfile::tempdir().unwrap();
    let own = dir.path().join("hestiad.sock");
    let _listener = tokio::net::UnixListener::bind(&own).unwrap();
    let own = own.to_str().unwrap().to_owned();
    let lane = dir.path().join("lane");
    std::fs::create_dir(&lane).unwrap();
    let sandbox = dir.path().join("sandbox");
    std::fs::create_dir(&sandbox).unwrap();
    std::os::unix::fs::symlink(dir.path(), sandbox.join("up")).unwrap();
    std::os::unix::fs::symlink(&own, sandbox.join("link.sock")).unwrap();
    std::fs::hard_link(&own, lane.join("hard.sock")).unwrap();

    for (case, endpoint) in [
        ("a symlink to it", sandbox.join("link.sock")),
        (
            "a path through a symlinked directory",
            sandbox.join("up/hestiad.sock"),
        ),
        (
            "a path that climbs back to it",
            lane.join("../hestiad.sock"),
        ),
        ("itself", PathBuf::from(&own)),
        (
            "a hard link, caught by the bound address",
            lane.join("hard.sock"),
        ),
    ] {
        let endpoint = endpoint.to_str().unwrap().to_owned();
        let mut plugin = state(
            dir.path(),
            Reach {
                endpoints: vec![endpoint.clone()],
                ..Reach::default()
            },
        );
        plugin
            .shared
            .own
            .set(Reserved {
                endpoints: vec![own.clone()],
                ..Reserved::default()
            })
            .unwrap();
        let result = ipc::Host::connect(&mut plugin, endpoint).await;
        assert!(
            matches!(&result, Err(PluginError::Denied(named)) if named == "ipc:connect"),
            "{case}: {result:?}"
        );
    }
}

#[tokio::test]
async fn stored_state_that_cannot_be_read_fails_and_is_never_replaced() {
    let data = tempfile::tempdir().unwrap();
    let mut plugin = state(data.path(), Reach::default());
    for name in ["settings.json", "secrets.json"] {
        std::fs::create_dir_all(data.path().join("state").join(name)).unwrap();
    }

    let results = [
        settings::Host::get(&mut plugin, "k".into()).await.map(drop),
        settings::Host::set(&mut plugin, "k".into(), "1".into()).await,
        secrets::Host::get(&mut plugin, "k".into()).await.map(drop),
        secrets::Host::set(&mut plugin, "k".into(), "v".into()).await,
    ];
    for result in results {
        assert!(matches!(result, Err(PluginError::Failed(_))), "{result:?}");
    }
    for name in ["settings.json", "secrets.json"] {
        assert!(data.path().join("state").join(name).is_dir(), "{name}");
    }
}

#[tokio::test]
async fn a_download_that_would_pass_the_data_quota_is_refused_and_a_replacement_is_not_counted_twice(
) {
    let data = tempfile::tempdir().unwrap();
    let port = serve().await;
    let shared = Shared {
        quota: Arc::new(Quota::new(10)),
        ..Shared::new(Arc::new(Data(data.path().to_owned())))
    };
    let mut plugin = State::new(
        "tester",
        true,
        MemoryCap::new(1 << 20),
        shared,
        Call::default(),
        Arc::new(reaching(&["files.example"], false)),
    );
    let url = format!("http://127.0.0.1:{port}/f");
    let sha = Some(PAYLOAD_SHA256);

    assert_eq!(fetch(&mut plugin, &url, "a", sha).await.unwrap(), 7);
    let result = fetch(&mut plugin, &url, "b", sha).await;
    assert!(matches!(result, Err(PluginError::Failed(_))), "{result:?}");
    assert!(!data.path().join("b").exists() && !data.path().join("b.part").exists());
    assert_eq!(fetch(&mut plugin, &url, "a", sha).await.unwrap(), 7);
}
