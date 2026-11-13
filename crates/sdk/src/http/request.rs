use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{encode, transport, Headers, Response};
use crate::Error;

/// An HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

/// A request being built. Start from the method, add headers and a body, then
/// [`send`](Request::send) it.
///
/// ```no_run
/// use hestia::http::{Request, Url};
///
/// let url = Url::new("https://api.example.com")?.segment("search");
/// let hits: Vec<String> = Request::get(url)
///     .header("authorization", "Bearer token")
///     .send_json()?;
///
/// let created = Request::post("https://api.example.com/items")
///     .json(&hestia::json!({ "name": "smp" }))?
///     .send()?
///     .error_for_status()?;
/// # Ok::<(), hestia::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Request {
    pub(crate) method: Method,
    pub(crate) url: String,
    pub(crate) headers: Headers,
    pub(crate) body: Vec<u8>,
}

impl Request {
    /// A request with `method` to `url`, which may be a [`Url`](super::Url).
    pub fn new(method: Method, url: impl ToString) -> Self {
        Request {
            method,
            url: url.to_string(),
            headers: Headers::default(),
            body: Vec::new(),
        }
    }

    pub fn get(url: impl ToString) -> Self {
        Self::new(Method::Get, url)
    }

    pub fn head(url: impl ToString) -> Self {
        Self::new(Method::Head, url)
    }

    pub fn post(url: impl ToString) -> Self {
        Self::new(Method::Post, url)
    }

    pub fn put(url: impl ToString) -> Self {
        Self::new(Method::Put, url)
    }

    pub fn patch(url: impl ToString) -> Self {
        Self::new(Method::Patch, url)
    }

    pub fn delete(url: impl ToString) -> Self {
        Self::new(Method::Delete, url)
    }

    /// Sets a header, replacing any of the same name.
    pub fn header(mut self, name: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.headers.set(name, value);
        self
    }

    /// Sets the body to `bytes` of `content_type`.
    pub fn body(self, content_type: &str, bytes: impl Into<Vec<u8>>) -> Self {
        let mut request = self.header("content-type", content_type);
        request.body = bytes.into();
        request
    }

    /// Sets the body to `value` as JSON; a value that does not encode is
    /// [`Error::Json`].
    pub fn json<T: Serialize>(self, value: &T) -> Result<Self, Error> {
        Ok(self.body("application/json", serde_json::to_vec(value)?))
    }

    /// Sets the body to `pairs` as `application/x-www-form-urlencoded`.
    pub fn form<K: AsRef<str>, V: AsRef<str>>(
        self,
        pairs: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        let body = pairs
            .into_iter()
            .map(|(key, value)| format!("{}={}", encode(key.as_ref()), encode(value.as_ref())))
            .collect::<Vec<_>>()
            .join("&");
        self.body("application/x-www-form-urlencoded", body)
    }

    /// Sends it and returns whatever status came back. Only a request that
    /// could not be made is an error: [`Error::Denied`] for a host the manifest
    /// does not grant, [`Error::Offline`] for one that cannot be reached.
    pub fn send(self) -> Result<Response, Error> {
        transport::send(self)
    }

    /// Sends it, requires a `2xx` ([`Response::error_for_status`]) and decodes
    /// the body as JSON.
    pub fn send_json<T: DeserializeOwned>(self) -> Result<T, Error> {
        self.send()?.error_for_status()?.json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_body_sets_its_content_type() {
        let request = Request::post("https://example.com")
            .json(&serde_json::json!({ "a": 1 }))
            .unwrap();
        assert_eq!(
            request.headers.get("Content-Type"),
            Some("application/json")
        );
        assert_eq!(request.body, br#"{"a":1}"#);
    }

    #[test]
    fn a_form_body_is_encoded() {
        let request = Request::post("https://example.com").form([("q", "a b&c"), ("k", "é")]);
        assert_eq!(request.body, b"q=a%20b%26c&k=%C3%A9");
        assert_eq!(
            request.headers.get("content-type"),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn a_header_replaces_the_one_before() {
        let request = Request::get("https://example.com")
            .header("Accept", "a")
            .header("accept", "b");
        assert_eq!(request.headers.get_all("accept").collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn a_url_builder_is_accepted_as_the_url() {
        let url = super::super::Url::new("https://example.com")
            .unwrap()
            .segment("a b");
        assert_eq!(Request::get(url).url, "https://example.com/a%20b");
    }
}
