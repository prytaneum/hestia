//! Only the sandbox's `Policy` reads what `Hosts` matches; without it `derive` just validates.
#![cfg_attr(not(feature = "sandbox"), allow(dead_code))]

use std::net::IpAddr;

use super::Refusal;

/// A host entry that cannot be granted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostError {
    #[error("`{0}` is not a host name")]
    Malformed(String),
    #[error("`{0}` is an IP address; grant a host name")]
    IpLiteral(String),
    #[error("`{0}` would match every name under a public suffix")]
    PublicSuffix(String),
}

#[derive(Debug, Clone)]
enum Allowed {
    Any,
    Only(Vec<String>),
}

impl Allowed {
    fn matches(&self, host: &str) -> bool {
        match self {
            Allowed::Any => true,
            Allowed::Only(entries) => entries.iter().any(|entry| matches(entry, host)),
        }
    }
}

/// The hosts a request may reach, and those that may also serve a body nothing
/// verifies. An entry is an exact name or `*.domain`, which matches the names
/// exactly one label below `domain`. Wildcards serve plugin grants, which name
/// a service's CDN rather than each of its hosts.
#[derive(Debug, Clone)]
pub struct Hosts {
    connect: Allowed,
    unhashed: Allowed,
    /// Plain-http loopback, granted only by a test policy; `Some(true)` also
    /// lets its files carry no checksum.
    loopback: Option<bool>,
}

impl Hosts {
    /// The named hosts, each validated: no IP address, a well-formed name, and
    /// no wildcard over a public suffix (`*.com`, `*.github.io`).
    pub fn new<S: AsRef<str>>(connect: &[S], unhashed: &[S]) -> Result<Self, HostError> {
        let entries = |list: &[S]| {
            list.iter()
                .map(|e| entry(e.as_ref()))
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(Hosts {
            connect: Allowed::Only(entries(connect)?),
            unhashed: Allowed::Only(entries(unhashed)?),
            loopback: None,
        })
    }

    /// Any public host, for a URL the user or untrusted content supplied. A body
    /// must still carry a checksum.
    pub fn open() -> Self {
        Hosts {
            connect: Allowed::Any,
            unhashed: Allowed::Only(Vec::new()),
            loopback: None,
        }
    }

    /// [`Hosts::open`] without the checksum requirement, for what cannot have
    /// one: a page, an image.
    pub fn open_unhashed() -> Self {
        Hosts {
            connect: Allowed::Any,
            unhashed: Allowed::Any,
            loopback: None,
        }
    }

    /// These hosts plus plain-http loopback, for tests that serve files locally;
    /// `unhashed` lets those files carry no checksum. Nothing else reaches
    /// loopback, and only a test build has this.
    #[cfg(any(test, feature = "loopback"))]
    pub fn with_loopback(self, unhashed: bool) -> Self {
        Hosts {
            loopback: Some(unhashed),
            ..self
        }
    }

    pub(super) fn lists_hosts(&self) -> bool {
        matches!(self.connect, Allowed::Only(_))
    }

    pub(super) fn reaches(&self, host: &str) -> bool {
        self.connect.matches(host)
    }

    pub(super) fn serves_unhashed(&self, host: &str) -> bool {
        self.unhashed.matches(host)
    }

    pub(super) fn vet_loopback(&self, host: String, unhashed: bool) -> Result<(), Refusal> {
        match self.loopback {
            None => Err(Refusal::NotHttps),
            Some(false) if unhashed => Err(Refusal::Unhashed(host)),
            Some(_) => Ok(()),
        }
    }
}

fn matches(entry: &str, host: &str) -> bool {
    match entry.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .and_then(|rest| rest.strip_suffix('.'))
            .is_some_and(|label| !label.is_empty() && !label.contains('.')),
        None => host == entry,
    }
}

fn entry(raw: &str) -> Result<String, HostError> {
    let host = raw.trim_end_matches('.').to_ascii_lowercase();
    if host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() {
        return Err(HostError::IpLiteral(raw.to_string()));
    }
    let (wildcard, name) = match host.strip_prefix("*.") {
        Some(name) => (true, name),
        None => (false, host.as_str()),
    };
    if !is_name(name) {
        return Err(HostError::Malformed(raw.to_string()));
    }
    let is_suffix =
        psl::suffix(name.as_bytes()).is_some_and(|suffix| suffix.as_bytes().len() == name.len());
    if wildcard && is_suffix {
        return Err(HostError::PublicSuffix(raw.to_string()));
    }
    Ok(host)
}

fn is_name(name: &str) -> bool {
    let label = |label: &str| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    name.len() <= 253 && name.contains('.') && name.split('.').all(label)
}
