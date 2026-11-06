//! Outgoing HTTP for plugins: `wasi:http` linked for every plugin, with the
//! decision made per request. A plugin without `network:connect` loads and runs
//! all the same; its requests are refused with a typed error, as a stub would.
//!
//! What a plugin may reach is its granted host list over https. Dialling goes
//! through [`guard::client`], whose resolver vets the addresses it connects to,
//! so a granted name that resolves inside this machine is still refused.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, LengthLimitError, Limited};
use proto::net::NetworkState;
use proto::plugin::{Grant, Permission};
use reqwest::Url;
use tracing::Level;
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

use super::output::PluginLog;
use crate::net::{guard, network};

type Request = http::Request<WasiBody>;
type Response = http::Response<WasiBody>;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

/// Why a request was not sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("the plugin lacks the network:connect permission")]
    NotGranted,
    #[error("only https requests are allowed")]
    NotHttps,
    #[error("the request URL is not valid")]
    BadUrl,
    #[error("an IP address cannot be a destination")]
    IpLiteral,
    #[error("`{0}` is not in the plugin's granted hosts")]
    HostNotGranted(String),
    #[error("the daemon is offline")]
    Offline,
}

impl Refusal {
    fn code(&self) -> Error {
        match self {
            Refusal::IpLiteral => Error::DestinationIpProhibited,
            Refusal::BadUrl => Error::HttpRequestUriInvalid,
            Refusal::Offline => Error::DestinationUnavailable,
            Refusal::NotGranted | Refusal::NotHttps | Refusal::HostNotGranted(_) => {
                Error::HttpRequestDenied
            }
        }
    }
}

/// The destination host, lowercased and without a trailing dot, when a plugin
/// holding `grant` may request `url` while the daemon is (not) `offline`.
pub fn authorize(grant: &Grant, url: &Url, offline: bool) -> Result<String, Refusal> {
    if !grant.permissions.contains(&Permission::NetworkConnect) {
        return Err(Refusal::NotGranted);
    }
    if url.scheme() != "https" {
        return Err(Refusal::NotHttps);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Refusal::BadUrl);
    }
    let Some(host) = url.host_str() else {
        return Err(Refusal::BadUrl);
    };
    let Some(domain) = url.domain() else {
        return Err(Refusal::IpLiteral);
    };
    if guard::is_ip_literal(host) {
        return Err(Refusal::IpLiteral);
    }
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    if !grant
        .network
        .iter()
        .any(|entry| host_matches(entry, &domain))
    {
        return Err(Refusal::HostNotGranted(domain));
    }
    if offline {
        return Err(Refusal::Offline);
    }
    Ok(domain)
}

/// `entry` is an exact host, or `*.domain`, which matches subdomains of
/// `domain` at any depth but not `domain` itself. `host` is already normalised.
fn host_matches(entry: &str, host: &str) -> bool {
    let entry = entry.trim_end_matches('.').to_ascii_lowercase();
    match entry.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.')),
        None => host == entry,
    }
}

/// Whether the engine may fetch `url` on behalf of a plugin holding `grant`:
/// the file a content source lists must sit on one of its granted hosts, over
/// https (plain http only to a loopback host the grant names, which a test or a
/// local index serves from).
pub fn download_allowed(grant: &Grant, url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    let secure = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    secure
        && grant
            .network
            .iter()
            .any(|entry| host_matches(entry, &host))
}

/// The `wasi:http` hooks of one plugin call.
pub(crate) struct PluginHttp {
    pub(crate) plugin: Arc<str>,
    pub(crate) grant: Arc<Grant>,
    pub(crate) log: Arc<PluginLog>,
}

impl WasiHttpHooks for PluginHttp {
    fn set_host_header(&mut self) -> bool {
        false
    }

    fn send_request(
        &mut self,
        request: Request,
        _options: Option<RequestOptions>,
        _fut: Box<dyn Future<Output = Result<(), Error>> + Send>,
    ) -> Box<
        dyn Future<
                Output = Result<
                    (Response, Box<dyn Future<Output = Result<(), Error>> + Send>),
                    Error,
                >,
            > + Send,
    > {
        let (plugin, grant, log) = (self.plugin.clone(), self.grant.clone(), self.log.clone());
        Box::new(async move {
            let offline = network().status().state == NetworkState::Offline;
            let checked = Url::parse(&request.uri().to_string())
                .map_err(|_| Refusal::BadUrl)
                .and_then(|url| authorize(&grant, &url, offline).map(|host| (url, host)));
            let (url, host) = checked.map_err(|refusal| {
                tracing::debug!(plugin = %plugin, %refusal, "plugin http request refused");
                log.line(Level::WARN, &format!("http request refused: {refusal}"));
                refusal.code()
            })?;
            let response = send(request, url).await.inspect_err(|error| {
                tracing::debug!(plugin = %plugin, %host, %error, "plugin http request failed");
            })?;
            tracing::debug!(
                plugin = %plugin,
                %host,
                status = response.status().as_u16(),
                "plugin http request answered"
            );
            let done: Box<dyn Future<Output = Result<(), Error>> + Send> =
                Box::new(async { Ok(()) });
            Ok((response, done))
        })
    }
}

