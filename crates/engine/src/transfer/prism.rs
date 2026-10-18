//! The Prism Launcher / MultiMC / PolyMC instance format.
//!
//! Those launchers describe an instance with two files: `instance.cfg`, a flat
//! `key=value` list of the settings the user overrode, and `mmc-pack.json`, a
//! list of *components* — the game itself and whatever is layered on it. The
//! game directory beside them is `.minecraft/` (`minecraft/` on the older
//! layout), which is exactly what hestia calls `data/`.
//!
//! The components name a game version and a loader rather than resolving them,
//! so the recipe carries no identity and the launcher looks them up the way
//! `instance create` does. Whether hestia *has* that loader is not decided
//! here: the answer is "whichever flavors are registered", which is the flow's
//! to ask.

use anyhow::Result;
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use proto::transfer::ImportFormat;
use serde_json::Value;

use super::archive::Reader;
use super::{pool, Blueprint, Descriptor, Format, Landed, Recipe, Target};
use crate::cancel::Job;
use crate::jvm::{JavaSettings, JVM_ARGS_KEY, MEMORY_KEY};

pub(crate) const CONFIG: &str = "instance.cfg";
pub(crate) const PACK: &str = "mmc-pack.json";

/// The component uids that name a mod loader, and the loader name each one is.
/// Everything else a pack lists — the intermediary mappings, LWJGL, the Java
/// agent components — is scaffolding those launchers need and hestia resolves
/// for itself.
const LOADERS: &[(&str, &str)] = &[
    ("net.fabricmc.fabric-loader", "fabric"),
    ("org.quiltmc.quilt-loader", "quilt"),
    ("net.neoforged", "neoforge"),
    ("net.minecraftforge", "forge"),
    ("com.mumfrey.liteloader", "liteloader"),
];

const MINECRAFT: &str = "net.minecraft";

/// What the two files together say the instance is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Instance {
    pub(crate) name: String,
    pub(crate) game_version: String,
    /// `(loader name, loader version)`; `None` for a vanilla instance.
    pub(crate) loader: Option<(String, String)>,
    /// `MaxMemAlloc`, in megabytes, only when the instance overrode memory —
    /// a launcher-wide default is not this instance's setting to carry.
    pub(crate) memory_mb: Option<u32>,
    /// `JvmArgs`, only when the instance overrode them.
    pub(crate) jvm_args: Option<String>,
}

pub(crate) struct Prism;

impl Format for Prism {
    fn id(&self) -> ImportFormat {
        ImportFormat::Prism
    }

    fn marker(&self) -> &'static str {
        CONFIG
    }

    fn read(&self, reader: &mut Reader, prefix: &str) -> Result<Blueprint> {
        let instance = describe(reader, prefix)?;
        let jvm = jvm_settings(&instance);
        let (loader, loader_version) = instance.loader.unzip();
        let loader = loader.unwrap_or_default();
        Ok(Blueprint {
            descriptor: Descriptor {
                name: instance.name,
                game_version: instance.game_version,
                loader,
                loader_version: loader_version.unwrap_or_default(),
            },
            recipe: Recipe { jvm, carried: None },
        })
    }

    fn land(
        &self,
        reader: &mut Reader,
        prefix: &str,
        target: &Target<'_>,
        job: &Job<'_>,
    ) -> Result<Landed> {
        let game_dir = game_dir(&reader.names(), prefix);
        let files = reader.extract_under(
            &format!("{prefix}{game_dir}/"),
            target.data_dir,
            job,
            &|_| true,
        )?;
        let adopted = pool::adopt(target.entry_dir, target.data_dir)?;
        Ok(Landed { files, adopted })
    }
}

fn describe(reader: &mut Reader, prefix: &str) -> Result<Instance> {
    let config = reader
        .read_text(&format!("{prefix}{CONFIG}"))
        .map_err(|e| {
            tracing::warn!(error = %e, "cannot read {CONFIG}");
            invalid(ArchiveFault::ManifestMissing)
        })?;
    let pack = reader.read_text(&format!("{prefix}{PACK}")).map_err(|e| {
        tracing::warn!(error = %e, "cannot read {PACK}");
        invalid(ArchiveFault::ManifestMissing)
    })?;
    read(&config, &pack)
}

