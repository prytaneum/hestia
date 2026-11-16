use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wasmtime_wasi_http::{Error as HttpError, WasiBody, WasiHttpHooks};

use super::bindings::hestia::kernel::types::PluginError;
use super::bindings::hestia::kernel::{download, ipc, programs};
use super::host::{Call, State};
use super::http::Hooks;
use super::limits::MemoryCap;
use super::reach::Reach;
use super::shared::Shared;
use super::{Services, Shape};
use crate::grant::{derive, Table};
use crate::manifest::Manifest;
use crate::vetting::{Hosts, Policy};

struct Data(PathBuf);

impl Services for Data {
    fn data_dir(&self, _plugin: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

fn state(data: &Path, reach: Reach) -> State {
    State::new(
        "tester",
        true,
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

    for path in ["../f", "/etc/f", "a/../../f", ""] {
        let result = fetch(&mut plugin, &format!("{local}/f"), path, sha).await;
        assert!(
            matches!(result, Err(PluginError::Invalid(_))),
            "{path:?}: {result:?}"
        );
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
    let data = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), data.path().join("out")).unwrap();
    let port = serve().await;
    let mut plugin = state(data.path(), reaching(&["files.example"], false));
    let url = format!("http://127.0.0.1:{port}/f");
    let result = fetch(&mut plugin, &url, "out/f", Some(PAYLOAD_SHA256)).await;
    assert!(matches!(result, Err(PluginError::Invalid(_))), "{result:?}");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
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
        reach: reach.clone(),
    };
    let sent = hooks.send_request(request(url), None, Box::new(async { Ok(()) }));
    Box::into_pin(sent)
        .await
        .map(|(response, _)| response.status())
}

#[tokio::test]
async fn wasi_http_goes_through_the_plugins_policy_and_leaves_redirects_to_the_guest() {
    let port = serve().await;
    let reach = Arc::new(reaching(&["files.example"], true));

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

fn invocations(program: &Path, args: &str) -> Reach {
    let manifest = Manifest::parse(&format!(
        "id = \"t\"\nname = \"t\"\nversion = \"1\"\n\
         [permissions.\"process:spawn\"]\nprograms = ['{} {args}']\n",
        program.display()
    ))
    .unwrap();
    let shape = Shape {
        imports: vec!["hestia:kernel/programs@0.1.0".into()],
        exports: Vec::new(),
    };
    Reach::of(&derive(&Table::kernel(), &manifest, &shape).unwrap())
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
    let tool = dir
        .path()
        .join(if cfg!(windows) { "tool.exe" } else { "tool" });
    std::fs::copy(std::env::current_exe().unwrap(), &tool).unwrap();
    let path = tool.to_str().unwrap().to_owned();
    let mut plugin = state(dir.path(), invocations(&tool, "--list"));

    assert_eq!(
        run(&mut plugin, &path, &["--list"]).await.unwrap().status,
        0
    );

    denied(
        run(&mut plugin, &path, &["--list", "--extra"]).await,
        "process:spawn",
    );
    denied(run(&mut plugin, &path, &[]).await, "process:spawn");
    denied(
        run(&mut plugin, "/bin/sh", &["-c", "id"]).await,
        "process:spawn",
    );

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&tool)
        .unwrap();
    std::io::Write::write_all(&mut file, b"\0").unwrap();
    drop(file);
    denied(run(&mut plugin, &path, &["--list"]).await, "process:spawn");
}

#[tokio::test]
async fn a_local_endpoint_is_reached_only_when_the_grant_lists_it() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = dir.path().join("peer.sock");
    let listed = endpoint.to_str().unwrap().to_owned();
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
        denied(
            ipc::Host::request(&mut plugin, other, Vec::new()).await,
            "ipc:connect",
        );
    }
    #[cfg(unix)]
    {
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut asked = Vec::new();
            stream.read_to_end(&mut asked).await.unwrap();
            let answer = [b"re:".as_slice(), &asked].concat();
            stream.write_all(&answer).await.unwrap();
        });
        let answer = ipc::Host::request(&mut plugin, listed, b"ping".to_vec()).await;
        assert_eq!(answer.unwrap(), b"re:ping");
    }
}
