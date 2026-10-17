//! The naming rules both sides of the socket resolve through, so the CLI and
//! the daemon can never disagree on which entry a bare reference names. Pure
//! functions, no I/O — the same no-drift role `contract` plays for payloads.

use serde_json::Value;

/// Reduce a display name to a filesystem-safe slug: lowercase alphanumeric runs
/// joined by single dashes. `None` when the name has no usable characters.
pub fn slugify(name: &str) -> Option<String> {
    let mut slug = String::new();
    let mut gap = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !slug.is_empty() {
                slug.push('-');
            }
            gap = false;
            slug.push(c.to_ascii_lowercase());
        } else {
            gap = true;
        }
    }
    (!slug.is_empty()).then_some(slug)
}

/// Does `reference` identify the entry with this `id`/`name`? Matches the exact
/// (opaque) id, or any spelling of the display name that slugs the same — so
/// `My Server`, `my-server`, and `MY  SERVER` all resolve to the one server
/// named "My Server". Unambiguous because entry names are slug-unique; an exact
/// id wins over a slugged name.
pub fn reference_matches(reference: &str, id: &str, name: &str) -> bool {
    if id == reference {
        return true;
    }
    matches!(
        (slugify(reference), slugify(name)),
        (Some(a), Some(b)) if a == b
    )
}

/// The supervisor key a managed server's process runs under — deterministic,
/// so every channel (and every front-end) can name a server's process from its
/// id alone, running or not.
pub fn server_process_id(id: &str) -> String {
    format!("server-{id}")
}

/// The instance *entry* key — the unit for the backup/content/update in-flight
/// sets and the lifecycle guards. Not a supervisor process key: an instance can
/// have many concurrent sessions, each keyed by [`instance_session_id`].
pub fn instance_process_id(id: &str) -> String {
    format!("instance-{id}")
}

/// The supervisor process key for one launch (session) of an instance. An id
/// never contains `_` (it is a uuid hex string), so the `_` separator keeps the
/// prefix `instance-<id>_` unambiguous across instances.
pub fn instance_session_id(id: &str, seq: u32) -> String {
    format!("instance-{id}_{seq}")
}

/// The prefix every session key of one instance shares.
pub fn instance_session_prefix(id: &str) -> String {
    format!("instance-{id}_")
}

/// The instance id embedded in a session key (`instance-<id>_<seq>`).
pub fn instance_id_of_session(session_id: &str) -> Option<String> {
    session_id
        .strip_prefix("instance-")
        .and_then(|rest| rest.rsplit_once('_'))
        .map(|(id, _seq)| id.to_string())
}

/// Does a subscription scoped to `key` cover events from process `id`? An entry
/// key covers itself and every session key beneath it (`instance-<id>_<seq>`),
/// so following an *entry* outlives the individual processes it runs — a
/// restart resumes the same stream. Job ids carry no `_`, so a job filter still
/// matches exactly one job.
pub fn process_in_scope(key: &str, id: &str) -> bool {
    id == key
        || id
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with('_'))
}

/// Translate a `config.*` key segment from its kebab-case vocabulary
/// (`jvm-args`) to the camelCase field the settings serialize as (`jvmArgs`).
/// The config keys are a deliberately stable kebab-case CLI vocabulary while
/// every serialized struct — settings included — is camelCase, so the
/// dotted-path get/set navigation translates each segment through here.
/// Single-word segments are unchanged.
pub fn config_key_to_field(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut upper = false;
    for c in segment.chars() {
        if c == '-' || c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Recursively rename object keys from the camelCase serialized form back to the
/// kebab-case `config.*` vocabulary, so `config list` presents the settings tree
/// in the keys a user sets. The per-key inverse of [`config_key_to_field`].
pub fn settings_to_config_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (field_to_config_key(&k), settings_to_config_keys(v)))
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(settings_to_config_keys).collect())
        }
        other => other,
    }
}

fn field_to_config_key(field: &str) -> String {
    let mut out = String::with_capacity(field.len() + 4);
    for c in field.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_normalizes_case_and_punctuation() {
        assert_eq!(slugify("My Server!").as_deref(), Some("my-server"));
        assert_eq!(slugify("  a__b  ").as_deref(), Some("a-b"));
        assert_eq!(slugify("!!!"), None);
    }

    #[test]
    fn reference_matches_id_exact_and_slugged_name() {
        let (id, name) = ("0192f3a45b6c7d8e9f00112233445566", "My Server");
        assert!(reference_matches(id, id, name), "exact id");
        assert!(reference_matches("My Server", id, name), "exact name");
        assert!(reference_matches("my-server", id, name), "slugged name");
        assert!(reference_matches("MY  SERVER", id, name), "loose spelling");
        assert!(!reference_matches("other", id, name));
    }

    #[test]
    fn a_sessions_prefix_never_matches_a_similarly_named_instance() {
        // Ids never contain `_`; using it as the session separator keeps one
        // instance's session prefix from matching another's sessions.
        let foo = instance_session_id("foo", 3);
        let foo_two = instance_session_id("foo-2", 1);
        assert!(foo.starts_with(&instance_session_prefix("foo")));
        assert!(!foo_two.starts_with(&instance_session_prefix("foo")));
        assert!(foo_two.starts_with(&instance_session_prefix("foo-2")));
    }

    #[test]
    fn session_seq_parses_back_off_the_prefix() {
        let id = instance_session_id("cozy", 7);
        let seq: u32 = id
            .strip_prefix(&instance_session_prefix("cozy"))
            .and_then(|s| s.parse().ok())
            .unwrap();
        assert_eq!(seq, 7);
        assert_eq!(instance_id_of_session(&id).as_deref(), Some("cozy"));
    }

    #[test]
    fn an_entry_key_scopes_its_own_sessions_only() {
        let entry = instance_process_id("foo");
        assert!(process_in_scope(&entry, &entry));
        assert!(process_in_scope(&entry, &instance_session_id("foo", 2)));
        assert!(!process_in_scope(&entry, &instance_session_id("foo-2", 1)));
        assert!(!process_in_scope(&entry, &instance_process_id("foo-2")));
        let server = server_process_id("abc");
        assert!(process_in_scope(&server, &server));
        assert!(!process_in_scope(&server, &server_process_id("abcd")));
        // Job ids share a prefix vocabulary but never a `_`.
        assert!(!process_in_scope("content-42", "content-42-7"));
    }

    #[test]
    fn config_key_translates_to_camel_field_and_back() {
        assert_eq!(config_key_to_field("jvm-args"), "jvmArgs");
        assert_eq!(config_key_to_field("memory"), "memory");
        let camel = serde_json::json!({ "defaults": { "jvmArgs": "-Xss1m", "memory": "4G" } });
        assert_eq!(
            settings_to_config_keys(camel),
            serde_json::json!({ "defaults": { "jvm-args": "-Xss1m", "memory": "4G" } })
        );
    }
}
