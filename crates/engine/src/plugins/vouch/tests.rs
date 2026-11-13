use proto::error::ErrorInfo;
use proto::plugin::{Grant, Permission, PermissionScope, ScopeKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

fn grant(network: &[&str], unhashed: &[&str]) -> Grant {
    let mut scope = PermissionScope::of(ScopeKind::Hosts, network.iter().copied());
    scope.extend(ScopeKind::Unhashed, unhashed.iter().copied());
    Grant {
        permissions: [(Permission::NetworkConnect, scope)].into(),
        ..Grant::default()
    }
}

fn detail(error: anyhow::Error) -> String {
    match error.downcast::<ErrorInfo>().expect("a typed error") {
        ErrorInfo::DownloadFailed { detail } => detail,
        other => panic!("unexpected {other:?}"),
    }
}

async fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 4096];
            let read = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
            let response = match path.as_str() {
                "/a" => redirect("/b"),
                "/out" => redirect(&format!("http://localhost:{port}/b")),
                "/loop" => redirect("/loop"),
                _ => "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".into(),
            };
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.ok();
        }
    });
    port
}

fn redirect(to: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

#[tokio::test]
async fn a_redirect_chain_within_the_grant_is_followed() {
    let port = serve().await;
    let policy = Policy::build(&grant(&["127.0.0.1"], &[]), true);

    let response = policy
        .get(&format!("http://127.0.0.1:{port}/a"), false)
        .await
        .expect("followed");
    assert_eq!(response.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn a_redirect_to_a_host_outside_the_grant_is_refused() {
    let port = serve().await;
    let policy = Policy::build(&grant(&["127.0.0.1"], &[]), true);

    let error = policy
        .get(&format!("http://127.0.0.1:{port}/out"), false)
        .await
        .expect_err("refused");
    assert!(detail(error).contains("`localhost` is not in the plugin's granted hosts"));
}

#[tokio::test]
async fn an_unhashed_download_is_held_to_the_unhashed_hosts_on_every_hop() {
    let port = serve().await;
    let policy = Policy::build(&grant(&["127.0.0.1", "localhost"], &["127.0.0.1"]), true);
    let start = format!("http://127.0.0.1:{port}/out");

    let error = policy.get(&start, true).await.expect_err("refused");
    assert!(detail(error).contains("`localhost` is not a host the plugin declared"));

    policy
        .get(&start, false)
        .await
        .expect("a verified download only needs the grant");
}

#[tokio::test]
async fn a_redirect_loop_is_cut_off() {
    let port = serve().await;
    let policy = Policy::build(&grant(&["127.0.0.1"], &[]), true);

    let error = policy
        .get(&format!("http://127.0.0.1:{port}/loop"), false)
        .await
        .expect_err("cut off");
    assert_eq!(detail(error), "too many redirects");
}

#[tokio::test]
async fn loopback_is_refused_without_the_test_seam() {
    let policy = Policy::build(&grant(&["localhost", "127.0.0.1"], &[]), false);

    let named = policy.get("https://localhost/a.jar", false).await;
    assert!(detail(named.expect_err("refused")).contains("non-public"));

    let literal = policy.get("https://127.0.0.1/a.jar", false).await;
    assert!(detail(literal.expect_err("refused")).contains("IP address"));
}

#[tokio::test]
async fn plain_http_is_refused_without_the_test_seam() {
    let policy = Policy::build(&grant(&["example.com", "127.0.0.1"], &[]), false);

    for url in ["http://example.com/a.jar", "http://127.0.0.1:1/a.jar"] {
        let error = policy.get(url, false).await.expect_err(url);
        assert!(detail(error).contains("https"), "{url}");
    }
}

#[test]
fn permits_needs_a_granted_https_host() {
    let policy = Policy::build(
        &grant(&["files.example.com", "*.cdn.example.org"], &[]),
        false,
    );
    for allowed in [
        "https://files.example.com/a.jar",
        "https://x.cdn.example.org/a.jar",
    ] {
        assert!(policy.permits(allowed, false), "{allowed}");
    }
    for denied in [
        "http://files.example.com/a.jar",
        "https://evil.example.com/a.jar",
        "https://user:pw@files.example.com/a.jar",
        "https://cdn.example.org/a.jar",
        "https://a.b.cdn.example.org/a.jar",
        "http://localhost/a.jar",
        "not a url",
    ] {
        assert!(!policy.permits(denied, false), "{denied}");
    }
}

#[test]
fn unhashed_needs_the_host_in_both_lists() {
    let policy = Policy::build(
        &grant(
            &["files.example.com", "other.example.com"],
            &["files.example.com"],
        ),
        false,
    );

    assert!(policy.permits("https://files.example.com/a.jar", true));
    assert!(!policy.permits("https://other.example.com/a.jar", true));
    assert!(policy.permits("https://other.example.com/a.jar", false));
}
