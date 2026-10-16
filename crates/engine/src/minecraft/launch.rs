//! Turns a resolved profile plus materialised paths into the JVM invocation:
//! classpath assembly and Mojang `${placeholder}` substitution. Pure functions —
//! spawning is the daemon supervisor's job.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use proto::instance::QuickPlay;
use serde::{Deserialize, Serialize};

use super::profile::{InstanceProfile, ServerProfile};

// Mojang's rule vocabulary spells the classpath separator per-OS; keep ours in
// lockstep with the JVM's.
const CLASSPATH_SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

/// The reserved per-entry JVM setting keys, shared by the server and instance
/// `config` surfaces.
pub const MEMORY_KEY: &str = "memory";
pub const JVM_ARGS_KEY: &str = "jvm-args";

/// Per-entry JVM tuning stored on a server/instance record: a single memory
/// value driving both `-Xms`/`-Xmx`, plus extra flags injected at launch.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct JavaSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub jvm_args: Vec<String>,
}

impl JavaSettings {
    /// The current value of a JVM key, or `None` (outer) when `key` is not one
    /// of the reserved JVM keys — the signal to fall through to properties. The
    /// inner `None` means the key is a JVM key but unset.
    pub fn get(&self, key: &str) -> Option<Option<String>> {
        match key {
            MEMORY_KEY => Some(self.memory.clone()),
            JVM_ARGS_KEY => Some(if self.jvm_args.is_empty() {
                None
            } else {
                Some(self.jvm_args.join(" "))
            }),
            _ => None,
        }
    }