async fn send(request: Request, url: Url) -> Result<Response, Error> {
    let (parts, body) = request.into_parts();
    if parts.method == http::Method::CONNECT {
        return Err(Error::HttpRequestMethodInvalid);
    }
    let body = Limited::new(body, MAX_BODY_BYTES)
        .collect()
        .await
        .map_err(|_| Error::HttpRequestBodySize(Some(MAX_BODY_BYTES as u64)))?
        .to_bytes();
    let reply = guard::client()
        .request(parts.method, url)
        .headers(parts.headers)
        .body(body)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(request_error)?;
    let reply: http::Response<reqwest::Body> = reply.into();
    Ok(reply.map(|body| {
        Limited::new(body, MAX_BODY_BYTES)
            .map_err(body_error)
            .boxed_unsync()
    }))
}

/// Mapped by kind: reqwest's own message carries the full URL.
fn request_error(error: reqwest::Error) -> Error {
    if guard::was_refused(&error) {
        Error::DestinationIpProhibited
    } else if error.is_timeout() {
        Error::ConnectionTimeout
    } else if error.is_connect() {
        Error::DestinationUnavailable
    } else {
        Error::HttpProtocolError
    }
}

fn body_error(error: Box<dyn std::error::Error + Send + Sync>) -> Error {
    if error.is::<LengthLimitError>() {
        Error::HttpResponseBodySize(Some(MAX_BODY_BYTES as u64))
    } else {
        Error::HttpResponseIncomplete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(hosts: &[&str], connect: bool) -> Grant {
        Grant {
            permissions: connect
                .then_some(Permission::NetworkConnect)
                .into_iter()
                .collect(),
            network: hosts.iter().map(|host| host.to_string()).collect(),
            ..Grant::default()
        }
    }

    fn check(grant: &Grant, url: &str) -> Result<String, Refusal> {
        authorize(grant, &Url::parse(url).unwrap(), false)
    }

    #[test]
    fn a_plugin_without_the_permission_is_refused_even_for_a_listed_host() {
        assert_eq!(
            check(&grant(&["example.com"], false), "https://example.com/"),
            Err(Refusal::NotGranted)
        );
    }

    #[test]
    fn only_granted_hosts_over_https_pass() {
        let grant = grant(&["example.com", "*.cdn.example.org"], true);
        assert_eq!(
            check(&grant, "https://example.com/a?b=c"),
            Ok("example.com".into())
        );
        assert_eq!(
            check(&grant, "https://EXAMPLE.com./"),
            Ok("example.com".into())
        );
        assert_eq!(
            check(&grant, "https://x.cdn.example.org/"),
            Ok("x.cdn.example.org".into())
        );
        assert_eq!(
            check(&grant, "https://a.b.cdn.example.org/"),
            Ok("a.b.cdn.example.org".into())
        );
        assert_eq!(
            check(&grant, "https://example.com:8443/"),
            Ok("example.com".into())
        );
        for denied in [
            "https://cdn.example.org/",
            "https://evilcdn.example.org/",
            "https://cdn.example.org.evil.com/",
            "https://sub.example.com/",
            "https://notexample.com/",
        ] {
            assert!(
                matches!(check(&grant, denied), Err(Refusal::HostNotGranted(_))),
                "{denied}"
            );
        }
        assert_eq!(check(&grant, "http://example.com/"), Err(Refusal::NotHttps));
        assert_eq!(
            check(&grant, "https://user:pw@example.com/"),
            Err(Refusal::BadUrl)
        );
    }

    #[test]
    fn an_address_is_never_a_destination_even_when_listed() {
        let grant = grant(&["127.0.0.1", "::1", "*.0.1"], true);
        for literal in [
            "https://127.0.0.1/",
            "https://[::1]/",
            "https://[::ffff:127.0.0.1]/",
            "https://2130706433/",
            "https://0x7f.1/",
            "https://93.184.216.34/",
        ] {
            assert_eq!(check(&grant, literal), Err(Refusal::IpLiteral), "{literal}");
        }
    }

    #[test]
    fn a_download_needs_a_granted_host_over_https() {
        let grant = grant(&["files.example.com", "*.cdn.example.org", "127.0.0.1"], true);
        for allowed in [
            "https://files.example.com/a.jar",
            "https://x.cdn.example.org/a.jar",
            "http://127.0.0.1:8080/a.jar",
        ] {
            assert!(download_allowed(&grant, allowed), "{allowed}");
        }
        for denied in [
            "http://files.example.com/a.jar",
            "https://evil.example.com/a.jar",
            "https://files.example.com.evil.com/a.jar",
            "https://user:pw@files.example.com/a.jar",
            "https://cdn.example.org/a.jar",
            "http://localhost/a.jar",
            "ftp://files.example.com/a.jar",
            "not a url",
        ] {
            assert!(!download_allowed(&grant, denied), "{denied}");
        }
    }

    #[test]
    fn offline_refuses_a_granted_host() {
        let grant = grant(&["example.com"], true);
        let url = Url::parse("https://example.com/").unwrap();
        assert_eq!(authorize(&grant, &url, true), Err(Refusal::Offline));
    }

    #[test]
    fn each_refusal_maps_to_a_wasi_error() {
        assert!(matches!(
            Refusal::IpLiteral.code(),
            Error::DestinationIpProhibited
        ));
        assert!(matches!(Refusal::NotHttps.code(), Error::HttpRequestDenied));
        assert!(matches!(
            Refusal::Offline.code(),
            Error::DestinationUnavailable
        ));
    }
}
