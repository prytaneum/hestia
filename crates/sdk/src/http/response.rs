use serde::de::DeserializeOwned;

use super::Headers;
use crate::Error;

const SNIPPET_BYTES: usize = 1024;
const SNIPPET_CHARS: usize = 200;

/// What a server answered. Redirects are not followed, so a `3xx` is a response
/// like any other.
#[derive(Debug, Clone, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    /// `1xx`.
    pub fn is_informational(&self) -> bool {
        (100..200).contains(&self.status)
    }

    /// `2xx`.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// `3xx`.
    pub fn is_redirection(&self) -> bool {
        (300..400).contains(&self.status)
    }

    /// `4xx`.
    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }

    /// `5xx`.
    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    /// The body as UTF-8 text; anything else is [`Error::Invalid`].
    pub fn text(&self) -> Result<&str, Error> {
        std::str::from_utf8(&self.body)
            .map_err(|error| Error::Invalid(format!("the body is not UTF-8: {error}")))
    }

    /// The body decoded as JSON; a body that does not decode is [`Error::Json`].
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        Ok(serde_json::from_slice(&self.body)?)
    }

    /// Seconds from a `Retry-After` header in its delay form. The HTTP-date
    /// form is not read and answers `None`.
    pub fn retry_after(&self) -> Option<u32> {
        self.headers.get("retry-after")?.trim().parse().ok()
    }

    /// Passes a `2xx` response through and turns any other status into the
    /// error that says what happened:
    ///
    /// | Status | Error |
    /// |---|---|
    /// | `404` | [`Error::NotFound`] |
    /// | `429` | [`Error::RateLimited`] with `Retry-After` |
    /// | any other, `3xx`, `4xx` or `5xx` | [`Error::Upstream`] with the status, `Retry-After` and a message of `HTTP <status>` plus a short snippet of a text body |
    ///
    /// `401` and `403` stay [`Error::Upstream`]: the upstream says no, and only
    /// the plugin knows whether that means a wrong API key
    /// ([`Error::NotConfigured`]), a missing sign-in ([`Error::SignInRequired`]) or
    /// something else. Map it where it is known:
    ///
    /// ```no_run
    /// use hestia::Error;
    ///
    /// let reply = hestia::http::get("https://api.example.com/me")?;
    /// let reply = match reply.error_for_status() {
    ///     Err(Error::Upstream { status: Some(401 | 403), .. }) => {
    ///         return Err(Error::NotConfigured("api-key".into()))
    ///     }
    ///     other => other?,
    /// };
    /// # Ok::<(), Error>(())
    /// ```
    pub fn error_for_status(self) -> Result<Self, Error> {
        if self.is_success() {
            Ok(self)
        } else {
            Err(self.status_error())
        }
    }

    fn status_error(&self) -> Error {
        let retry_after = self.retry_after();
        match self.status {
            404 => Error::NotFound(self.message()),
            429 => Error::RateLimited { retry_after },
            status => Error::Upstream {
                status: Some(status),
                retry_after,
                message: self.message(),
            },
        }
    }

    fn message(&self) -> String {
        let head = format!("HTTP {}", self.status);
        match self.snippet() {
            Some(snippet) => format!("{head}: {snippet}"),
            None => head,
        }
    }

    fn snippet(&self) -> Option<String> {
        let readable = match self.headers.get("content-type") {
            Some(kind) => {
                let kind = kind.to_ascii_lowercase();
                kind.starts_with("text/") || kind.contains("json") || kind.contains("xml")
            }
            None => true,
        };
        if !readable || self.body.is_empty() {
            return None;
        }
        let head = &self.body[..self.body.len().min(SNIPPET_BYTES)];
        let squeezed = String::from_utf8_lossy(head)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut snippet: String = squeezed.chars().take(SNIPPET_CHARS).collect();
        let cut = squeezed.chars().count() > SNIPPET_CHARS || head.len() < self.body.len();
        snippet = snippet.trim_end_matches('\u{FFFD}').to_string();
        if snippet.is_empty() {
            return None;
        }
        if cut {
            snippet.push('…');
        }
        Some(snippet)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
        Response {
            status,
            headers: headers.iter().copied().collect(),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn status_classes() {
        let class = |status| {
            let reply = reply(status, &[], "");
            (
                reply.is_informational(),
                reply.is_success(),
                reply.is_redirection(),
                reply.is_client_error(),
                reply.is_server_error(),
            )
        };
        assert_eq!(class(101), (true, false, false, false, false));
        assert_eq!(class(204), (false, true, false, false, false));
        assert_eq!(class(302), (false, false, true, false, false));
        assert_eq!(class(418), (false, false, false, true, false));
        assert_eq!(class(503), (false, false, false, false, true));
    }

    #[test]
    fn a_success_passes_through() {
        assert_eq!(
            reply(200, &[], "ok").error_for_status().unwrap().status,
            200
        );
        assert!(reply(204, &[], "").error_for_status().is_ok());
    }

    #[test]
    fn not_found_is_not_found() {
        let error = reply(404, &[], "").error_for_status().unwrap_err();
        assert!(matches!(&error, Error::NotFound(message) if message == "HTTP 404"));
    }

    #[test]
    fn too_many_requests_carries_the_wait() {
        let waiting = reply(429, &[("Retry-After", " 30 ")], "").error_for_status();
        assert!(matches!(
            waiting,
            Err(Error::RateLimited {
                retry_after: Some(30)
            })
        ));
        let date = reply(429, &[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")], "");
        assert!(matches!(
            date.error_for_status(),
            Err(Error::RateLimited { retry_after: None })
        ));
    }

    #[test]
    fn credentials_failures_stay_upstream_for_the_plugin_to_map() {
        for status in [401, 403] {
            let error = reply(status, &[], "").error_for_status().unwrap_err();
            assert!(matches!(
                error,
                Error::Upstream { status: Some(code), retry_after: None, .. } if code == status
            ));
        }
    }

    #[test]
    fn other_failures_are_upstream_with_a_snippet() {
        let error = reply(
            503,
            &[("content-type", "text/plain"), ("retry-after", "5")],
            "down\n  for   maintenance",
        )
        .error_for_status()
        .unwrap_err();
        let Error::Upstream {
            status,
            retry_after,
            message,
        } = error
        else {
            panic!("{error:?}")
        };
        assert_eq!(status, Some(503));
        assert_eq!(retry_after, Some(5));
        assert_eq!(message, "HTTP 503: down for maintenance");

        let redirect = reply(301, &[("location", "https://elsewhere")], "");
        assert!(matches!(
            redirect.error_for_status(),
            Err(Error::Upstream {
                status: Some(301),
                ..
            })
        ));
    }

    #[test]
    fn a_snippet_is_bounded_and_skips_binary_bodies() {
        let long = "x".repeat(5000);
        let message = reply(500, &[("content-type", "application/json")], &long)
            .error_for_status()
            .unwrap_err()
            .to_string();
        assert_eq!(
            message.chars().count(),
            "HTTP 500: ".len() + SNIPPET_CHARS + 1
        );
        assert!(message.ends_with('…'));

        let binary = reply(500, &[("content-type", "image/png")], "not text");
        assert_eq!(
            binary.error_for_status().unwrap_err().to_string(),
            "HTTP 500"
        );
    }

    #[test]
    fn a_snippet_never_splits_a_character() {
        let body = "é".repeat(2000);
        let message = reply(500, &[], &body)
            .error_for_status()
            .unwrap_err()
            .to_string();
        assert!(!message.contains('\u{FFFD}'));
    }

    #[test]
    fn text_and_json_decode_the_body() {
        let response = reply(200, &[], r#"{"a":1}"#);
        assert_eq!(response.text().unwrap(), r#"{"a":1}"#);
        let value: serde_json::Value = response.json().unwrap();
        assert_eq!(value["a"], 1);
        assert!(matches!(
            reply(200, &[], "nope").json::<serde_json::Value>(),
            Err(Error::Json(_))
        ));
        let binary = Response {
            body: vec![0xff, 0xfe],
            ..Response::default()
        };
        assert!(matches!(binary.text(), Err(Error::Invalid(_))));
    }
}
