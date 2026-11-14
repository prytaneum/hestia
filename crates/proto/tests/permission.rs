//! Drift guards for `Contract::PLUGIN` / `Topic::PLUGIN`: a plugin reaches only
//! channels that name a permission, so a new contract must be classified or
//! explicitly denied, and a secret-bearing channel must never become reachable.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use proto::{Contract, Topic};

/// Channels and topics that deliberately name no permission, each with the reason.
const DENIED: &[&str] = &[
    // Secrets
    "entry.config.get",
    "entry.config.list",
    "entry.logs",
    "process.logs",
    "process.output",
    "server.properties",
    // Arbitrary program, argv and env; plugins spawn only through `process:spawn`
    "process.start",
    // Sign-in, tokens and the account itself
    "account.login.begin",
    "account.login.complete",
    "account.switch",
    "account.remove",
    "skin.list",
    "skin.add",
    "skin.equip",
    "skin.reset",
    "skin.update",
    "skin.remove",
    "cape.equip",
    "cape.clear",
    // Daemon lifecycle, self-update and plugin management
    "daemon.stop",
    "daemon.status",
    "update.check",
    "update.download",
    "update.apply",
    "plugin.list",
    "plugin.info",
    "plugin.settings.get",
    "plugin.settings.set",
    "plugin.enable",
    "plugin.disable",
    "plugin.remove",
    "plugin.inspect",
    "plugin.install",
    "plugin.update",
    "plugin.repo.add",
    "plugin.repo.list",
    "plugin.repo.remove",
    "plugin.repo.refresh",
    "plugin.catalogue",
    "plugin.outdated",
    "plugin.updates",
    "plugin.run",
    "plugin.asset",
    "plugin.image",
    "plugin.messages",
    "plugin.pages",
    "plugin.commands",
    "plugin.themes",
    "plugin.ui.render",
    "plugin.ui.action",
    "plugin.state",
    "plugin.rolled_back",
    "plugin.auto_updated",
    "plugin.schedule",
    "plugin.log",
    // Other clients' subscriptions and jobs
    "events.subscribe",
    "job.list",
    "job.cancel",
    "job",
    // Reach outside the plugin's grants: any path, any address, the whole cache
    "content.inspect",
    "entry.icon.set",
    "instance.import.inspect",
    "minecraft.ping",
    "cache.info",
    "cache.list",
    "cache.clear",
    "image.fetch",
];

/// Channels any plugin may call with no permission, each with why its whole
/// result is harmless. Growing this list is a decision, so it is pinned here.
const AMBIENT: &[&str] = &[
    // Name, version, id, vendor and release channel of this build
    "app.info",
    // Operating system, architecture, path separator, daemon pid and installed memory
    "host.info",
    // Liveness and the daemon pid
    "health.ping",
    // Whether upstream is reachable, and since when
    "net.status",
];

/// Every literal channel or topic in `src`, with whether its impl is classified:
/// it names a permission or is ambient.
fn declared() -> Vec<(String, bool)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in fs::read_dir(&dir)
        .unwrap()
        .chain(fs::read_dir(dir.join("plugin")).unwrap())
    {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        let mut current: Option<usize> = None;
        for line in source.lines() {
            let literal = [
                "const CHANNEL: &'static str = \"",
                "const TOPIC: &'static str = \"",
            ]
            .iter()
            .find_map(|marker| line.trim().strip_prefix(marker))
            .and_then(|rest| rest.strip_suffix("\";"));
            let name = if line.trim() == "const TOPIC: &'static str = TOPIC;" {
                Some(proto::job::TOPIC)
            } else {
                literal
            };
            if let Some(name) = name {
                found.push((name.to_string(), false));
                current = Some(found.len() - 1);
            } else if line.contains("const PLUGIN: Option<Permission> = Some(")
                || line.trim() == "const AMBIENT: bool = true;"
            {
                if let Some(index) = current {
                    found[index].1 = true;
                }
            } else if line.starts_with('}') {
                current = None;
            }
        }
    }
    found
}

