//! The plugin API is every contract and topic that names a `PLUGIN` permission,
//! plus the WIT package. `golden/plugin-api.json` snapshots its shape, as the
//! ts-rs declarations of every type it reaches, so a change cannot ship under an
//! unchanged `API_VERSION`.
//!
//! Needs the `ts` feature: `cargo test -p proto --features ts --test plugin_api`.
//! It is a target of its own because enabling `ts` for a whole `cargo test` run
//! would make proto's unit tests export bindings into the frontend.

use std::any::type_name;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use proto::job::Job;
use proto::plugin::API_VERSION;
use proto::*;
use serde_json::{json, Value};
use ts_rs::{Config, Dependency, TypeVisitor, TS};

const REGENERATE: &str =
    "regenerate with `HESTIA_UPDATE_GOLDEN=1 cargo test -p proto --features ts --test plugin_api` \
     and review the diff of tests/golden/plugin-api.json";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every declared type reached from a root, by its TypeScript name.
#[derive(Default)]
struct Types {
    cfg: Config,
    decls: BTreeMap<String, String>,
}

impl Types {
    fn root<T: TS + 'static>(&mut self) -> String {
        self.visit::<T>();
        T::visit_generics(self);
        T::name(&self.cfg)
    }
}

impl TypeVisitor for Types {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if Dependency::from_ty::<T>(&self.cfg).is_none() {
            return;
        }
        let name = T::ident(&self.cfg);
        if self.decls.contains_key(&name) {
            return;
        }
        self.decls
            .insert(name, without_comments(&T::decl(&self.cfg)));
        T::visit_dependencies(self);
    }
}

/// A doc comment is not part of the shape.
fn without_comments(decl: &str) -> String {
    let mut kept = String::new();
    let mut rest = decl;
    while let Some(start) = rest.find("/**") {
        kept.push_str(&rest[..start]);
        rest = rest[start..]
            .split_once("*/")
            .map_or("", |(_, after)| after);
    }
    kept.push_str(rest);
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Default)]
struct Api {
    types: Types,
    channels: BTreeMap<String, Value>,
    listed: BTreeMap<&'static str, BTreeSet<String>>,
}

impl Api {
    fn list<T: ?Sized>(&mut self, kind: &'static str) {
        let name = type_name::<T>().rsplit("::").next().unwrap();
        self.listed
            .entry(kind)
            .or_default()
            .insert(name.to_string());
    }

    fn contract<C: Contract>(&mut self)
    where
        C::Params: TS + 'static,
        C::Result: TS + 'static,
    {
        self.list::<C>("Contract");
        if let Some(permission) = C::PLUGIN {
            let entry = json!({
                "permission": permission.to_string(),
                "params": self.types.root::<C::Params>(),
                "result": self.types.root::<C::Result>(),
            });
            self.channels.insert(C::CHANNEL.to_string(), entry);
        }
    }

    fn job<C: Job>(&mut self)
    where
        C::Progress: TS,
        C::Done: TS,
    {
        self.list::<C>("Job");
        if C::PLUGIN.is_some() {
            let progress = self.types.root::<C::Progress>();
            let done = self.types.root::<C::Done>();
            let entry = self
                .channels
                .get_mut(C::CHANNEL)
                .expect("a job is listed as a contract first");
            entry["progress"] = json!(progress);
            entry["done"] = json!(done);
        }
    }

    fn topic<T: Topic + TS + 'static>(&mut self) {
        self.list::<T>("Topic");
        if let Some(permission) = T::PLUGIN {
            let entry = json!({
                "permission": permission.to_string(),
                "payload": self.types.root::<T>(),
            });
            self.channels.insert(T::TOPIC.to_string(), entry);
        }
    }
}

macro_rules! each {
    ($api:ident.$method:ident: $($ty:ty),* $(,)?) => {
        $( $api.$method::<$ty>(); )*
    };
}

