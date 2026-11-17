use std::future::Future;

use reqwest::header::LOCATION;
use reqwest::{Method, RequestBuilder, Response, Url};
use std::time::Duration;

use super::{client, Hosts, Refusal};

const MAX_REDIRECTS: usize = 5;

/// What one request may cost: the time until its body is read, and the bytes of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The whole exchange; `None` leaves only the stall timeout, which ends a
    /// connection that sends nothing for 30 s.
    pub time: Option<Duration>,
    /// The body, declared or streamed.
    pub bytes: u64,
}

impl Limits {
    /// A plugin's request, and a core API or metadata call.
    pub const REQUEST: Limits = Limits {
        time: Some(Duration::from_secs(30)),
        bytes: 32 * 1024 * 1024,
    };
    /// A core file download: a Java runtime or a game jar on a slow line is not
    /// a failure, so only the stall timeout and the size cap apply.
    pub const DOWNLOAD: Limits = Limits {
        time: None,
        bytes: 2 * 1024 * 1024 * 1024,
    };
    /// What one plugin's `/data` may hold in all.
    pub const DATA_QUOTA: u64 = 8 * 1024 * 1024 * 1024;
}

/// A request refused by the policy, or one that failed while being sent or read.
#[derive(Debug)]
pub enum Failure<E> {
    /// The policy refused the URL, a redirect or the body.
    Refused(Refusal),
    /// The caller's sender, or the body stream, failed.
    Send(E),
}

/// The hosts one caller may reach, and the checks every request it makes passes.
#[derive(Debug, Clone)]
pub struct Policy {
    hosts: Hosts,
}

impl Policy {
    pub fn new(hosts: Hosts) -> Self {
        Policy { hosts }
    }

    /// Whether a request to `url` may be made. `unhashed` is true when nothing
    /// verifies the body, which then has to come from a host declared for that.
    pub fn vet(&self, url: &Url, unhashed: bool) -> Result<(), Refusal> {
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Refusal::Credentials);
        }
        if let Some(host) = client::loopback_host(url) {
            return self.hosts.vet_loopback(host, unhashed);
        }
        if url.scheme() != "https" {
            return Err(Refusal::NotHttps);
        }
        let Some(domain) = url.domain() else {
            return Err(Refusal::IpLiteral);
        };
        let domain = domain.trim_end_matches('.').to_ascii_lowercase();
        if !self.hosts.reaches(&domain) {
            return Err(Refusal::HostNotGranted(domain));
        }
        if unhashed && !self.hosts.serves_unhashed(&domain) {
            return Err(Refusal::Unhashed(domain));
        }
        Ok(())
    }

    /// A request to an API host, which has no checksum to offer: the host must
    /// be declared as serving unhashed files. Redirects are not followed; a 3xx
    /// answer reaches the caller as it is. Read its body with [`read`].
    pub fn request(
        &self,
        method: Method,
        url: &str,
        limits: Limits,
    ) -> Result<RequestBuilder, Refusal> {
        self.vetted_request(method, url, limits, true)
    }

    /// [`Policy::request`] for a caller whose hosts are all granted by the
    /// user, such as a plugin: the host must be one of them, and nothing more.
    pub fn granted_request(
        &self,
        method: Method,
        url: &str,
        limits: Limits,
    ) -> Result<RequestBuilder, Refusal> {
        self.vetted_request(method, url, limits, false)
    }

    fn vetted_request(
        &self,
        method: Method,
        url: &str,
        limits: Limits,
        unhashed: bool,
    ) -> Result<RequestBuilder, Refusal> {
        let url = Url::parse(url).map_err(|_| Refusal::BadUrl)?;
        self.vet(&url, unhashed)?;
        Ok(self.builder(method, url, limits))
    }

    fn builder(&self, method: Method, url: Url, limits: Limits) -> RequestBuilder {
        let route = client::route(&url, self.hosts.lists_hosts());
        let builder = client::client(route).request(method, url);
        match limits.time {
            Some(time) => builder.timeout(time),
            None => builder,
        }
    }

    /// Fetches `url`, following redirects only to targets the policy allows;
    /// `unhashed` is held on every hop. `send` makes each hop's request, so a
    /// caller adds its retries and bookkeeping. The body is the caller's to read
    /// under `limits.bytes`, through [`read`] or its own stream.
    pub async fn get<S, F, E>(
        &self,
        url: &str,
        unhashed: bool,
        limits: Limits,
        mut send: S,
    ) -> Result<Response, Failure<E>>
    where
        S: FnMut(RequestBuilder) -> F,
        F: Future<Output = Result<Response, E>>,
    {
        let mut url = Url::parse(url).map_err(|_| Failure::Refused(Refusal::BadUrl))?;
        for _ in 0..=MAX_REDIRECTS {
            if let Err(refusal) = self.vet(&url, unhashed) {
                let host = url.host_str().unwrap_or_default();
                tracing::warn!(host, %refusal, "request refused");
                return Err(Failure::Refused(refusal));
            }
            let response = send(self.builder(Method::GET, url.clone(), limits))
                .await
                .map_err(Failure::Send)?;
            let target = match response.headers().get(LOCATION) {
                Some(location) if response.status().is_redirection() => location,
                _ => return Ok(response),
            };
            url = target
                .to_str()
                .ok()
                .and_then(|location| url.join(location).ok())
                .ok_or(Failure::Refused(Refusal::BadRedirect))?;
        }
        Err(Failure::Refused(Refusal::TooManyRedirects))
    }
}

/// The body of `response`, refused when it declares or streams past `limits.bytes`.
pub async fn read(
    mut response: Response,
    limits: Limits,
) -> Result<Vec<u8>, Failure<reqwest::Error>> {
    let too_large = || Failure::Refused(Refusal::TooLarge(limits.bytes));
    if response
        .content_length()
        .is_some_and(|length| length > limits.bytes)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Failure::Send)? {
        body.extend_from_slice(&chunk);
        if body.len() as u64 > limits.bytes {
            return Err(too_large());
        }
    }
    Ok(body)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