    /// Apply a JVM key. `Ok(false)` means `key` is not a JVM key (fall through
    /// to properties); an empty value clears the setting; an invalid value is
    /// `Err`.
    pub fn set(&mut self, key: &str, value: &str) -> Result<bool> {
        match key {
            MEMORY_KEY => {
                self.memory = if value.trim().is_empty() {
                    None
                } else {
                    Some(normalize_memory(value)?)
                };
                Ok(true)
            }
            JVM_ARGS_KEY => {
                self.jvm_args = parse_jvm_args(value)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Both reserved keys with their current values (empty when unset), so a
    /// `config list` always shows what is settable.
    pub fn entries(&self) -> Vec<(String, String)> {
        vec![
            (
                MEMORY_KEY.to_string(),
                self.memory.clone().unwrap_or_default(),
            ),
            (JVM_ARGS_KEY.to_string(), self.jvm_args.join(" ")),
        ]
    }

    /// These settings with unset fields filled from `defaults` — how the
    /// launcher-wide `defaults.*` config keys reach an entry that never set
    /// its own.
    pub fn or_defaults(&self, defaults: &JavaSettings) -> JavaSettings {
        JavaSettings {
            memory: self.memory.clone().or_else(|| defaults.memory.clone()),
            jvm_args: if self.jvm_args.is_empty() {
                defaults.jvm_args.clone()
            } else {
                self.jvm_args.clone()
            },
        }
    }

    /// The JVM flags these settings inject: the memory pair first, then the
    /// extra args.
    pub fn flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if let Some(memory) = &self.memory {
            flags.push(format!("-Xms{memory}"));
            flags.push(format!("-Xmx{memory}"));
        }
        flags.extend(self.jvm_args.iter().cloned());
        flags
    }
}

/// Validate and normalise a memory value: digits followed by one unit char
/// (k/m/g, case-insensitive), the number nonzero, the unit upper-cased and
/// carried as far up the ladder as it divides exactly — an import that speaks
/// megabytes (Prism's `MaxMemAlloc=8192`) stores the `8G` the user chose. An
/// amount above this machine's installed memory is rejected.
pub(crate) fn normalize_memory(value: &str) -> Result<String> {
    normalize_memory_within(value, crate::host::total_memory())
}

fn normalize_memory_within(value: &str, limit: Option<u64>) -> Result<String> {
    let trimmed = value.trim();
    let unit = trimmed
        .chars()
        .last()
        .filter(|c| "kmgKMG".contains(*c))
        .ok_or(proto::error::ErrorInfo::InvalidValue {
            field: proto::error::Field::Memory,
            reason: proto::error::Reason::MemoryFormat,
        })?;
    let digits = &trimmed[..trimmed.len() - unit.len_utf8()];
    let valid = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.bytes().any(|b| b != b'0');
    if !valid {
        bail!(proto::error::ErrorInfo::InvalidValue {
            field: proto::error::Field::Memory,
            reason: proto::error::Reason::MemoryFormat
        });
    }
    let unit = unit.to_ascii_uppercase();
    if limit.is_some_and(|limit| bytes(digits, unit).is_none_or(|bytes| bytes > limit)) {
        bail!(proto::error::ErrorInfo::InvalidValue {
            field: proto::error::Field::Memory,
            reason: proto::error::Reason::MemoryExceedsHost
        });
    }
    Ok(promote(digits, unit))
}

const LADDER: [char; 3] = ['K', 'M', 'G'];

/// The amount in bytes, or `None` when it does not fit a `u64`.
fn bytes(digits: &str, unit: char) -> Option<u64> {
    let rung = LADDER.iter().position(|u| *u == unit)? as u32;
    digits
        .parse::<u64>()
        .ok()?
        .checked_mul(1024u64.checked_pow(rung + 1)?)
}

/// Carry an amount up the unit ladder while it divides exactly. A number too
/// large for `u64` is left as written — the JVM is the one that rejects it.
fn promote(digits: &str, unit: char) -> String {
    let Ok(mut amount) = digits.parse::<u64>() else {
        return format!("{digits}{unit}");
    };
    let mut at = LADDER
        .iter()
        .position(|u| *u == unit)
        .unwrap_or(LADDER.len());
    while at + 1 < LADDER.len() && amount % 1024 == 0 {
        amount /= 1024;
        at += 1;
    }
    format!("{amount}{}", LADDER.get(at).copied().unwrap_or(unit))
}

/// Split JVM args on whitespace; every token must start with `-`.
pub(crate) fn parse_jvm_args(value: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    for token in value.split_whitespace() {
        if !token.starts_with('-') {
            bail!(proto::error::ErrorInfo::InvalidValue {
                field: proto::error::Field::JvmArgs,
                reason: proto::error::Reason::JvmArgsPrefix
            });
        }
        args.push(token.to_string());
    }
    Ok(args)
}

/// A ready-to-spawn process: program + args + working directory.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The signed-in identity substituted into the client's auth placeholders.
pub struct LaunchAccount {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
}

/// The token an offline launch runs on. Deliberately not a real one: the game
/// opens singleplayer with it and is refused by every authenticated service,
/// which is exactly the entitlement an unverified session should have.
pub const OFFLINE_TOKEN: &str = "0";

/// Where the materialised instance pieces live.
pub struct InstancePaths<'a> {
    pub game_dir: &'a Path,
    pub natives_dir: &'a Path,
    pub client_jar: &'a Path,
    pub libraries_root: &'a Path,
    pub assets_root: &'a Path,
    /// What `${game_assets}` resolves to: the mirrored tree for a legacy index
    /// (`materialize::ensure_assets`), otherwise `assets_root`.
    pub game_assets: &'a Path,
    /// A per-session Log4j2 config (`crate::minecraft::log4j`) redirecting the
    /// game's own log to a private file; `None` keeps the version's default
    /// (`logs/latest.log`).
    pub log_config: Option<&'a Path>,
    /// Where the game reports what the session joins
    /// (`crate::minecraft::quickplay`); `None` for a client too old to.
    pub quick_play_log: Option<&'a Path>,
}

/// The server invocation, run from the server's data directory. A profile without a
/// main class is a self-contained jar (`-jar <primary> nogui`); one with a main
/// class runs off a classpath of its libraries plus the primary.
pub fn server_plan(
    profile: &ServerProfile,
    java: &Path,
    dir: &Path,
    settings: &JavaSettings,
) -> LaunchPlan {
    server_invocation(profile, java, Path::new(""), dir, settings)
}

/// The same invocation, but with every artifact named relative to `artifacts`
/// and run from `cwd` — how the schema-generation run boots the server in a
/// throwaway directory instead of the game's own data dir.
///
/// A generated argument file is deliberately **not** used here: it names its
/// libraries relative to the launch directory, so it only resolves from the data
/// dir, and a loader that reads it gates on the EULA before writing any
/// properties. The profile's primary artifact is the vanilla server jar for
/// exactly this game version, which is what the key set describes.
pub fn server_schema_plan(
    profile: &ServerProfile,
    java: &Path,
    artifacts: &Path,
    cwd: &Path,
    settings: &JavaSettings,
) -> LaunchPlan {
    let profile = ServerProfile {
        args_file: String::new(),
        ..profile.clone()
    };
    server_invocation(&profile, java, artifacts, cwd, settings)
}

fn server_invocation(
    profile: &ServerProfile,
    java: &Path,
    artifacts: &Path,
    cwd: &Path,
    settings: &JavaSettings,
) -> LaunchPlan {
    let mut args = settings.flags();
    let primary = join_str(artifacts, &profile.primary.filename);
    if !profile.args_file.is_empty() {
        // The file names its libraries relative to the launch directory, so it
        // is passed relative too and never resolved against `artifacts`.
        args.push(format!("@{}", profile.args_file));
    } else if profile.main_class.is_empty() {
        args.push("-jar".to_string());
        args.push(primary);
    } else {
        let mut entries: Vec<String> = Vec::new();
        for library in &profile.libraries {
            push_unique(
                &mut entries,
                join_str(&artifacts.join("libraries"), &library.path),
            );
        }
        entries.push(primary);
        args.push("-cp".to_string());
        args.push(entries.join(CLASSPATH_SEPARATOR));
        args.push(profile.main_class.clone());
    }
    args.push("nogui".to_string());
    LaunchPlan {
        program: java.to_path_buf(),
        args,
        cwd: cwd.to_path_buf(),
    }
}

/// The first release that understands the Quick Play arguments (1.20, snapshot
/// 23w14a). An older client parses them as unknown options and opens to the
/// title screen instead, so a launch that asked to join something is refused
/// rather than quietly ignored.
pub const QUICK_PLAY_SINCE: (u64, u64, u64) = (1, 20, 0);

/// Whether `game_version` takes the Quick Play arguments. A version that does
/// not parse as a release triple — a snapshot, an April Fools' build — answers
/// no: it may well support them, but nothing here can tell, and refusing is the
/// answer that cannot silently drop the player at the title screen.
pub fn supports_quick_play(game_version: &str) -> bool {
    crate::version::parse(game_version).is_some_and(|v| v >= QUICK_PLAY_SINCE)
}

/// The client invocation: substituted JVM args, the main class, then
/// substituted game args, run from the instance's game directory. `quick_play`
/// appends the join-on-start argument; the caller has already checked the
/// version takes it.
pub fn instance_plan(
    profile: &InstanceProfile,
    java: &Path,
    paths: &InstancePaths<'_>,
    account: &LaunchAccount,
    settings: &JavaSettings,
    quick_play: Option<&QuickPlay>,
) -> LaunchPlan {
    let classpath = build_classpath(profile, paths);
    let vars = build_vars(profile, paths, account, classpath);

    let mut args = Vec::new();
    if profile.jvm_args.is_empty() {
        // Pre-`arguments` manifests carry no JVM section; supply what one would
        // have declared. That era's natives are unpacked, not read from a jar.
        args.push(format!(
            "-Djava.library.path={}",
            paths.natives_dir.to_string_lossy()
        ));
        args.push("-cp".to_string());
        args.push(vars["classpath"].clone());
    } else {
        args.extend(profile.jvm_args.iter().map(|a| substitute(a, &vars)));
    }
    // Per-session log routing sits in the JVM section, before the main class.
    if let Some(config) = paths.log_config {
        args.push(format!(
            "{}{}",
            super::log4j::CONFIG_PROPERTY,
            config.to_string_lossy()
        ));
        args.push(super::log4j::NO_LOOKUPS_PROPERTY.to_string());
    }
    // User flags come last in the JVM section so they win over the manifest.
    args.extend(settings.flags());
    args.push(profile.main_class.clone());
    args.extend(profile.game_args.iter().map(|a| substitute(a, &vars)));
    // The manifest declares these behind feature rules, which resolve to
    // nothing here (see `meta::rules_allow`), so the pair is appended directly.
    if let Some(log) = paths.quick_play_log {
        args.push(super::quickplay::ARGUMENT.to_string());
        args.push(log.to_string_lossy().into_owned());
    }
    if let Some(quick_play) = quick_play {
        let (flag, target) = match quick_play {
            QuickPlay::World(folder) => ("--quickPlaySingleplayer", folder),
            QuickPlay::Server(address) => ("--quickPlayMultiplayer", address),
            QuickPlay::Realm(id) => ("--quickPlayRealms", id),
        };
        args.push(flag.to_string());
        args.push(target.clone());
    }

    LaunchPlan {
        program: java.to_path_buf(),
        args,
        cwd: paths.game_dir.to_path_buf(),
    }
}

fn build_classpath(profile: &InstanceProfile, paths: &InstancePaths<'_>) -> String {
    let mut entries: Vec<String> = Vec::new();
    for library in &profile.libraries {
        push_unique(&mut entries, join_str(paths.libraries_root, &library.path));
    }
    entries.push(paths.client_jar.to_string_lossy().into_owned());
    entries.join(CLASSPATH_SEPARATOR)
}

fn build_vars(
    profile: &InstanceProfile,
    paths: &InstancePaths<'_>,
    account: &LaunchAccount,
    classpath: String,
) -> HashMap<&'static str, String> {
    let path_str = |p: &Path| p.to_string_lossy().into_owned();
    HashMap::from([
        ("classpath", classpath),
        // A modloader that boots off the module path (NeoForge's
        // BootstrapLauncher) names its jars by these rather than by the
        // classpath. An unknown placeholder substitutes to empty, so omitting
        // them would silently produce a broken `-p` argument.
        ("library_directory", path_str(paths.libraries_root)),
        ("classpath_separator", CLASSPATH_SEPARATOR.to_string()),
        ("natives_directory", path_str(paths.natives_dir)),
        ("game_directory", path_str(paths.game_dir)),
        ("assets_root", path_str(paths.assets_root)),
        ("game_assets", path_str(paths.game_assets)),
        ("assets_index_name", profile.asset_index.id.clone()),
        ("version_name", profile.game_version.clone()),
        ("version_type", "release".to_string()),
        ("launcher_name", common::app::NAME.to_string()),
        ("launcher_version", common::app::VERSION.to_string()),
        ("auth_player_name", account.name.clone()),
        ("auth_uuid", account.uuid.clone()),
        ("auth_access_token", account.access_token.clone()),
        ("auth_session", account.access_token.clone()),
        ("user_type", "msa".to_string()),
        ("user_properties", "{}".to_string()),
        ("clientid", String::new()),
        ("auth_xuid", String::new()),
    ])
}

