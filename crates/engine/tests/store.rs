//! Behaviour-parity tests for the pure-filesystem engine stores (config, cache,
//! servers, instances).

use engine::{
    Cache, Config, InstanceProfile, Instances, PluginRecord, Plugins, ServerJar, ServerPhase,
    ServerProfile, Servers,
};
use proto::download::{Checksum, HashAlgorithm};

/// A directory that removes itself when the test ends, named after the test so
/// a leftover under a debugger says which one left it.
fn temp_dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("hestia-engine-{tag}-"))
        .tempdir()
        .expect("temp dir")
}

#[test]
fn config_rejects_unknown_keys() {
    let dir = temp_dir("config");
    let cfg = Config::new(dir.path().join("config"));
    assert!(cfg.get("launcher.memory").is_err());
    assert!(cfg.set("launcher.memory", serde_json::json!(4096)).is_err());
}

#[test]
fn config_jvm_defaults_validate_and_normalise() {
    let dir = temp_dir("config-defaults");
    let cfg = Config::new(dir.path().join("config"));

    cfg.set("defaults.memory", serde_json::json!("4g")).unwrap();
    assert_eq!(cfg.get("defaults.memory").unwrap(), serde_json::json!("4G"));
    assert!(cfg
        .set("defaults.memory", serde_json::json!("lots"))
        .is_err());

    cfg.set("defaults.jvm-args", serde_json::json!("-XX:+UseG1GC"))
        .unwrap();
    assert!(cfg
        .set("defaults.jvm-args", serde_json::json!("not-a-flag stray"))
        .is_err());

    // Clearing both leaves no default applied at launch.
    cfg.set("defaults.memory", serde_json::json!("")).unwrap();
    cfg.set("defaults.jvm-args", serde_json::json!("")).unwrap();
    let defaults = cfg.settings().java_defaults();
    assert!(defaults.memory.is_none());
    assert!(defaults.jvm_args.is_empty());
}

#[test]
fn cache_stores_and_lists_by_checksum() {
    let dir = temp_dir("cache");
    let cache = Cache::new(dir.path().join("cache"));

    // sha256 of the empty input.
    let checksum = Checksum {
        algorithm: HashAlgorithm::Sha256,
        hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
    };
    let blob = dir.path().join("blob.bin");
    std::fs::write(&blob, b"").unwrap();

    assert!(cache.lookup(&checksum).is_none());
    cache.store(&blob, &checksum);
    assert!(cache.lookup(&checksum).is_some());

    let usage = cache.usage();
    assert_eq!(usage.entries, 1);

    let entries = cache.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].checksum.hex, checksum.hex);

    let freed = cache.clear();
    assert_eq!(freed.entries, 1);
    assert!(cache.lookup(&checksum).is_none());
}

#[test]
fn servers_store_round_trips_records() {
    let dir = temp_dir("servers");
    let servers = Servers::new(dir.path().join("servers"));
    assert!(servers.list().is_empty());

    let profile = ServerProfile {
        flavor: "vanilla".into(),
        game_version: "1.21.1".into(),
        java_major: 21,
        ..Default::default()
    };
    let record = servers.create("My Server!", profile.clone(), None).unwrap();
    assert!(
        record.id.len() == 32 && record.id.chars().all(|c| c.is_ascii_hexdigit()),
        "id is an opaque uuid, not the slug: {}",
        record.id
    );
    assert!(!record.ready());
    assert_ne!(record.game_port, 0);
    let entry_dir = servers.server_dir(&record);
    assert!(entry_dir.is_dir());
    assert!(
        entry_dir.ends_with("my-server"),
        "the directory is named for the slug, not the id: {}",
        entry_dir.display()
    );
    assert!(
        !servers.data_dir(&record).exists(),
        "data/ appears on demand, not at create"
    );
    assert!(servers.create("My Server!", profile.clone(), None).is_err());

    let second = servers.create("Other", profile.clone(), None).unwrap();
    assert_ne!(second.game_port, record.game_port);
    assert!(servers
        .create("Third", profile.clone(), Some(record.game_port))
        .is_err());
    assert!(servers.remove(&second.id).unwrap());

    let ready = servers.mark_ready(&record.id).unwrap();
    assert!(ready.ready());
    assert_eq!(servers.get(&record.id).unwrap().name, "My Server!");
    assert_eq!(servers.get("My Server!").unwrap().id, record.id);
    // A reference resolves by any spelling that slugs to the name.
    assert_eq!(servers.get("my-server").unwrap().id, record.id);
    assert_eq!(servers.get("MY  server").unwrap().id, record.id);
    assert!(servers.get("nope").is_none());
    assert_eq!(servers.list().len(), 1);
    assert_eq!(servers.list()[0].profile.game_version, profile.game_version);

    assert!(servers.remove(&record.id).unwrap());
    assert!(!servers.remove(&record.id).unwrap());
    assert!(servers.list().is_empty());
}