/// Every contract, job and topic in proto; the test fails when one is missing.
fn api() -> Api {
    let mut api = Api::default();
    each!(api.contract:
        accounts::AccountList, accounts::AccountLoginBegin, accounts::AccountLoginComplete,
        accounts::AccountRemove, accounts::AccountSwitch,
        app::AppInfo,
        backup::ServerBackupCreate, backup::ServerBackupList, backup::ServerBackupRemove,
        backup::ServerBackupRestore,
        cache::CacheClear, cache::CacheInfo, cache::CacheList,
        config::ConfigGet, config::ConfigList, config::ConfigSet,
        content::ContentInspect, content::ContentProjectGet, content::ContentResolveUrl,
        content::ContentSearch, content::ContentSources, content::ContentVersions,
        content::EntryContentAdd, content::EntryContentCheckUpdates, content::EntryContentEnable,
        content::EntryContentList, content::EntryContentMatches, content::EntryContentRemove,
        content::EntryContentSetVersion, content::EntryContentUpdate,
        daemon::DaemonStatus, daemon::DaemonStop,
        entry::EntryConfigGet, entry::EntryConfigList, entry::EntryConfigSet, entry::EntryFlavors,
        entry::EntryLoaders, entry::EntryLogs, entry::EntryRemove, entry::EntryRename,
        entry::EntryStop, entry::EntryVersions,
        events::EventsSubscribe,
        files::ServerFilesCompress, files::ServerFilesCopy, files::ServerFilesExtract,
        files::ServerFilesImport, files::ServerFilesList, files::ServerFilesMkdir,
        files::ServerFilesMove, files::ServerFilesRead, files::ServerFilesRemove,
        files::ServerFilesWrite,
        health::Ping,
        host::HostInfo,
        instance::AddressPing, instance::InstanceCreate, instance::InstanceInfoQuery,
        instance::InstanceLaunch, instance::InstanceList, instance::InstanceRecent,
        instance::InstanceServerEdit, instance::InstanceServerRemove, instance::InstanceServers,
        instance::InstanceServersArrange, instance::InstanceUpdate, instance::InstanceWorlds,
        java::JavaInstall, java::JavaList, java::JavaReleases, java::JavaUninstall,
        job::JobCancel, job::JobList,
        modpack::EntryModpackCheckUpdate, modpack::EntryModpackPreviewUpdate,
        modpack::EntryModpackRemove, modpack::EntryModpackStatus, modpack::EntryModpackUpdate,
        modpack::InstanceModpackInstall, modpack::ServerModpackInstall,
        net::NetStatus,
        players::ServerPlayerKick, players::ServerPlayers, players::ServerPlayerSet,
        plugin::PluginDisable, plugin::PluginEnable, plugin::PluginGet, plugin::PluginInspect,
        plugin::PluginInstall, plugin::PluginList, plugin::PluginRemove,
        process::ProcessList, process::ProcessLogs, process::ProcessStart, process::ProcessStatus,
        process::ProcessStop,
        screenshot::ScreenshotDelete, screenshot::ScreenshotList,
        server::ServerCommand, server::ServerCommandComplete, server::ServerCreate,
        server::ServerDetail, server::ServerList, server::ServerPing, server::ServerProperties,
        server::ServerRestart, server::ServerStart, server::ServerStatus, server::ServerUpdate,
        skins::CapeClear, skins::CapeEquip, skins::SkinAdd, skins::SkinEquip, skins::SkinList,
        skins::SkinRemove, skins::SkinReset, skins::SkinUpdate,
        sync::InstanceSyncKeys, sync::InstanceSyncUnit, sync::SyncDisable, sync::SyncEnable,
        sync::SyncGet, sync::SyncKeys, sync::SyncOptionSet, sync::SyncOptionsGet,
        sync::SyncPackRemove, sync::SyncPacks, sync::SyncPackSet, sync::SyncSources,
        sync::SyncStatus,
        tasks::ServerTaskList, tasks::ServerTaskRemove, tasks::ServerTaskSave,
        transfer::InstanceExport, transfer::InstanceExportContents, transfer::InstanceImport,
        transfer::InstanceImportInspect,
        update::UpdateApply, update::UpdateCheck, update::UpdateDownload,
    );
    each!(api.job:
        backup::ServerBackupCreate, backup::ServerBackupRestore,
        content::EntryContentAdd, content::EntryContentSetVersion, content::EntryContentUpdate,
        instance::InstanceLaunch, instance::InstanceUpdate,
        java::JavaInstall,
        modpack::EntryModpackUpdate, modpack::InstanceModpackInstall,
        modpack::ServerModpackInstall,
        plugin::PluginInstall,
        server::ServerCreate, server::ServerUpdate,
        transfer::InstanceExport, transfer::InstanceImport,
        update::UpdateDownload,
    );
    each!(api.topic:
        change::ChangedEvent,
        net::NetworkStatus,
        plugin::PluginStateEvent,
        process::ProcessExitEvent, process::ProcessMetricsEvent, process::ProcessOutputEvent,
        process::ProcessStartedEvent,
    );
    api
}

/// The type names behind every `impl <kind> for` in `src/`.
fn implemented(kind: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for entry in fs::read_dir(crate_dir().join("src")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        for line in text.lines() {
            let Some((trait_path, target)) = line
                .strip_prefix("impl ")
                .and_then(|rest| rest.split_once(" for "))
            else {
                continue;
            };
            if trait_path.rsplit("::").next() == Some(kind) {
                let name = target.split([' ', '{', '<']).next().unwrap();
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// Each WIT file without its comments, blank lines or package line (the
/// version is `apiVersion`).
fn wit() -> BTreeMap<String, String> {
    fs::read_dir(crate_dir().join("../sdk/wit"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "wit"))
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty() && !line.starts_with("//") && !line.starts_with("package ")
                })
                .collect();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, lines.join("\n"))
        })
        .collect()
}

#[test]
fn plugin_api_matches_its_snapshot() {
    let api = api();
    for kind in ["Contract", "Job", "Topic"] {
        assert_eq!(
            api.listed[kind],
            implemented(kind),
            "list every `impl {kind}` in this test's api()"
        );
    }
    let current = json!({
        "apiVersion": API_VERSION,
        "wit": wit(),
        "channels": api.channels,
        "types": api.types.decls,
    });
    let path = crate_dir().join("tests/golden/plugin-api.json");
    let golden: Option<Value> = fs::read_to_string(&path)
        .ok()
        .map(|text| serde_json::from_str(&text).unwrap());
    if golden.as_ref() == Some(&current) {
        return;
    }
    let same_version = golden
        .as_ref()
        .is_some_and(|golden| golden["apiVersion"] == current["apiVersion"]);
    assert!(
        !same_version,
        "the plugin API changed under an unchanged version: bump the WIT package and \
         `proto::plugin::API_VERSION` (minor for an addition, major for a removal or a \
         changed shape), then {REGENERATE}"
    );
    if std::env::var_os("HESTIA_UPDATE_GOLDEN").is_some() {
        fs::write(
            &path,
            serde_json::to_string_pretty(&current).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    panic!("the plugin API snapshot is stale; {REGENERATE}");
}