/// Replace every `${key}` with its variable; an unknown key becomes empty (and
/// is logged) rather than leaking the placeholder to the JVM.
fn substitute(arg: &str, vars: &HashMap<&'static str, String>) -> String {
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(value) => out.push_str(value),
                    None => tracing::debug!(key, "unknown launch placeholder"),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn join_str(root: &Path, relative: &str) -> String {
    root.join(relative).to_string_lossy().into_owned()
}

fn push_unique(entries: &mut Vec<String>, entry: String) {
    if !entries.contains(&entry) {
        entries.push(entry);
    }
}

#[cfg(test)]
mod tests {
    use proto::minecraft::Artifact;

    use super::super::profile::{AssetIndex, Library};
    use super::*;

    fn account() -> LaunchAccount {
        LaunchAccount {
            name: "Steve".into(),
            uuid: "uuid-1".into(),
            access_token: "token-1".into(),
        }
    }

    fn library(path: &str) -> Library {
        Library {
            name: path.to_string(),
            path: path.to_string(),
            artifact: Artifact::default(),
        }
    }

    #[test]
    fn substitute_replaces_known_and_drops_unknown() {
        let vars = HashMap::from([("version_name", "1.21.1".to_string())]);
        assert_eq!(
            substitute("--version=${version_name}", &vars),
            "--version=1.21.1"
        );
        assert_eq!(substitute("${unknown_key}", &vars), "");
        assert_eq!(substitute("${broken", &vars), "${broken");
        assert_eq!(substitute("plain", &vars), "plain");
    }

    #[test]
    fn an_args_file_replaces_the_jar_invocation() {
        let mut profile = ServerProfile {
            args_file: "libraries/net/neoforged/neoforge/21.1.244/unix_args.txt".into(),
            ..ServerProfile::default()
        };
        profile.primary.filename = "server.jar".into();
        let plan = server_plan(
            &profile,
            Path::new("/java"),
            Path::new("/srv"),
            &JavaSettings::default(),
        );
        assert_eq!(
            plan.args,
            [
                "@libraries/net/neoforged/neoforge/21.1.244/unix_args.txt",
                "nogui"
            ],
            "the file is passed relative — it names its own libraries that way"
        );
    }

    #[test]
    fn the_schema_run_ignores_the_args_file() {
        let mut profile = ServerProfile {
            args_file: "libraries/net/neoforged/neoforge/21.1.244/unix_args.txt".into(),
            ..ServerProfile::default()
        };
        profile.primary.filename = "server.jar".into();
        let plan = server_schema_plan(
            &profile,
            Path::new("/java"),
            Path::new("/srv/data"),
            Path::new("/srv/.schema"),
            &JavaSettings::default(),
        );
        assert_eq!(
            plan.args,
            [
                "-jar",
                &join_str(Path::new("/srv/data"), "server.jar"),
                "nogui"
            ],
            "the relative args file cannot resolve from the scratch dir, and its \
             loader gates on the EULA before writing properties"
        );
        assert_eq!(plan.cwd, Path::new("/srv/.schema"));
    }

    #[test]
    fn server_plan_without_main_class_is_a_jar_invocation() {
        let profile = ServerProfile {
            primary: Artifact {
                filename: "server.jar".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let plan = server_plan(
            &profile,
            Path::new("/java/bin/java"),
            Path::new("/srv"),
            &JavaSettings::default(),
        );
        assert_eq!(plan.args, ["-jar", "server.jar", "nogui"]);
        assert_eq!(plan.cwd, Path::new("/srv"));
    }

    #[test]
    fn server_plan_with_main_class_builds_a_classpath() {
        let profile = ServerProfile {
            primary: Artifact {
                filename: "launcher.jar".into(),
                ..Default::default()
            },
            libraries: vec![library("a/b.jar"), library("a/b.jar"), library("c/d.jar")],
            main_class: "net.example.Main".into(),
            ..Default::default()
        };
        let plan = server_plan(
            &profile,
            Path::new("java"),
            Path::new("/srv"),
            &JavaSettings::default(),
        );
        assert_eq!(plan.args[0], "-cp");
        let classpath = &plan.args[1];
        assert_eq!(classpath.matches("b.jar").count(), 1, "duplicates dropped");
        assert!(classpath.contains("launcher.jar"));
        assert_eq!(plan.args[2], "net.example.Main");
        assert_eq!(plan.args.last().map(String::as_str), Some("nogui"));
    }

    #[test]
    fn server_schema_plan_names_artifacts_outside_its_cwd() {
        let profile = ServerProfile {
            primary: Artifact {
                filename: "server.jar".into(),
                ..Default::default()
            },
            libraries: vec![library("a/b.jar")],
            main_class: "net.example.Main".into(),
            ..Default::default()
        };
        let plan = server_schema_plan(
            &profile,
            Path::new("java"),
            Path::new("/srv/data"),
            Path::new("/srv/.schema"),
            &JavaSettings::default(),
        );
        // The run happens in the scratch dir, so nothing it writes can land in
        // the game's data dir — but it still boots that dir's jar.
        assert_eq!(plan.cwd, Path::new("/srv/.schema"));
        let classpath = &plan.args[1];
        assert!(classpath.contains(&join_str(Path::new("/srv/data"), "server.jar")));
        assert!(classpath.contains(&join_str(
            &Path::new("/srv/data").join("libraries"),
            "a/b.jar"
        )));
    }

    #[test]
    fn instance_plan_substitutes_and_orders_sections() {
        let profile = InstanceProfile {
            game_version: "1.21.1".into(),
            client: Artifact::default(),
            libraries: vec![library("a/b.jar")],
            asset_index: AssetIndex {
                id: "17".into(),
                ..Default::default()
            },
            main_class: "net.minecraft.client.main.Main".into(),
            jvm_args: vec!["-cp".into(), "${classpath}".into()],
            game_args: vec!["--username".into(), "${auth_player_name}".into()],
            ..Default::default()
        };
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/1.21.1/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: Path::new("/assets"),
            log_config: None,
            quick_play_log: None,
        };
        let plan = instance_plan(
            &profile,
            Path::new("java"),
            &paths,
            &account(),
            &JavaSettings::default(),
            None,
        );
        let main_at = plan
            .args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .expect("main class present");
        assert!(plan.args[..main_at]
            .iter()
            .any(|a| a.contains("client.jar")));
        assert_eq!(plan.args[main_at + 1..], ["--username", "Steve"]);
        assert_eq!(plan.cwd, Path::new("/inst"));
    }

    #[test]
    fn instance_plan_appends_the_quick_play_target_after_the_game_args() {
        let profile = InstanceProfile {
            client: Artifact::default(),
            main_class: "net.minecraft.client.main.Main".into(),
            jvm_args: vec!["-cp".into(), "${classpath}".into()],
            game_args: vec!["--username".into(), "${auth_player_name}".into()],
            ..Default::default()
        };
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: Path::new("/assets"),
            log_config: None,
            quick_play_log: None,
        };
        let plan_for = |target: QuickPlay| {
            instance_plan(
                &profile,
                Path::new("java"),
                &paths,
                &account(),
                &JavaSettings::default(),
                Some(&target),
            )
            .args
        };

        let world = plan_for(QuickPlay::World("New World".into()));
        assert_eq!(
            world[world.len() - 4..],
            [
                "--username",
                "Steve",
                "--quickPlaySingleplayer",
                "New World"
            ],
            "the target follows the manifest's own game args"
        );
        let server = plan_for(QuickPlay::Server("smp.example.net:25566".into()));
        assert_eq!(
            server[server.len() - 2..],
            ["--quickPlayMultiplayer", "smp.example.net:25566"]
        );
        let realm = plan_for(QuickPlay::Realm("1234567".into()));
        assert_eq!(realm[realm.len() - 2..], ["--quickPlayRealms", "1234567"]);
    }

    #[test]
    fn instance_plan_names_the_quick_play_log_whether_or_not_it_joins_anything() {
        let profile = InstanceProfile {
            client: Artifact::default(),
            main_class: "net.minecraft.client.main.Main".into(),
            ..Default::default()
        };
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: Path::new("/assets"),
            log_config: None,
            quick_play_log: Some(Path::new("/logs/session-1.quickplay.json")),
        };

        let args = instance_plan(
            &profile,
            Path::new("java"),
            &paths,
            &account(),
            &JavaSettings::default(),
            None,
        )
        .args;

        assert_eq!(
            args[args.len() - 2..],
            ["--quickPlayPath", "/logs/session-1.quickplay.json"],
            "a session that opens to the title screen still reports what it joins from there"
        );
    }

    #[test]
    fn quick_play_needs_1_20_and_a_version_it_can_read() {
        assert!(supports_quick_play("1.20"));
        assert!(supports_quick_play("1.21.1"));
        assert!(!supports_quick_play("1.19.4"));
        assert!(!supports_quick_play("1.7.10"));
        assert!(
            !supports_quick_play("23w14a"),
            "a snapshot cannot be placed, and a launch that would open the title screen instead \
             is worse than a refusal"
        );
    }

    #[test]
    fn instance_plan_injects_log_config_before_main_class() {
        let profile = InstanceProfile {
            client: Artifact::default(),
            main_class: "net.minecraft.client.main.Main".into(),
            jvm_args: vec!["-cp".into(), "${classpath}".into()],
            ..Default::default()
        };
        let log = Path::new("/inst/logs/session-2.log");
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/1.21.1/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: Path::new("/assets"),
            log_config: Some(log),
            quick_play_log: None,
        };
        let plan = instance_plan(
            &profile,
            Path::new("java"),
            &paths,
            &account(),
            &JavaSettings::default(),
            None,
        );
        let main_at = plan
            .args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .expect("main class present");
        let config_at = plan
            .args
            .iter()
            .position(|a| a == "-Dlog4j.configurationFile=/inst/logs/session-2.log")
            .expect("log config arg present");
        assert!(config_at < main_at, "log config sits in the JVM section");
        assert!(plan
            .args
            .iter()
            .any(|a| a == "-Dlog4j2.formatMsgNoLookups=true"));
    }

    #[test]
    fn instance_plan_supplies_classpath_when_jvm_args_absent() {
        let profile = InstanceProfile {
            main_class: "net.minecraft.client.Minecraft".into(),
            ..Default::default()
        };
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/old/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: Path::new("/assets"),
            log_config: None,
            quick_play_log: None,
        };
        let plan = instance_plan(
            &profile,
            Path::new("java"),
            &paths,
            &account(),
            &JavaSettings::default(),
            None,
        );
        assert_eq!(
            plan.args[0],
            format!(
                "-Djava.library.path={}",
                Path::new("/inst/natives").display()
            ),
            "the era with no JVM section still needs its unpacked natives found"
        );
        assert_eq!(plan.args[1], "-cp");
        assert!(plan.args[2].contains("client.jar"));
    }

    #[test]
    fn legacy_game_args_point_at_the_mirrored_asset_tree() {
        let profile = InstanceProfile {
            game_args: vec!["--assetsDir".into(), "${game_assets}".into()],
            ..Default::default()
        };
        let mirrored = Path::new("/assets/virtual/legacy");
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/old/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: mirrored,
            log_config: None,
            quick_play_log: None,
        };
        let plan = instance_plan(
            &profile,
            Path::new("java"),
            &paths,
            &account(),
            &JavaSettings::default(),
            None,
        );
        assert_eq!(plan.args.last().unwrap(), &mirrored.display().to_string());
    }

    fn settings(memory: Option<&str>, jvm_args: &[&str]) -> JavaSettings {
        JavaSettings {
            memory: memory.map(str::to_string),
            jvm_args: jvm_args.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn or_defaults_fills_only_unset_fields() {
        let defaults = settings(Some("8G"), &["-XX:+UseZGC"]);
        let merged = settings(None, &[]).or_defaults(&defaults);
        assert_eq!(merged.memory.as_deref(), Some("8G"));
        assert_eq!(merged.jvm_args, ["-XX:+UseZGC"]);

        let own = settings(Some("2G"), &["-XX:+UseG1GC"]).or_defaults(&defaults);
        assert_eq!(own.memory.as_deref(), Some("2G"));
        assert_eq!(own.jvm_args, ["-XX:+UseG1GC"]);
    }

    #[test]
    fn server_plan_prepends_memory_and_extra_flags() {
        let profile = ServerProfile {
            primary: Artifact {
                filename: "server.jar".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let plan = server_plan(
            &profile,
            Path::new("java"),
            Path::new("/srv"),
            &settings(Some("4G"), &["-XX:+UseG1GC"]),
        );
        assert_eq!(
            plan.args,
            [
                "-Xms4G",
                "-Xmx4G",
                "-XX:+UseG1GC",
                "-jar",
                "server.jar",
                "nogui"
            ]
        );
    }

    #[test]
    fn instance_plan_appends_user_flags_after_manifest_jvm_args() {
        let profile = InstanceProfile {
            client: Artifact::default(),
            main_class: "net.minecraft.client.main.Main".into(),
            jvm_args: vec!["-cp".into(), "${classpath}".into()],
            ..Default::default()
        };
        let paths = InstancePaths {
            game_dir: Path::new("/inst"),
            natives_dir: Path::new("/inst/natives"),
            client_jar: Path::new("/versions/1.21.1/client.jar"),
            libraries_root: Path::new("/libraries"),
            assets_root: Path::new("/assets"),
            game_assets: Path::new("/assets"),
            log_config: None,
            quick_play_log: None,
        };
        let plan = instance_plan(
            &profile,
            Path::new("java"),
            &paths,
            &account(),
            &settings(Some("2048M"), &["-XX:+UseZGC"]),
            None,
        );
        let main_at = plan
            .args
            .iter()
            .position(|a| a == "net.minecraft.client.main.Main")
            .expect("main class present");
        // The injected flags are the three args immediately before the main
        // class, after the manifest's jvm section.
        assert_eq!(
            &plan.args[main_at - 3..main_at],
            ["-Xms2048M", "-Xmx2048M", "-XX:+UseZGC"]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .as_slice()
        );
        let cp_at = plan.args.iter().position(|a| a == "-cp").unwrap();
        assert!(cp_at < main_at - 3, "manifest jvm args precede user flags");
    }

    #[test]
    fn memory_validation_normalizes_accepts_and_rejects() {
        let mut s = JavaSettings::default();
        assert!(s.set(MEMORY_KEY, "4g").unwrap());
        assert_eq!(s.memory.as_deref(), Some("4G"));
        assert_eq!(s.flags(), ["-Xms4G", "-Xmx4G"]);
        assert!(s.set(MEMORY_KEY, "2048M").unwrap());
        assert_eq!(s.memory.as_deref(), Some("2G"));
        // Only an exact multiple is promoted; the rest keeps its own unit.
        assert!(s.set(MEMORY_KEY, "6656M").unwrap());
        assert_eq!(s.memory.as_deref(), Some("6656M"));
        assert!(s.set(MEMORY_KEY, "1048576k").unwrap());
        assert_eq!(s.memory.as_deref(), Some("1G"));
        // Empty clears.
        assert!(s.set(MEMORY_KEY, "").unwrap());
        assert_eq!(s.memory, None);
        for bad in ["4", "G", "0G", "4GB", "4.5G", "-4G", "four"] {
            assert!(s.set(MEMORY_KEY, bad).is_err(), "'{bad}' must be rejected");
        }
    }

    #[test]
    fn memory_above_the_installed_amount_is_rejected() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let limit = Some(16 * GIB - 1);
        assert_eq!(normalize_memory_within("15G", limit).unwrap(), "15G");
        assert_eq!(normalize_memory_within("15360M", limit).unwrap(), "15G");
        for over in ["16G", "16384M", "32G", "99999999999999999999G"] {
            assert!(
                normalize_memory_within(over, limit).is_err(),
                "'{over}' must be rejected"
            );
        }
        assert_eq!(
            normalize_memory_within("16G", Some(16 * GIB)).unwrap(),
            "16G"
        );
        assert_eq!(normalize_memory_within("64G", None).unwrap(), "64G");
    }

    #[test]
    fn jvm_args_split_validate_and_clear() {
        let mut s = JavaSettings::default();
        assert!(s.set(JVM_ARGS_KEY, "-XX:+UseG1GC  -Xmn1G").unwrap());
        assert_eq!(s.jvm_args, ["-XX:+UseG1GC", "-Xmn1G"]);
        assert_eq!(
            s.get(JVM_ARGS_KEY),
            Some(Some("-XX:+UseG1GC -Xmn1G".into()))
        );
        assert!(s.set(JVM_ARGS_KEY, "notaflag").is_err());
        assert!(s.set(JVM_ARGS_KEY, "").unwrap());
        assert!(s.jvm_args.is_empty());
        assert_eq!(s.get(JVM_ARGS_KEY), Some(None));
    }

    #[test]
    fn get_returns_outer_none_for_non_jvm_key() {
        assert_eq!(JavaSettings::default().get("server-port"), None);
    }
}