#[test]
fn recovery_discards_a_half_created_server_but_never_a_half_updated_one() {
    let dir = temp_dir("server-reconcile");
    let servers = Servers::new(dir.path().join("servers"));
    let profile = ServerProfile {
        flavor: "vanilla".into(),
        game_version: "1.21.1".into(),
        ..Default::default()
    };

    // What a daemon killed mid-create leaves: registered to hold its port
    // claim, never provisioned, and — before this — un-startable forever.
    let orphan = servers
        .create("Half Created", profile.clone(), None)
        .unwrap();
    assert_eq!(orphan.phase, ServerPhase::Provisioning);

    // A ready server, and one caught mid-update: the latter was ready before,
    // so its world is on disk and discarding it would destroy real data.
    let ready = servers.create("Fine", profile.clone(), None).unwrap();
    servers.mark_ready(&ready.id).unwrap();
    let updating = servers.create("Mid Update", profile, None).unwrap();
    servers.mark_ready(&updating.id).unwrap();
    servers
        .mark_phase(&updating.id, ServerPhase::Updating)
        .unwrap();

    let discarded = servers.reconcile();
    assert_eq!(discarded, vec!["Half Created".to_string()]);
    assert!(servers.get(&orphan.id).is_none(), "the orphan is gone");
    assert!(!servers.server_dir(&orphan).exists());

    assert!(servers.get(&ready.id).unwrap().ready());
    let survivor = servers
        .get(&updating.id)
        .expect("a mid-update server stays");
    assert_eq!(
        survivor.phase,
        ServerPhase::Updating,
        "its phase is kept, so updating again is what recovers it"
    );
    assert!(!survivor.ready(), "and it stays un-startable until then");

    // Recovery is idempotent: a second pass has nothing left to discard.
    assert!(servers.reconcile().is_empty());
}

#[test]
fn server_rename_keeps_the_id_moves_the_directory() {
    let dir = temp_dir("server-rename");
    let servers = Servers::new(dir.path().join("servers"));
    let profile = ServerProfile {
        flavor: "vanilla".into(),
        game_version: "1.21.1".into(),
        ..Default::default()
    };
    let created = servers.create("Old Name", profile.clone(), None).unwrap();
    let id = created.id.clone();
    let port = created.game_port;
    let old_dir = servers.server_dir(&created);
    servers.create("Keeper", profile, None).unwrap();

    // Renaming onto another entry's name is refused.
    assert!(servers.rename(&id, "Keeper").is_err());

    let renamed = servers.rename(&id, "New Name").unwrap();
    assert_eq!(renamed.id, id, "the id is stable across a rename");
    assert_eq!(renamed.name, "New Name");
    assert_eq!(renamed.game_port, port, "the claimed port is untouched");
    let new_dir = servers.server_dir(&renamed);
    assert!(new_dir.is_dir() && new_dir.ends_with("new-name"));
    assert!(!old_dir.exists(), "the birth-name directory is gone");
    assert_eq!(servers.get("New Name").unwrap().id, id);
    assert!(servers.get("Old Name").is_none());

    assert!(servers.rename("missing", "Whatever").is_err());
}

