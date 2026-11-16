use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

type Sent = Result<Response, reqwest::Error>;

fn policy(connect: &[&str], unhashed: &[&str]) -> Policy {
    Policy::new(Hosts::new(connect, unhashed).expect("valid hosts"))
}

fn local(connect: &[&str], loopback_unhashed: bool) -> Policy {
    let hosts = Hosts::new(connect, &[]).expect("valid hosts");
    Policy::new(hosts.with_loopback(loopback_unhashed))
}

async fn fetch(
    policy: &Policy,
    url: &str,
    unhashed: bool,
    limits: Limits,
) -> Result<Response, Failure<reqwest::Error>> {
    policy
        .get(url, unhashed, limits, |request| async move {
            request.send().await as Sent
        })
        .await
}

fn refusal<T>(result: Result<T, Failure<reqwest::Error>>) -> Refusal {
    match result {
        Err(Failure::Refused(refusal)) => refusal,
        Err(Failure::Send(error)) => panic!("expected a refusal, got {error}"),
        Ok(_) => panic!("expected a refusal"),
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
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let response = match path.as_str() {
                    "/a" => redirect("/b"),
                    "/off-grant" => redirect("https://other.example.com/b"),
                    "/downgrade" => redirect("http://cdn.example.com/b"),
                    "/literal" => redirect("https://93.184.216.34/b"),
                    "/hashed-only" => redirect("https://cdn.example.com/b"),
                    "/loop" => redirect("/loop"),
                    "/declared" => {
                        "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n"
                            .into()
                    }
                    "/stream" => format!(
                        "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{}",
                        "x".repeat(100)
                    ),
                    "/stall" => {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        return;
                    }
                    _ => {
                        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".into()
                    }
                };
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.ok();
            });
        }
    });
    port
}

