//! What a plugin id may be. It names directories on every platform, so it is
//! held to the narrowest form that is safe everywhere.

const MAX_BYTES: usize = 64;

/// Device names Windows reserves in any directory, with or without an extension.
const RESERVED: [&str; 4] = ["con", "nul", "aux", "prn"];

/// Lowercase ASCII letters, digits and `-`, starting with a letter or digit, at
/// most 64 bytes, and not a name Windows reserves (`con`, `com1`, `lpt9`, ...).
pub fn is_valid(id: &str) -> bool {
    let shape = (1..=MAX_BYTES).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-');
    shape && !reserved(id)
}

fn reserved(id: &str) -> bool {
    let numbered = |prefix: &str| {
        id.strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
    };
    RESERVED.contains(&id) || numbered("com") || numbered("lpt")
}

#[cfg(test)]
mod tests {
    use super::is_valid;

    #[test]
    fn an_id_is_safe_as_a_directory_on_every_platform() {
        for id in ["tidy", "world-edit", "a1"] {
            assert!(is_valid(id), "{id}");
        }
        for id in [
            "",
            "-x",
            "Tidy",
            "a.b",
            "../x",
            "a/b",
            "a b",
            "con",
            "nul",
            "com1",
            "lpt9",
            &"a".repeat(65),
        ] {
            assert!(!is_valid(id), "{id}");
        }
    }
}