#[test]
fn instances_store_round_trips_records() {
    let dir = temp_dir("instances");
    let instances = Instances::new(dir.path().join("instances"));
    assert!(instances.list().is_empty());

    let profile = InstanceProfile {
        flavor: "fabric".into(),
        game_version: "1.21.1".into(),
        loader_version: Some("0.16.5".into()),
        java_major: 21,
        ..Default::default()
    };
    let record = instances.create("Modded", profile).unwrap();
    assert!(
        record.id.len() == 32 && record.id.chars().all(|c| c.is_ascii_hexdigit()),
        "id is an opaque uuid, not the slug: {}",
        record.id
    );
    assert!(
        instances
            .create("modded", InstanceProfile::default())
            .is_err(),
        "a slug-colliding name is refused"
    );

    assert_eq!(
        instances.get("Modded").unwrap().profile.loader_version,
        Some("0.16.5".into())
    );
    let entry_dir = instances.instance_dir(&record);
    assert!(entry_dir.is_dir() && entry_dir.ends_with("modded"));
    assert_eq!(instances.data_dir(&record), entry_dir.join("data"));
    assert!(
        !instances.data_dir(&record).exists(),
        "data/ appears on demand, not at create"
    );

    assert!(instances.remove(&record.id).unwrap());
    assert!(instances.list().is_empty());
}

