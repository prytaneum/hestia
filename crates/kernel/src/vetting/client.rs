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

/// The client a vetted request goes out on. A policy that lists its hosts may
/// go through the proxy the environment names: the proxy resolves the target,
/// so the guard cannot, but the target is a name the policy granted. A policy
/// open to any host goes direct, so a user-supplied URL cannot reach what the
/// proxy can see inside the network.
pub(super) fn for_url(url: &Url, lists_hosts: bool) -> &'static Client {
    if loopback_host(url).is_some() {
        return loopback();
    }
    if lists_hosts {
        if let Some(client) = proxied() {
            return client;
        }
    }
    direct()
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
            let url = env_proxy()?;
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

fn env_proxy() -> Option<Url> {
    let raw = PROXY_VARIABLES
        .iter()
        .filter_map(|name| std::env::var(name).ok())
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