/// The channels whose impl marks `AMBIENT = true`.
fn declared_ambient() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = BTreeSet::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let mut channel = None;
        for line in fs::read_to_string(&path).unwrap().lines() {
            if let Some(name) = line
                .trim()
                .strip_prefix("const CHANNEL: &'static str = \"")
                .and_then(|rest| rest.strip_suffix("\";"))
            {
                channel = Some(name.to_string());
            } else if line.trim() == "const AMBIENT: bool = true;" {
                found.extend(channel.clone());
            }
        }
    }
    found
}

#[test]
fn every_unclassified_channel_is_explicitly_denied() {
    let denied: BTreeSet<&str> = DENIED.iter().copied().collect();
    let declared = declared();
    let unclassified: Vec<&str> = declared
        .iter()
        .filter(|(name, classified)| !classified && !denied.contains(name.as_str()))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        unclassified.is_empty(),
        "give these a `PLUGIN` permission, or add them to DENIED with a reason: {unclassified:?}"
    );

    let contradicted: Vec<&str> = declared
        .iter()
        .filter(|(name, classified)| *classified && denied.contains(name.as_str()))
        .map(|(name, _)| name.as_str())
        .collect();
    assert!(
        contradicted.is_empty(),
        "denied but classified: {contradicted:?}"
    );

    let existing: BTreeSet<&str> = declared.iter().map(|(name, _)| name.as_str()).collect();
    let stale: Vec<&str> = denied.difference(&existing).copied().collect();
    assert!(
        stale.is_empty(),
        "DENIED names no declared channel: {stale:?}"
    );
}

#[test]
fn the_ambient_channels_are_exactly_the_pinned_list() {
    let declared = declared();
    let ambient: BTreeSet<&str> = AMBIENT.iter().copied().collect();
    let classified: BTreeSet<&str> = declared
        .iter()
        .filter(|(_, classified)| *classified)
        .map(|(name, _)| name.as_str())
        .collect();
    for channel in &ambient {
        assert!(
            classified.contains(channel),
            "AMBIENT names an unclassified or missing channel: {channel}"
        );
    }

    use proto::app::AppInfo;
    use proto::health::Ping;
    use proto::host::HostInfo;
    use proto::net::NetStatus;
    let marked = [
        (AppInfo::CHANNEL, AppInfo::AMBIENT, AppInfo::PLUGIN),
        (Ping::CHANNEL, Ping::AMBIENT, Ping::PLUGIN),
        (HostInfo::CHANNEL, HostInfo::AMBIENT, HostInfo::PLUGIN),
        (NetStatus::CHANNEL, NetStatus::AMBIENT, NetStatus::PLUGIN),
    ];
    let marked_names: BTreeSet<&str> = marked.iter().map(|(name, _, _)| *name).collect();
    assert_eq!(marked_names, ambient);
    for (name, is_ambient, permission) in marked {
        assert!(is_ambient, "{name} is listed ambient but not marked");
        assert!(permission.is_none(), "{name} is ambient and permissioned");
    }

    let by_source: BTreeSet<String> = declared_ambient();
    let by_source: BTreeSet<&str> = by_source.iter().map(String::as_str).collect();
    assert_eq!(by_source, ambient, "an `AMBIENT = true` impl is not pinned");
}

#[test]
fn an_ambient_channel_is_never_part_of_a_grant() {
    use proto::plugin::{Grant, Permission};

    assert!(serde_json::from_str::<Permission>("\"ambient\"").is_err());
    for permission in Permission::ALL {
        assert_ne!(permission.to_string(), "ambient");
    }
    let nothing = Grant::default();
    assert!(nothing.permissions.is_empty());
    assert_eq!(
        nothing.approval_digest(&Default::default()),
        Grant::default().approval_digest(&Default::default())
    );
}