#[test]
fn instance_rename_keeps_the_id_moves_the_directory() {
    let dir = temp_dir("instance-rename");
    let instances = Instances::new(dir.path().join("instances"));
    let record = instances
        .create(
            "Old Pack",
            InstanceProfile {
                flavor: "fabric".into(),
                game_version: "1.21.1".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let id = record.id.clone();
    let old_dir = instances.instance_dir(&record);
    instances.config_set(&id, "memory", "4G").unwrap();
    instances
        .create("Keeper", InstanceProfile::default())
        .unwrap();

    assert!(instances.rename(&id, "Keeper").is_err());

    let renamed = instances.rename(&id, "New Pack").unwrap();
    assert_eq!(renamed.id, id, "the id is stable across a rename");
    assert_eq!(renamed.name, "New Pack");
    let new_dir = instances.instance_dir(&renamed);
    assert!(new_dir.is_dir() && new_dir.ends_with("new-pack"));
    assert!(!old_dir.exists(), "the birth-name directory is gone");
    assert_eq!(instances.get("New Pack").unwrap().id, id);
    assert!(instances.get("Old Pack").is_none());
    assert_eq!(
        instances.config_get(&id, "memory").unwrap().as_deref(),
        Some("4G"),
        "JVM settings are untouched"
    );
    assert!(instances.rename("missing", "Whatever").is_err());
}

#[test]
fn instance_update_swaps_profile_and_keeps_settings() {
    let dir = temp_dir("instance-update");
    let instances = Instances::new(dir.path().join("instances"));
    let record = instances
        .create(
            "Modded",
            InstanceProfile {
                flavor: "fabric".into(),
                game_version: "1.21.1".into(),
                loader_version: Some("0.16.5".into()),
                ..Default::default()
            },
        )
        .unwrap();
    instances.config_set(&record.id, "memory", "4G").unwrap();

    let updated = instances
        .update(
            &record.id,
            InstanceProfile {
                flavor: "fabric".into(),
                game_version: "1.21.4".into(),
                loader_version: Some("0.16.9".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.name, "Modded");
    assert_eq!(updated.profile.game_version, "1.21.4");

    assert_eq!(
        instances.get(&record.id).unwrap().profile.game_version,
        "1.21.4"
    );
    assert_eq!(
        instances
            .config_get(&record.id, "memory")
            .unwrap()
            .as_deref(),
        Some("4G"),
        "JVM settings survive an update"
    );
    assert!(instances
        .update("missing", InstanceProfile::default())
        .is_err());
}

/// A server's own settings outlive a version change. The profile is the only
/// thing an update resolves upstream, so it must be the only thing it writes.
#[tokio::test]
async fn server_update_swaps_profile_and_keeps_settings() {
    let dir = temp_dir("server-update");
    let servers = Servers::new(dir.path().join("servers"));
    let profile = |version: &str| ServerProfile {
        flavor: "paper".into(),
        game_version: version.into(),
        jar: ServerJar::Download(proto::minecraft::Artifact {
            filename: "server.jar".into(),
            ..Default::default()
        }),
        ..ServerProfile::default()
    };
    let record = servers.create("SMP", profile("1.21"), None).unwrap();
    servers.config_set(&record.id, "memory", "4G").unwrap();
    let claimed = servers.get(&record.id).unwrap().game_port;

    let (swapped, previous_jar) = servers.begin_update(&record.id, profile("1.21.4")).unwrap();
    assert_eq!(swapped.phase, ServerPhase::Updating);
    let updated = servers.finish_update(&swapped, &previous_jar).unwrap();

    assert_eq!(updated.profile.game_version, "1.21.4");
    assert_eq!(updated.name, "SMP");
    assert_eq!(updated.phase, ServerPhase::Ready);
    assert_eq!(
        updated.game_port, claimed,
        "the claimed port is what players connect to; an update must not move it"
    );

    let reread = servers.get(&record.id).unwrap();
    assert_eq!(reread.profile.game_version, "1.21.4");
    assert_eq!(
        servers.config_get(&record.id, "memory").unwrap().as_deref(),
        Some("4G"),
        "JVM settings survive an update"
    );
}

#[test]
fn server_config_covers_jvm_and_properties() {
    let dir = temp_dir("server-config");
    let servers = Servers::new(dir.path().join("servers"));
    let record = servers
        .create("SMP", ServerProfile::default(), None)
        .unwrap();
    let id = &record.id;

    // JVM keys live on the record; unset reads back as None.
    assert_eq!(servers.config_get(id, "memory").unwrap(), None);
    servers.config_set(id, "memory", "4g").unwrap();
    assert_eq!(
        servers.config_get(id, "memory").unwrap().as_deref(),
        Some("4G")
    );

    // A hestia-managed property is rejected before any other check, file or
    // no file.
    let managed = servers.config_set(id, "server-port", "25570").unwrap_err();
    assert!(managed.to_string().contains("managed by hestia"));

    // No derived schema yet: there is no ground truth, so any (unmanaged) key
    // is accepted rather than everything rejected.
    assert!(!servers.has_schema(&record));
    servers.config_set(id, "no-schema-yet", "1").unwrap();
    assert_eq!(
        servers.config_get(id, "no-schema-yet").unwrap().as_deref(),
        Some("1")
    );

    // Store the schema as the pristine generation run would: every key this
    // version knows, with its default.
    std::fs::write(
        servers.schema_path(&record),
        "motd=A Minecraft Server\nview-distance=10\n",
    )
    .unwrap();
    assert!(servers.has_schema(&record));

    // A key in the schema is accepted and reads back.
    servers.config_set(id, "motd", "hi there").unwrap();
    assert_eq!(
        servers.config_get(id, "motd").unwrap().as_deref(),
        Some("hi there")
    );

    // A key the schema does not carry is rejected — a typo cannot drift the
    // file.
    let unknown = servers.config_set(id, "this-is-a-typo", "x").unwrap_err();
    assert!(unknown.to_string().contains("unknown config key"));

    // The live file is values, not the schema: a key vanilla preserved through
    // an update is still readable and listable, but no longer settable, because
    // the version that carried it is gone.
    let properties = servers.data_dir(&record).join("server.properties");
    std::fs::create_dir_all(properties.parent().unwrap()).unwrap();
    let existing = std::fs::read_to_string(&properties).unwrap_or_default();
    std::fs::write(&properties, format!("{existing}retired-key=leftover\n")).unwrap();
    assert_eq!(
        servers.config_get(id, "retired-key").unwrap().as_deref(),
        Some("leftover")
    );
    assert!(servers
        .config_set(id, "retired-key", "x")
        .unwrap_err()
        .to_string()
        .contains("unknown config key"));

    // Managed keys stay rejected even though the file now exists.
    assert!(servers.config_set(id, "rcon.port", "25580").is_err());

    // list surfaces the reserved JVM keys, the values the user set, and the
    // keys the file kept from an earlier version.
    servers.config_set(id, "view-distance", "10").unwrap();
    let entries = servers.config_list(id).unwrap();
    assert!(entries.iter().any(|(k, v)| k == "memory" && v == "4G"));
    assert!(entries.iter().any(|(k, _)| k == "jvm-args"));
    assert!(entries.iter().any(|(k, v)| k == "motd" && v == "hi there"));
    assert!(entries
        .iter()
        .any(|(k, v)| k == "view-distance" && v == "10"));
    assert!(entries
        .iter()
        .any(|(k, v)| k == "retired-key" && v == "leftover"));
}

#[test]
fn server_config_enforces_hardcore_invariant() {
    let dir = temp_dir("server-hardcore");
    let servers = Servers::new(dir.path().join("servers"));
    let record = servers
        .create("HC", ServerProfile::default(), None)
        .unwrap();
    let id = &record.id;

    // Seed the file as a fresh server generates it: not hardcore, easy, survival.
    let properties = servers.data_dir(&record).join("server.properties");
    std::fs::create_dir_all(properties.parent().unwrap()).unwrap();
    std::fs::write(
        &properties,
        "hardcore=false\ndifficulty=easy\ngamemode=survival\n",
    )
    .unwrap();

    // Enabling hardcore while difficulty is not hard is refused.
    let err = servers.config_set(id, "hardcore", "true").unwrap_err();
    assert!(err.to_string().contains("difficulty=hard"));

    // Aligning the linked keys first, then enabling hardcore, is accepted.
    servers.config_set(id, "difficulty", "hard").unwrap();
    servers.config_set(id, "hardcore", "true").unwrap();

    // With hardcore on, difficulty is locked to hard and gamemode to survival.
    assert!(servers
        .config_set(id, "difficulty", "easy")
        .unwrap_err()
        .to_string()
        .contains("locked to 'hard'"));
    assert!(servers
        .config_set(id, "gamemode", "creative")
        .unwrap_err()
        .to_string()
        .contains("locked to 'survival'"));

    // Setting them to the pinned values is a no-op that still passes.
    servers.config_set(id, "gamemode", "survival").unwrap();

    // Disabling hardcore frees the linked keys again.
    servers.config_set(id, "hardcore", "false").unwrap();
    servers.config_set(id, "difficulty", "easy").unwrap();
    servers.config_set(id, "gamemode", "creative").unwrap();
}

#[test]
fn instance_config_rejects_unknown_keys() {
    let dir = temp_dir("instance-config");
    let instances = Instances::new(dir.path().join("instances"));
    let record = instances
        .create("Modded", InstanceProfile::default())
        .unwrap();
    let id = &record.id;

    instances
        .config_set(id, "jvm-args", "-XX:+UseG1GC -Xmn1G")
        .unwrap();
    assert_eq!(
        instances.config_get(id, "jvm-args").unwrap().as_deref(),
        Some("-XX:+UseG1GC -Xmn1G")
    );
    // Empty clears.
    instances.config_set(id, "jvm-args", "").unwrap();
    assert_eq!(instances.config_get(id, "jvm-args").unwrap(), None);

    // Non-JVM keys are rejected (no properties file for instances).
    assert!(instances.config_set(id, "motd", "hi").is_err());
    assert!(instances.config_get(id, "motd").is_err());
}

fn plugin_record(id: &str) -> PluginRecord {
    PluginRecord {
        id: id.into(),
        wasm_sha256: Some("ab".repeat(32)),
        enabled: true,
        ..PluginRecord::default()
    }
}

#[test]
fn plugin_record_round_trips_and_removes() {
    let dir = temp_dir("plugins");
    let plugins = Plugins::new(dir.path().join("plugins"));
    plugins.save(&plugin_record("alpha")).unwrap();

    let stored = plugins.get("alpha").unwrap();
    assert_eq!(stored.wasm_sha256, plugin_record("alpha").wasm_sha256);
    assert!(plugins.data_dir("alpha").unwrap().is_dir());

    assert!(plugins.set_enabled("alpha", false).unwrap());
    assert!(!plugins.get("alpha").unwrap().enabled);

    assert!(plugins.remove("alpha").unwrap());
    assert!(plugins.get("alpha").is_none());
    assert!(!plugins.remove("alpha").unwrap());
}

#[test]
fn plugin_ids_that_escape_the_directory_are_refused() {
    let dir = temp_dir("plugins-id");
    let plugins = Plugins::new(dir.path().join("plugins"));
    assert!(plugins.plugin_dir("../escape").is_err());
    assert!(plugins.remove("..").is_err());
    assert!(plugins.save(&plugin_record("Bad/Id")).is_err());
}