fn redirect(to: &str) -> String {
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

#[test]
fn a_url_is_vetted_before_any_request() {
    let policy = policy(&["cdn.example.com"], &[]);
    for (url, unhashed, expected) in [
        ("http://cdn.example.com/a", false, Refusal::NotHttps),
        ("http://127.0.0.1:8080/a", false, Refusal::NotHttps),
        ("http://localhost/a", false, Refusal::NotHttps),
        ("https://93.184.216.34/a", false, Refusal::IpLiteral),
        ("https://[2606:2800::1]/a", false, Refusal::IpLiteral),
        ("https://0x7f000001/a", false, Refusal::IpLiteral),
        (
            "https://user:secret@cdn.example.com/a",
            false,
            Refusal::Credentials,
        ),
        (
            "https://other.example.com/a",
            false,
            Refusal::HostNotGranted("other.example.com".into()),
        ),
        (
            "https://cdn.example.com/a",
            true,
            Refusal::Unhashed("cdn.example.com".into()),
        ),
    ] {
        let url = Url::parse(url).unwrap();
        assert_eq!(policy.vet(&url, unhashed), Err(expected), "{url}");
    }
    for url in ["https://cdn.example.com/a", "https://CDN.example.com./a"] {
        assert!(
            policy.vet(&Url::parse(url).unwrap(), false).is_ok(),
            "{url}"
        );
    }
    let open = Policy::new(Hosts::open_unhashed());
    let loopback = Url::parse("http://127.0.0.1:8080/a").unwrap();
    assert_eq!(
        open.vet(&loopback, true),
        Err(Refusal::NotHttps),
        "an open policy never reaches plain-http loopback"
    );
}

#[test]
fn a_granted_host_entry_is_validated() {
    for bad in [
        "*.com",
        "*.co.uk",
        "*.github.io",
        "localhost",
        "127.0.0.1",
        "[::1]",
        "93.184.216.34",
        "https://a.example.com",
        "a.example.com/x",
        "*",
        "a*.example.com",
    ] {
        assert!(Hosts::new(&[bad], &[]).is_err(), "{bad} must be refused");
    }
    let hosts = Hosts::new(&["*.example.com", "api.example.org."], &["example.co.uk"]).unwrap();
    assert!(hosts.reaches("cdn.example.com"));
    assert!(
        !hosts.reaches("example.com"),
        "a wildcard is exactly one label"
    );
    assert!(!hosts.reaches("a.b.example.com"));
    assert!(hosts.reaches("api.example.org"));
    assert!(hosts.serves_unhashed("example.co.uk"));
}

#[tokio::test]
async fn a_name_that_resolves_inside_the_machine_is_refused() {
    let policy = Policy::new(Hosts::open());
    let Err(Failure::Send(error)) =
        fetch(&policy, "https://localhost/", false, Limits::REQUEST).await
    else {
        panic!("expected the resolver to refuse");
    };
    assert!(was_refused(&error), "{error:?}");
}

#[tokio::test]
async fn a_redirect_is_vetted_on_every_hop() {
    let port = serve().await;
    let policy = local(&["cdn.example.com"], true);
    let hop = |path: &str| format!("http://127.0.0.1:{port}{path}");

    let followed = fetch(&policy, &hop("/a"), true, Limits::REQUEST).await;
    assert_eq!(followed.expect("followed").text().await.unwrap(), "ok");

    for (path, expected) in [
        (
            "/off-grant",
            Refusal::HostNotGranted("other.example.com".into()),
        ),
        ("/downgrade", Refusal::NotHttps),
        ("/literal", Refusal::IpLiteral),
        ("/hashed-only", Refusal::Unhashed("cdn.example.com".into())),
        ("/loop", Refusal::TooManyRedirects),
    ] {
        let outcome = fetch(&policy, &hop(path), true, Limits::REQUEST).await;
        assert_eq!(refusal(outcome), expected, "{path}");
    }
}

#[tokio::test]
async fn the_size_cap_holds_for_a_declared_and_a_streamed_body() {
    let port = serve().await;
    let limits = Limits {
        time: Some(Duration::from_secs(30)),
        bytes: 10,
    };
    let policy = local(&[], false);
    for path in ["/declared", "/stream"] {
        let response = fetch(
            &policy,
            &format!("http://127.0.0.1:{port}{path}"),
            false,
            limits,
        )
        .await
        .ok()
        .unwrap();
        assert_eq!(
            refusal(read(response, limits).await),
            Refusal::TooLarge(10),
            "{path}"
        );
    }
}

#[tokio::test]
async fn the_time_cap_ends_a_request_that_never_answers() {
    let port = serve().await;
    let limits = Limits {
        time: Some(Duration::from_millis(100)),
        bytes: 10,
    };
    let policy = local(&[], false);

    let Err(Failure::Send(error)) = fetch(
        &policy,
        &format!("http://127.0.0.1:{port}/stall"),
        false,
        limits,
    )
    .await
    else {
        panic!("expected a timeout");
    };
    assert!(error.is_timeout(), "{error:?}");
}

#[test]
fn only_a_policy_that_lists_its_hosts_may_use_the_proxy() {
    use client::{route, Route};
    let granted = Policy::new(Hosts::new(&["cdn.example.com"], &[]).unwrap());
    let url = |raw: &str| Url::parse(raw).unwrap();
    for (policy, expected) in [
        (&granted, Route::Granted),
        (&Policy::new(Hosts::open()), Route::Open),
        (&Policy::new(Hosts::open_unhashed()), Route::Open),
    ] {
        let lists_hosts = policy.hosts.lists_hosts();
        assert_eq!(
            route(&url("https://cdn.example.com/a"), lists_hosts),
            expected
        );
    }
    assert_eq!(
        route(&url("http://127.0.0.1:8080/a"), false),
        Route::Loopback
    );

    let setting = |vars: &[(&str, &str)]| {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        client::proxy_setting(|name| {
            vars.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        })
        .map(|url| url.to_string())
    };
    assert_eq!(setting(&[]), None);
    assert_eq!(setting(&[("HTTPS_PROXY", " ")]), None);
    assert_eq!(
        setting(&[
            ("ALL_PROXY", "socks.example:1080"),
            ("https_proxy", "proxy.corp:3128")
        ]),
        Some("http://proxy.corp:3128/".into()),
        "https_proxy wins over ALL_PROXY, and a bare host:port is http"
    );
}