#[test]
fn secret_bearing_and_privileged_channels_stay_closed() {
    use proto::accounts::*;
    use proto::daemon::*;
    use proto::entry::*;
    use proto::events::*;
    use proto::job::*;
    use proto::plugin::*;
    use proto::process::*;
    use proto::server::*;
    use proto::update::*;

    let closed = [
        (ProcessStart::CHANNEL, ProcessStart::PLUGIN),
        (ProcessLogs::CHANNEL, ProcessLogs::PLUGIN),
        (EntryLogs::CHANNEL, EntryLogs::PLUGIN),
        (ServerProperties::CHANNEL, ServerProperties::PLUGIN),
        (EntryConfigGet::CHANNEL, EntryConfigGet::PLUGIN),
        (EntryConfigList::CHANNEL, EntryConfigList::PLUGIN),
        (EventsSubscribe::CHANNEL, EventsSubscribe::PLUGIN),
        (JobCancel::CHANNEL, JobCancel::PLUGIN),
        (AccountLoginBegin::CHANNEL, AccountLoginBegin::PLUGIN),
        (AccountLoginComplete::CHANNEL, AccountLoginComplete::PLUGIN),
        (AccountSwitch::CHANNEL, AccountSwitch::PLUGIN),
        (AccountRemove::CHANNEL, AccountRemove::PLUGIN),
        (UpdateCheck::CHANNEL, UpdateCheck::PLUGIN),
        (UpdateDownload::CHANNEL, UpdateDownload::PLUGIN),
        (UpdateApply::CHANNEL, UpdateApply::PLUGIN),
        (DaemonStop::CHANNEL, DaemonStop::PLUGIN),
        (PluginList::CHANNEL, PluginList::PLUGIN),
        (PluginGet::CHANNEL, PluginGet::PLUGIN),
        (PluginEnable::CHANNEL, PluginEnable::PLUGIN),
        (PluginDisable::CHANNEL, PluginDisable::PLUGIN),
        (PluginRemove::CHANNEL, PluginRemove::PLUGIN),
        (PluginAsset::CHANNEL, PluginAsset::PLUGIN),
        (PluginImage::CHANNEL, PluginImage::PLUGIN),
        (PluginMessages::CHANNEL, PluginMessages::PLUGIN),
        (PluginThemes::CHANNEL, PluginThemes::PLUGIN),
        (
            <ProcessOutputEvent as Topic>::TOPIC,
            <ProcessOutputEvent as Topic>::PLUGIN,
        ),
        (
            <JobEvent<(), ()> as Topic>::TOPIC,
            <JobEvent<(), ()> as Topic>::PLUGIN,
        ),
    ];
    let opened: Vec<&str> = closed
        .iter()
        .filter(|(_, permission)| permission.is_some())
        .map(|(name, _)| *name)
        .collect();
    assert!(
        opened.is_empty(),
        "secret-bearing channels reclassified: {opened:?}"
    );
}

#[test]
fn every_topic_is_in_the_registry_plugins_are_admitted_by() {
    let registered: BTreeSet<&str> = proto::events::TOPICS
        .iter()
        .map(|access| access.topic)
        .collect();
    let declared: BTreeSet<String> = declared()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| {
            let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
            fs::read_dir(&src)
                .unwrap()
                .chain(fs::read_dir(src.join("plugin")).unwrap())
                .any(|entry| {
                    fs::read_to_string(entry.unwrap().path())
                        .unwrap_or_default()
                        .contains(&format!("const TOPIC: &'static str = \"{name}\";"))
                })
                || name == proto::job::TOPIC
        })
        .collect();
    let missing: Vec<&String> = declared
        .iter()
        .filter(|topic| !registered.contains(topic.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "add these topics to proto::events::TOPICS: {missing:?}"
    );
}

#[test]
fn a_topic_without_a_permission_reaches_no_plugin() {
    use serde_json::json;
    for topic in [
        "process.output",
        "plugin.schedule",
        "plugin.updates",
        "plugin.rolled_back",
        "plugin.schedule",
        "no.such.topic",
    ] {
        assert_eq!(
            proto::events::topic_permission(topic, &json!({})),
            None,
            "{topic}"
        );
    }
}
