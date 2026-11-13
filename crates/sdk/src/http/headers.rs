/// The headers of a request or a response. Names compare case-insensitively
/// and a name may repeat, so a lookup says whether it wants the first value or
/// all of them.
///
/// ```
/// let mut headers = hestia::http::Headers::default();
/// headers.append("Set-Cookie", "a=1");
/// headers.append("set-cookie", "b=2");
/// assert_eq!(headers.get("SET-COOKIE"), Some("a=1"));
/// assert_eq!(headers.get_all("set-cookie").count(), 2);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers(Vec<(String, String)>);

impl Headers {
    /// Adds a header after any of the same name.
    pub fn append(&mut self, name: impl AsRef<str>, value: impl Into<String>) {
        self.0
            .push((name.as_ref().to_ascii_lowercase(), value.into()));
    }

    /// Replaces every header of that name with one value.
    pub fn set(&mut self, name: impl AsRef<str>, value: impl Into<String>) {
        let name = name.as_ref().to_ascii_lowercase();
        self.0.retain(|(existing, _)| *existing != name);
        self.0.push((name, value.into()));
    }

    /// The first value of `name`, whatever its case.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.get_all(name).next()
    }

    /// Every value of `name`, in the order they arrived.
    pub fn get_all<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> {
        let name = name.to_ascii_lowercase();
        self.0
            .iter()
            .filter(move |(existing, _)| *existing == name)
            .map(|(_, value)| value.as_str())
    }

    /// Every header as a lowercase name and its value.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<N: AsRef<str>, V: Into<String>> FromIterator<(N, V)> for Headers {
    fn from_iter<I: IntoIterator<Item = (N, V)>>(pairs: I) -> Self {
        let mut headers = Headers::default();
        for (name, value) in pairs {
            headers.append(name, value);
        }
        headers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_ignores_case_and_keeps_every_value() {
        let headers: Headers = [("Link", "<a>"), ("link", "<b>"), ("Etag", "x")]
            .into_iter()
            .collect();
        assert_eq!(headers.get("LINK"), Some("<a>"));
        assert_eq!(headers.get_all("Link").collect::<Vec<_>>(), ["<a>", "<b>"]);
        assert_eq!(headers.get("missing"), None);
    }

    #[test]
    fn set_replaces_every_earlier_value() {
        let mut headers: Headers = [("Accept", "a"), ("accept", "b")].into_iter().collect();
        headers.set("ACCEPT", "c");
        assert_eq!(headers.get_all("accept").collect::<Vec<_>>(), ["c"]);
    }
}
