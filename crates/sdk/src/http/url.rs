use std::fmt;

use crate::Error;

/// Percent-encodes `text` for use as one path segment, one query key or one
/// query value: every byte but `A-Z a-z 0-9 - . _ ~` becomes `%XX`, so `/`, `?`,
/// `#`, `&`, `=`, `+`, spaces and non-ASCII UTF-8 all stay inside the component.
///
/// ```
/// assert_eq!(hestia::http::encode("a b&c/é"), "a%20b%26c%2F%C3%A9");
/// ```
pub fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// An `https://` URL built from a base, path segments and query pairs, each
/// encoded for the place it goes.
///
/// ```
/// use hestia::http::Url;
///
/// let url = Url::new("https://api.example.com/v2/")?
///     .segment("project")
///     .segment("a/b")
///     .query("query", "fabric api")
///     .query("limit", 5);
/// assert_eq!(
///     url.to_string(),
///     "https://api.example.com/v2/project/a%2Fb?query=fabric%20api&limit=5"
/// );
/// # Ok::<(), hestia::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    text: String,
    has_query: bool,
}

impl Url {
    /// Starts from `https://host` or `https://host/some/path`. A base with a
    /// query or a fragment, or without a host, is [`Error::Invalid`].
    pub fn new(base: impl AsRef<str>) -> Result<Self, Error> {
        let base = base.as_ref();
        let host = base
            .strip_prefix("https://")
            .map(|rest| rest.split('/').next().unwrap_or_default())
            .filter(|host| !host.is_empty());
        if host.is_none() || base.contains(['?', '#']) {
            return Err(Error::Invalid(format!(
                "`{base}` is not an https:// base URL without a query or fragment"
            )));
        }
        Ok(Url {
            text: base.trim_end_matches('/').to_string(),
            has_query: false,
        })
    }

    /// Appends one path segment.
    pub fn segment(mut self, segment: impl AsRef<str>) -> Self {
        self.text.push('/');
        self.text.push_str(&encode(segment.as_ref()));
        self
    }

    /// Appends each of `segments` in order.
    pub fn segments<S: AsRef<str>>(self, segments: impl IntoIterator<Item = S>) -> Self {
        segments
            .into_iter()
            .fold(self, |url, segment| url.segment(segment))
    }

    /// Appends one `key=value` pair; the same key may repeat.
    pub fn query(mut self, key: impl AsRef<str>, value: impl ToString) -> Self {
        self.text.push(if self.has_query { '&' } else { '?' });
        self.has_query = true;
        self.text.push_str(&encode(key.as_ref()));
        self.text.push('=');
        self.text.push_str(&encode(&value.to_string()));
        self
    }
}

impl fmt::Display for Url {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_keeps_unreserved_and_escapes_the_rest() {
        let vectors = [
            ("", ""),
            ("abc-._~019", "abc-._~019"),
            (" ", "%20"),
            ("a&b=c+d", "a%26b%3Dc%2Bd"),
            ("a/b?c#d", "a%2Fb%3Fc%23d"),
            ("100%", "100%25"),
            ("é", "%C3%A9"),
            ("日本", "%E6%97%A5%E6%9C%AC"),
            ("😀", "%F0%9F%98%80"),
        ];
        for (raw, expected) in vectors {
            assert_eq!(encode(raw), expected, "{raw}");
        }
    }

    #[test]
    fn a_url_is_built_from_its_parts() {
        let url = Url::new("https://api.example.com")
            .unwrap()
            .segments(["v1", "a b", "x/y"])
            .query("q", "a&b=c")
            .query("q", "ü")
            .query("n", 3);
        assert_eq!(
            url.to_string(),
            "https://api.example.com/v1/a%20b/x%2Fy?q=a%26b%3Dc&q=%C3%BC&n=3"
        );
    }

    #[test]
    fn a_base_keeps_its_path_and_loses_a_trailing_slash() {
        let url = Url::new("https://example.com/api/").unwrap().segment("v1");
        assert_eq!(url.to_string(), "https://example.com/api/v1");
    }

    #[test]
    fn a_bad_base_is_refused() {
        for base in [
            "http://example.com",
            "example.com",
            "https://",
            "https:///x",
            "https://example.com?a=1",
            "https://example.com/#top",
        ] {
            assert!(matches!(Url::new(base), Err(Error::Invalid(_))), "{base}");
        }
    }
}
