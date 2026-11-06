//! The topic globs a plugin's `contributes.events` may name: an exact topic
//! (`job`, `process.exit`) or a prefix glob with a trailing `.*` (`server.*`).

fn is_topic(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
}

pub fn is_topic_glob(pattern: &str) -> bool {
    is_topic(pattern.strip_suffix(".*").unwrap_or(pattern))
}

/// `server.*` matches every topic beneath `server.` but not `server` itself.
pub fn topic_matches(pattern: &str, topic: &str) -> bool {
    match pattern.strip_suffix('*') {
        Some(prefix) => prefix.ends_with('.') && topic.starts_with(prefix),
        None => pattern == topic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_is_an_exact_topic_or_a_trailing_prefix() {
        for good in ["job", "server.*", "process.exit", "a.b.*"] {
            assert!(is_topic_glob(good), "{good}");
        }
        for bad in [
            "", "*", ".*", "server*", "a.*.b", "Server.*", "a b", "a.**", "a/*",
        ] {
            assert!(!is_topic_glob(bad), "{bad}");
        }
    }

    #[test]
    fn matching_is_exact_or_by_prefix() {
        assert!(topic_matches("server.*", "server.created"));
        assert!(topic_matches("server.*", "server.a.b"));
        assert!(!topic_matches("server.*", "server"));
        assert!(!topic_matches("server.*", "servers.x"));
        assert!(topic_matches("job", "job"));
        assert!(!topic_matches("job", "job.x"));
        assert!(!topic_matches("process.exit", "process.exited"));
    }
}
