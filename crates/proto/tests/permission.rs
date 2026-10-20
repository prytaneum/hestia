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
    "config.get",
    "config.list",
    "entry.config.get",
    "entry.config.list",
    "entry.logs",
    "process.logs",
    "process.output",
    "server.files.read",
    "server.properties",
    // Arbitrary program, argv and env; plugins spawn only through `process_spawn`
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
    "plugin.enable",
    "plugin.disable",
    "plugin.remove",
    "plugin.state",
    // Other clients' subscriptions and jobs
    "events.subscribe",
    "job.list",
    "job.cancel",
    "job",
    "changed",
    // Reach outside the plugin's grants: any path, any address, the whole cache
    "content.inspect",
    "instance.import.inspect",
    "minecraft.ping",
    "cache.info",
    "cache.list",
    "cache.clear",
    // No permission fits
    "app.info",
    "host.info",
    "health.ping",
    "net.status",
    "net.state",
];

/// Every literal channel or topic in `src`, with whether its impl names a permission.
fn declared() -> Vec<(String, bool)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
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
            } else if line.contains("const PLUGIN: Option<Permission> = Some(") {
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
fn secret_bearing_and_privileged_channels_stay_closed() {
    use proto::accounts::*;
    use proto::config::*;
    use proto::daemon::*;
    use proto::entry::*;
    use proto::events::*;
    use proto::files::*;
    use proto::job::*;
    use proto::plugin::*;
    use proto::process::*;
    use proto::server::*;
    use proto::update::*;

    let closed = [
        (ProcessStart::CHANNEL, ProcessStart::PLUGIN),
        (ProcessLogs::CHANNEL, ProcessLogs::PLUGIN),
        (EntryLogs::CHANNEL, EntryLogs::PLUGIN),
        (ServerFilesRead::CHANNEL, ServerFilesRead::PLUGIN),
        (ServerProperties::CHANNEL, ServerProperties::PLUGIN),
        (EntryConfigGet::CHANNEL, EntryConfigGet::PLUGIN),
        (EntryConfigList::CHANNEL, EntryConfigList::PLUGIN),
        (ConfigGet::CHANNEL, ConfigGet::PLUGIN),
        (ConfigList::CHANNEL, ConfigList::PLUGIN),
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