fn invalid(reason: ArchiveFault) -> ErrorInfo {
    ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Prism,
        reason,
    }
}

/// The per-instance Java settings, kept only where the instance overrode its
/// launcher's defaults. An unparseable value is dropped rather than failing the
/// import — a JVM flag hestia rejects is not worth losing an instance over.
fn jvm_settings(instance: &Instance) -> JavaSettings {
    let mut jvm = JavaSettings::default();
    if let Some(mb) = instance.memory_mb {
        if let Err(e) = jvm.set(MEMORY_KEY, &format!("{mb}M")) {
            tracing::warn!(error = %e, "dropping an imported memory setting");
        }
    }
    if let Some(args) = &instance.jvm_args {
        if let Err(e) = jvm.set(JVM_ARGS_KEY, args) {
            tracing::warn!(error = %e, "dropping imported jvm arguments");
        }
    }
    jvm
}

/// Parse the flat `key=value` config. Section headers and comments are
/// tolerated: the format is whatever Qt's settings writer produced, and a file
/// hand-edited into having a `[General]` header still describes an instance.
pub(crate) fn parse_config(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(['#', ';', '[']))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

fn setting<'a>(config: &'a [(String, String)], key: &str) -> Option<&'a str> {
    config
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// Whether an override flag is on. Absent means off — the instance is then
/// using the launcher's global setting, which is not part of the instance.
fn overridden(config: &[(String, String)], key: &str) -> bool {
    setting(config, key).is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

pub(crate) fn read(config_text: &str, pack_text: &str) -> Result<Instance> {
    let config = parse_config(config_text);
    let pack: Value = serde_json::from_str(pack_text).map_err(|e| {
        tracing::warn!(error = %e, "{PACK} is malformed");
        invalid(ArchiveFault::ManifestMalformed)
    })?;

    let components = pack
        .get("components")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(ArchiveFault::NoComponents))?;

    let mut game_version = String::new();
    let mut loader = None;
    for component in components {
        // A dependency-only component is one the launcher pulled in to satisfy
        // another, not something the user chose.
        if component
            .get("dependencyOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let uid = component.get("uid").and_then(Value::as_str).unwrap_or("");
        let version = component
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if uid == MINECRAFT {
            game_version = version;
        } else if let Some((_, name)) = LOADERS.iter().find(|(id, _)| *id == uid) {
            loader = Some((name.to_string(), version));
        }
    }

    if game_version.is_empty() {
        return Err(invalid(ArchiveFault::NoGameVersion).into());
    }

    Ok(Instance {
        name: setting(&config, "name").unwrap_or_default().to_string(),
        game_version,
        loader,
        memory_mb: overridden(&config, "OverrideMemory")
            .then(|| setting(&config, "MaxMemAlloc").and_then(|v| v.parse().ok()))
            .flatten(),
        jvm_args: overridden(&config, "OverrideJavaArgs")
            .then(|| setting(&config, "JvmArgs").filter(|v| !v.is_empty()))
            .flatten()
            .map(str::to_string),
    })
}

/// The game directory inside the instance, given every member name in the
/// archive under its root. `.minecraft/` is the modern layout and `minecraft/`
/// the older one; Prism itself prefers the former when both are present.
pub(crate) fn game_dir(names: &[String], prefix: &str) -> &'static str {
    let has = |dir: &str| {
        names
            .iter()
            .any(|name| name.starts_with(&format!("{prefix}{dir}/")))
    };
    match has(".minecraft") || !has("minecraft") {
        true => ".minecraft",
        false => "minecraft",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACK_FABRIC: &str = r#"{
        "formatVersion": 1,
        "components": [
            { "uid": "net.fabricmc.intermediary", "version": "1.21.1", "dependencyOnly": true },
            { "uid": "org.lwjgl3", "version": "3.3.3", "dependencyOnly": true },
            { "uid": "net.minecraft", "version": "1.21.1", "important": true },
            { "uid": "net.fabricmc.fabric-loader", "version": "0.16.9" }
        ]
    }"#;

    #[test]
    fn components_name_the_game_version_and_the_loader() {
        let instance = read("name=Cozy\n", PACK_FABRIC).unwrap();
        assert_eq!(instance.name, "Cozy");
        assert_eq!(instance.game_version, "1.21.1");
        assert_eq!(
            instance.loader,
            Some(("fabric".to_string(), "0.16.9".to_string()))
        );
    }

    #[test]
    fn a_pack_with_no_loader_component_is_vanilla() {
        let pack = r#"{"components":[{"uid":"net.minecraft","version":"1.20.4"}]}"#;
        let instance = read("name=Plain\n", pack).unwrap();
        assert_eq!(instance.game_version, "1.20.4");
        assert_eq!(instance.loader, None);
    }

    #[test]
    fn settings_are_carried_only_when_the_instance_overrode_them() {
        let overriding = "name=Cozy\nOverrideMemory=true\nMaxMemAlloc=6144\n\
                          OverrideJavaArgs=true\nJvmArgs=-XX:+UseG1GC\n";
        let instance = read(overriding, PACK_FABRIC).unwrap();
        assert_eq!(instance.memory_mb, Some(6144));
        assert_eq!(instance.jvm_args.as_deref(), Some("-XX:+UseG1GC"));

        let inheriting = "name=Cozy\nMaxMemAlloc=6144\nJvmArgs=-XX:+UseG1GC\n";
        let instance = read(inheriting, PACK_FABRIC).unwrap();
        assert_eq!(
            instance.memory_mb, None,
            "without the override flag those are the launcher's defaults, not the instance's"
        );
        assert_eq!(instance.jvm_args, None);
    }

    #[test]
    fn a_config_with_sections_and_comments_still_parses() {
        let text = "[General]\n# a comment\nname = Cozy\nInstanceType=OneSix\n";
        let config = parse_config(text);
        assert_eq!(setting(&config, "name"), Some("Cozy"));
        assert_eq!(setting(&config, "InstanceType"), Some("OneSix"));
    }

    #[test]
    fn a_pack_with_no_minecraft_component_is_refused() {
        let pack = r#"{"components":[{"uid":"net.fabricmc.fabric-loader","version":"0.16.9"}]}"#;
        assert!(read("name=x\n", pack).is_err());
        assert!(read("name=x\n", "not json").is_err());
    }

    #[test]
    fn overridden_settings_become_jvm_settings() {
        let instance = Instance {
            name: "Cozy".to_string(),
            game_version: "1.21.1".to_string(),
            loader: None,
            memory_mb: Some(6144),
            jvm_args: Some("-XX:+UseG1GC".to_string()),
        };
        let jvm = jvm_settings(&instance);
        assert_eq!(jvm.get(MEMORY_KEY).unwrap().as_deref(), Some("6G"));
        assert_eq!(
            jvm.get(JVM_ARGS_KEY).unwrap().as_deref(),
            Some("-XX:+UseG1GC")
        );
    }

    #[test]
    fn a_jvm_setting_hestia_rejects_is_dropped_not_fatal() {
        let instance = Instance {
            name: "Cozy".to_string(),
            game_version: "1.21.1".to_string(),
            loader: None,
            memory_mb: None,
            jvm_args: Some("not-a-flag".to_string()),
        };
        let jvm = jvm_settings(&instance);
        assert_eq!(jvm.get(JVM_ARGS_KEY).unwrap(), None);
    }

    #[test]
    fn the_game_directory_is_whichever_layout_the_archive_used() {
        let modern = vec!["p/.minecraft/options.txt".to_string()];
        assert_eq!(game_dir(&modern, "p/"), ".minecraft");

        let legacy = vec!["p/minecraft/options.txt".to_string()];
        assert_eq!(game_dir(&legacy, "p/"), "minecraft");

        let both = vec![
            "p/minecraft/options.txt".to_string(),
            "p/.minecraft/options.txt".to_string(),
        ];
        assert_eq!(game_dir(&both, "p/"), ".minecraft");

        assert_eq!(
            game_dir(&[], ""),
            ".minecraft",
            "an empty instance is modern"
        );
    }
}
