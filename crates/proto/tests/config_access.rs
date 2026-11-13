//! The plugin access table: closed by default, matched in canonical form, a
//! parent only as open as its least open key, and reads filtered per key.

use proto::config::{plugin_may_read, plugin_may_write, plugin_view, PluginAccess, PLUGIN_ACCESS};
use serde_json::json;

#[test]
fn the_table_lists_each_key_once() {
    let mut keys: Vec<&str> = PLUGIN_ACCESS.iter().map(|(key, _)| *key).collect();
    keys.sort_unstable();
    let before = keys.len();
    keys.dedup();
    assert_eq!(before, keys.len());
}

#[test]
fn reserved_and_sensitive_keys_stay_closed() {
    for key in [
        "home",
        "autostart",
        "developer",
        "defaults.jvm-args",
        "content.curseforge-key",
        "update.channel",
        "network.offline",
        "java.provider",
        "plugins.auto-update",
    ] {
        assert!(!plugin_may_read(key), "{key} readable");
        assert!(!plugin_may_write(key), "{key} writable");
    }
}

#[test]
fn an_unlisted_key_is_closed() {
    for key in [
        "",
        "no.such.key",
        "defaults.memory.extra",
        "defaults..memory",
    ] {
        assert!(!plugin_may_write(key), "{key:?} writable");
    }
    assert!(!plugin_may_read("no.such.key"));
}

#[test]
fn a_key_matches_in_canonical_form() {
    assert_eq!(
        plugin_may_read("defaults.jvmArgs"),
        plugin_may_read("defaults.jvm-args")
    );
    assert!(!plugin_may_write("defaults.jvmArgs"));
    assert!(plugin_may_read("modpack.defaultExcludes"));
    assert!(plugin_may_read("modpack.default-excludes"));
}

#[test]
fn only_a_readwrite_key_is_writable() {
    for (key, access) in PLUGIN_ACCESS {
        assert_eq!(
            plugin_may_write(key),
            *access == PluginAccess::ReadWrite,
            "{key}"
        );
        assert_eq!(plugin_may_read(key), *access >= PluginAccess::Read, "{key}");
    }
}

#[test]
fn a_parent_is_writable_only_when_every_key_under_it_is() {
    assert!(!plugin_may_write("defaults"));
    assert!(!plugin_may_write("modpack"));
    assert!(plugin_may_read("defaults"));
    assert!(!plugin_may_read("update"));
}

#[test]
fn a_plugin_sees_only_readable_keys() {
    let everything = json!({
        "defaults": { "memory": "2G", "jvm-args": "-javaagent:x.jar" },
        "content": { "curseforge-key": "secret" },
        "update": { "channel": "stable" },
        "discord": { "enabled": true },
        "home": "/data",
    });
    assert_eq!(
        plugin_view("", everything),
        Some(json!({
            "defaults": { "memory": "2G" },
            "discord": { "enabled": true },
        }))
    );
    assert_eq!(
        plugin_view("defaults", json!({ "memory": "2G", "jvmArgs": "-x" })),
        Some(json!({ "memory": "2G" }))
    );
    assert_eq!(
        plugin_view("defaults.memory", json!("2G")),
        Some(json!("2G"))
    );
    assert_eq!(plugin_view("defaults.jvm-args", json!("-x")), None);
    assert_eq!(
        plugin_view("content", json!({ "curseforgeKey": "s" })),
        None
    );
    assert_eq!(plugin_view("", json!({ "home": "/data" })), None);
}
