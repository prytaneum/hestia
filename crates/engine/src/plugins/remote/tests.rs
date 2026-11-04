use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use proto::error::ErrorInfo;
use proto::plugin::PluginSource;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zip::write::SimpleFileOptions;

use super::Remote;
use crate::cancel::Cancel;
use crate::plugins::Plugins;

struct Server {
    base: String,
    requests: Arc<AtomicUsize>,
}

/// Serves `respond(path)` as raw HTTP to every connection until dropped.
async fn serve(respond: impl Fn(&str) -> Vec<u8> + Send + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = requests.clone();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            counted.fetch_add(1, Ordering::SeqCst);
            let mut buffer = vec![0u8; 4096];
            let read = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            stream.write_all(&respond(&path)).await.unwrap();
            stream.shutdown().await.ok();
        }
    });
    Server { base, requests }
}

fn ok(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn package() -> Vec<u8> {
    let manifest = format!(
        "id = \"tunnel\"\nversion = \"1.0.0\"\napi = \"^{}\"\nwasm = \"plugin.wasm\"\n",
        proto::plugin::API_VERSION
    );
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in [
        ("plugin.toml", manifest.as_bytes()),
        ("plugin.wasm", b"component"),
    ] {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(body).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[tokio::test]
async fn a_url_install_uses_the_file_inspect_downloaded() {
    let home = tempfile::tempdir().unwrap();
    let remote = Remote::new(home.path().join("tmp"));
    let plugins = Plugins::new(home.path().join("plugins"));
    let archive = package();
    let server = serve(move |_| ok(&archive)).await;
    let url = format!("{}/tunnel.hplugin", server.base);

    let token = remote.download(&url, 1 << 20).await.expect("download");
    let staged = remote.staged(&token).expect("held");
    let digest = plugins.inspect(&staged.path).unwrap().grant_digest;
    let record = plugins
        .install_downloaded(&staged, &digest, &Cancel::new(), &|_| {}, |_| Ok(()))
        .expect("install");

    assert_eq!(record.source, PluginSource::Url);
    assert_eq!(record.url.as_deref(), Some(url.as_str()));
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);

    remote.discard(&token);
    assert!(!staged.path.exists(), "a discarded package is deleted");
    assert!(
        remote.staged(&token).is_err(),
        "and its token stops working"
    );
}

#[tokio::test]
async fn only_https_links_are_fetched() {
    let home = tempfile::tempdir().unwrap();
    let remote = Remote::new(home.path().join("tmp"));
    let server = serve(|_| ok(b"x")).await;

    for url in [
        format!("{}/a.hplugin", server.base),
        "ftp://example.com/a.hplugin".to_string(),
        "https://user:secret@example.com/a.hplugin".to_string(),
    ] {
        let refused = remote.fetch(&url).await.expect_err("refused");
        assert!(
            matches!(crate::error_info(refused), ErrorInfo::PluginInvalid { .. }),
            "{url}"
        );
    }
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_redirect_off_https_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let remote = Remote::new(home.path().join("tmp"));
    let target = serve(|_| ok(b"x")).await;
    let plain = format!("{}/stolen", target.base);
    let server = serve(move |_| {
        format!("HTTP/1.1 302 Found\r\nLocation: {plain}\r\nContent-Length: 0\r\n\r\n").into_bytes()
    })
    .await;

    remote
        .download(&format!("{}/a.hplugin", server.base), 1 << 20)
        .await
        .expect_err("a redirect to plain http");
    assert_eq!(target.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_package_over_the_cap_is_refused_and_not_kept() {
    let home = tempfile::tempdir().unwrap();
    let remote = Remote::new(home.path().join("tmp"));
    let server = serve(|_| ok(&[0u8; 2048])).await;

    remote
        .download(&format!("{}/big.hplugin", server.base), 1024)
        .await
        .expect_err("over the cap");
    assert_eq!(
        std::fs::read_dir(home.path().join("tmp")).map_or(0, |dir| dir.count()),
        0
    );
}
