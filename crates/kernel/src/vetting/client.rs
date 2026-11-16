use std::sync::OnceLock;
use std::time::Duration;

use reqwest::{Client, ClientBuilder, NoProxy, Proxy, Url};

use super::resolver::Guarded;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
const PROXY_VARIABLES: [&str; 4] = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"];

/// The host of a plain-http loopback URL, the one shape a test policy may grant.
pub(super) fn loopback_host(url: &Url) -> Option<String> {
    let host = url.host_str()?.to_ascii_lowercase();
    (url.scheme() == "http" && matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]"))
        .then_some(host)
}

/// How a vetted request leaves the machine.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Route {
    /// A test's plain-http loopback server.
    Loopback,
    /// The environment's proxy when one is set, else direct and guarded.
    Granted,
    /// Direct and guarded, never through a proxy.
    Open,
}

/// A policy that lists its hosts may go through the proxy the environment
/// names: the proxy resolves the target, so the guard cannot, but the target is
/// a name the policy granted. A policy open to any host goes direct, so a
/// user-supplied URL cannot reach what the proxy can see inside the network.
pub(super) fn route(url: &Url, lists_hosts: bool) -> Route {
    if loopback_host(url).is_some() {
        Route::Loopback
    } else if lists_hosts {
        Route::Granted
    } else {
        Route::Open
    }
}

/// The client a request on `route` goes out on.
pub(super) fn client(route: Route) -> &'static Client {
    match route {
        Route::Loopback => loopback(),
        Route::Granted => proxied().unwrap_or_else(direct),
        Route::Open => direct(),
    }
}

fn base() -> ClientBuilder {
    Client::builder()
        .user_agent(common::app::user_agent())
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(STALL_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
}

/// Cookies are off because the `cookies` feature is not built.
fn direct() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        base()
            .no_proxy()
            .dns_resolver(Guarded::default())
            .build()
            .expect("vetted reqwest client builds")
    })
}

/// The user's own proxy is trusted to be inside the network, so its name is the
/// one the guard resolves without vetting; a host `NO_PROXY` exempts is dialled
/// direct and guarded.
fn proxied() -> Option<&'static Client> {
    static CLIENT: OnceLock<Option<Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let url = proxy_setting(|name| std::env::var(name).ok())?;
            let proxy = Proxy::all(url.as_str())
                .inspect_err(|error| tracing::warn!(%error, "proxy setting ignored"))
                .ok()?
                .no_proxy(NoProxy::from_env());
            tracing::info!(
                host = url.host_str().unwrap_or_default(),
                "using the environment's proxy"
            );
            base()
                .proxy(proxy)
                .dns_resolver(Guarded::trusting(url.host_str().map(str::to_owned)))
                .build()
                .inspect_err(|error| tracing::warn!(%error, "proxy client unavailable"))
                .ok()
        })
        .as_ref()
}

/// The first non-empty proxy variable, as a URL; a bare `host:port` is http.
pub(super) fn proxy_setting(var: impl Fn(&str) -> Option<String>) -> Option<Url> {
    let raw = PROXY_VARIABLES
        .iter()
        .filter_map(|name| var(name))
        .find(|value| !value.trim().is_empty())?;
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("http://{raw}")
    };
    Url::parse(&with_scheme)
        .inspect_err(|error| tracing::warn!(%error, "proxy setting ignored"))
        .ok()
}

fn loopback() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        base()
            .no_proxy()
            .build()
            .expect("loopback reqwest client builds")
    })
}
