//! The one rule for an engine download of a URL a plugin supplied: it is fetched
//! through the guarded client, over https, and every hop of its redirect chain
//! is held to the plugin's grant again. A plugin-backed provider owns a
//! [`Policy`] built from its grant; whatever downloads what that provider
//! returned fetches it through [`Policy::get`].

use std::sync::OnceLock;
use std::time::Duration;

use proto::error::ErrorInfo;
use proto::minecraft::Artifact;
use reqwest::header::LOCATION;
use reqwest::{Response, Url};

use crate::net::{self, guard};

const MAX_REDIRECTS: usize = 5;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a request to a plugin-supplied destination was not made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("only https requests are allowed")]
    NotHttps,
    #[error("the request URL is not valid")]
    BadUrl,
    #[error("an IP address cannot be a destination")]
    IpLiteral,
    #[error("`{0}` is not one of the hosts this download may use")]
    HostNotGranted(String),
    #[error("the daemon is offline")]
    Offline,
}

/// The destination host, lowercased and without a trailing dot, when `hosts`
/// allow a request to `url` while the daemon is (not) `offline`.
pub fn authorize(hosts: &Hosts, url: &Url, offline: bool) -> Result<String, Refusal> {
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
    if !hosts.reaches(&domain) {
        return Err(Refusal::HostNotGranted(domain));
    }
    if offline {
        return Err(Refusal::Offline);
    }
    Ok(domain)
}

/// Why a plugin-supplied download was stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("{0}")]
    Destination(Refusal),
    #[error("`{0}` is not a host the plugin declared as serving unhashed files")]
    Unhashed(String),
    #[error("`{0}` names no well-formed checksum and is not declared unhashed by a host that may serve one")]
    Unverified(String),
    #[error("the redirect target is not a valid URL")]
    BadRedirect,
    #[error("too many redirects")]
    TooManyRedirects,
}

impl From<Refused> for ErrorInfo {
    fn from(refused: Refused) -> Self {
        ErrorInfo::DownloadFailed {
            detail: refused.to_string(),
        }
    }
}

/// The hosts a download may come from: those that may be reached at all, and
/// those that may also serve a file nothing verifies. Each entry is an exact
/// name or `*.domain`, matching the names exactly one label below it.
#[derive(Debug, Clone, Default)]
pub struct Hosts {
    pub connect: Vec<String>,
    pub unhashed: Vec<String>,
}

/// Whether the granted host `entry` reaches `host`, which is lowercase and has
/// no trailing dot: an exact name, or for `*.domain` a name exactly one label
/// below `domain`, never `domain` itself nor anything deeper.
fn network_matches(entry: &str, host: &str) -> bool {
    let entry = entry.trim_end_matches('.').to_ascii_lowercase();
    match entry.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .and_then(|rest| rest.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty() && !label.contains('.')),
        None => host == entry,
    }
}

impl Hosts {
    fn reaches(&self, host: &str) -> bool {
        self.connect
            .iter()
            .any(|entry| network_matches(entry, host))
    }

    fn serves_unhashed(&self, host: &str) -> bool {
        self.unhashed
            .iter()
            .any(|entry| network_matches(entry, host))
    }
}

/// What the engine may fetch for a source that names its own files: the hosts
/// it declared, plus a loopback host they name when the engine is built with
/// the `loopback` feature (its own tests), which no build that ships enables.
#[derive(Debug)]
pub struct Policy {
    hosts: Hosts,
    loopback: bool,
}

impl Policy {
    pub fn new(hosts: &Hosts) -> Self {
        Policy::build(hosts, cfg!(feature = "loopback"))
    }

    fn build(hosts: &Hosts, loopback: bool) -> Self {
        Policy {
            hosts: hosts.clone(),
            loopback,
        }
    }

    /// Whether `url` may be fetched under this policy. `unhashed` is true when
    /// nothing verifies the body, which then has to come from a host the grant
    /// declares as serving unhashed files.
    pub fn permits(&self, url: &str, unhashed: bool) -> bool {
        self.vet(url, unhashed).is_ok()
    }

    /// [`Policy::permits`] with the reason, for a caller that answers typed.
    pub fn vet(&self, url: &str, unhashed: bool) -> Result<(), Refused> {
        let url = Url::parse(url).map_err(|_| Refused::Destination(Refusal::BadUrl))?;
        self.check(&url, unhashed)
    }

    /// Whether the engine may fetch `artifact`: its URL is on a granted host
    /// over https, and its body is verified, by a well-formed checksum or by
    /// being declared unhashed from a host the grant allows that of.
    pub fn vet_artifact(&self, artifact: &Artifact) -> Result<(), Refused> {
        self.vet(&artifact.url, false)?;
        match &artifact.checksum {
            Some(checksum) if checksum.is_valid() => Ok(()),
            None if artifact.unhashed => self.vet(&artifact.url, true),
            _ => Err(Refused::Unverified(artifact.filename.clone())),
        }
    }

    fn check(&self, url: &Url, unhashed: bool) -> Result<(), Refused> {
        let host = if self.loopback && is_loopback(url) {
            let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
            if !self.hosts.reaches(&host) {
                return Err(Refused::Destination(Refusal::HostNotGranted(host)));
            }
            host
        } else {
            authorize(&self.hosts, url, false).map_err(Refused::Destination)?
        };
        if unhashed && !self.hosts.serves_unhashed(&host) {
            return Err(Refused::Unhashed(host));
        }
        Ok(())
    }

    fn client(&self, url: &Url) -> &'static reqwest::Client {
        if self.loopback && is_loopback(url) {
            local_client()
        } else {
            guard::client()
        }
    }

    /// Fetches `url`, following redirects only to targets the grant allows;
    /// `unhashed` is held on every hop.
    pub async fn get(&self, url: &str, unhashed: bool) -> anyhow::Result<Response> {
        let mut url =
            Url::parse(url).map_err(|_| refusal(Refused::Destination(Refusal::BadUrl)))?;
        for _ in 0..=MAX_REDIRECTS {
            self.check(&url, unhashed).map_err(|refused| {
                tracing::warn!(host = url.host_str().unwrap_or_default(), %refused, "plugin download refused");
                refusal(refused)
            })?;
            let response = net::send(None, self.client(&url).get(url.clone())).await?;
            if !response.status().is_redirection() {
                return Ok(response);
            }
            let Some(location) = response.headers().get(LOCATION) else {
                return Ok(response);
            };
            url = location
                .to_str()
                .ok()
                .and_then(|location| url.join(location).ok())
                .ok_or_else(|| refusal(Refused::BadRedirect))?;
        }
        Err(refusal(Refused::TooManyRedirects))
    }
}

/// The failure as an [`ErrorInfo`] with the typed reason attached, so a caller
/// that answers a plugin can tell a refusal from a transfer failure.
fn refusal(refused: Refused) -> anyhow::Error {
    anyhow::Error::new(ErrorInfo::from(refused.clone())).context(refused)
}

fn is_loopback(url: &Url) -> bool {
    matches!(
        url.host_str().map(str::to_ascii_lowercase).as_deref(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    )
}

fn local_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(common::app::user_agent())
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .expect("loopback reqwest client builds")
    })
}

#[cfg(test)]
mod tests;
