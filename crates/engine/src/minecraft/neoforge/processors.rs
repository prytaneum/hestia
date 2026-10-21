//! The NeoForge install processors: the step that turns a vanilla jar into the
//! patched one NeoForge actually runs.
//!
//! There is no downloadable patched jar. `install_profile.json` names a chain of
//! small Java tools — extract mappings, merge them, split the jar, remap it,
//! then apply a binary patch — that the official installer runs locally, and
//! whose final output (`net.neoforged:neoforge:<v>:{client,server}`) is what the
//! launch profile puts on the classpath. Every launcher that supports
//! Forge/NeoForge runs this chain itself; this follows theseus's shape.
//!
//! Two things differ from theseus, both because it reads Modrinth's
//! pre-processed metadata where this reads the installer directly: the data
//! table's `/data/*.lzma` binary patches must be extracted from the installer
//! (daedalus re-hosts them as maven coordinates, so theseus never sees the
//! form), and the substitution is side-aware — theseus is a client-only
//! launcher and reads only each entry's `client` value.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use crate::process::{self, ProcessSupervisor, Task};
use serde_json::Value;

use super::super::launch::CLASSPATH_SEPARATOR;
use super::super::materialize::OnProgress;
use super::super::meta::neoforge::Installer;
use super::super::provider::Side;

pub(super) fn side_key(side: Side) -> &'static str {
    match side {
        Side::Client => "client",
        Side::Server => "server",
    }
}

/// Where an install writes and what it starts from.
pub struct Install<'a> {
    /// `{ROOT}` — the install root. The processors write `libraries/` beneath
    /// it, so this is the shared `meta/` root for a client and the server's own
    /// data directory for a server (whose generated arg file names
    /// `libraries/…` relative to the directory it is launched from).
    pub root: &'a Path,
    /// `{INSTALLER}` — the installer jar itself, on disk: one processor reads
    /// files straight back out of it.
    pub installer: &'a Path,
    /// `{MINECRAFT_JAR}` — the vanilla jar for this side.
    pub minecraft_jar: &'a Path,
    pub side: Side,
    pub java: &'a Path,
    pub processes: &'a ProcessSupervisor,
}

impl Install<'_> {
    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }
}

/// Run every processor this side needs, in order.
///
/// Each processor is a checkpoint: they run sequentially and each is a separate
/// JVM, so a cancel between them costs only the work already done — and what it
/// leaves behind is exactly what a failed processor would have left, which the
/// caller's existing failure path already handles.
pub async fn run(
    installer: &Installer,
    ctx: &Install<'_>,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let processors = installer
        .profile
        .get("processors")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let wanted: Vec<&Value> = processors.iter().filter(|p| runs_on(p, ctx.side)).collect();
    if wanted.is_empty() {
        return Ok(());
    }

    let staged = stage_data_files(installer, ctx.root)?;
    let table = data_table(installer, ctx, &staged)?;
    let libraries = ctx.libraries();

    let total = wanted.len() as u64;
    for (index, processor) in wanted.iter().enumerate() {
        on_progress.check()?;
        let jar = processor
            .get("jar")
            .and_then(Value::as_str)
            .context("a neoforge processor names no jar")?;
        let jar_path = library_path(&libraries, jar)?;
        let main_class = main_class(&jar_path)
            .with_context(|| format!("processor {jar} declares no Main-Class"))?;

        let mut classpath: Vec<String> = processor
            .get("classpath")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|coord| library_path(&libraries, coord))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        classpath.push(jar_path.to_string_lossy().into_owned());

        let args = arguments(processor, &table, &libraries)?;

        on_progress.report(&ProvisionProgress {
            phase: ProvisionPhase::Libraries,
            current: index as u64,
            total,
            detail: short_name(jar),
            ..ProvisionProgress::default()
        });

        tracing::debug!(processor = jar, %main_class, "running neoforge processor");
        let mut command = vec![
            "-cp".to_string(),
            classpath.join(CLASSPATH_SEPARATOR),
            main_class,
        ];
        command.extend(args);
        let outcome = ctx
            .processes
            .run(
                Task {
                    id: format!("install-neoforge-{}-{index}", side_key(ctx.side)),
                    program: ctx.java,
                    args: command,
                    cwd: ctx.root.to_path_buf(),
                    phase: ProvisionPhase::Libraries,
                    narrates: process::silent,
                    deadline: None,
                },
                on_progress,
            )
            .await
            .with_context(|| format!("cannot run neoforge processor {jar}"))?;
        if !outcome.succeeded() {
            bail!("neoforge processor {jar} failed");
        }
    }
    on_progress.report(&ProvisionProgress {
        phase: ProvisionPhase::Libraries,
        current: total,
        total,
        ..ProvisionProgress::default()
    });
    Ok(())
}

/// Where this side's patched game jar lands, as the install profile's own data
/// table declares it.
///
/// Never guessed from a naming convention: NeoForge changed the coordinate
/// between installer generations — `net.neoforged:neoforge:<v>:server` for the
/// 1.x line, `net.neoforged:minecraft-server-patched:<v>` for the calendar-
/// versioned one. A hard-coded name silently stops matching, and since this is
/// the install's idempotence check, the cost of that is re-running the whole
/// processor chain on every single launch.
pub fn patched_output(installer: &Installer, side: Side, libraries: &Path) -> Option<PathBuf> {
    let raw = installer
        .profile
        .get("data")?
        .get("PATCHED")?
        .get(side_key(side))?
        .as_str()?;
    let coord = raw.strip_prefix('[')?.strip_suffix(']')?;
    library_path(libraries, coord).ok()
}

/// A processor with no `sides` runs on both.
fn runs_on(processor: &Value, side: Side) -> bool {
    match processor.get("sides").and_then(Value::as_array) {
        Some(sides) => sides
            .iter()
            .filter_map(Value::as_str)
            .any(|s| s == side_key(side)),
        None => true,
    }
}

/// Write the installer's own `data/` entries to disk, keyed by their
/// archive-relative name (`data/client.lzma`). The data table addresses the
/// binary patches by that path, and a processor takes a file, not a jar entry.
fn stage_data_files(installer: &Installer, root: &Path) -> Result<HashMap<String, PathBuf>> {
    let dir = root.join(".neoforge");
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let mut staged = HashMap::new();
    for (name, bytes) in &installer.data {
        let file = name.rsplit('/').next().unwrap_or(name);
        let path = dir.join(file);
        std::fs::write(&path, bytes).with_context(|| format!("cannot write {}", path.display()))?;
        staged.insert(format!("/{name}"), path);
    }
    Ok(staged)
}

/// The `{TOKEN}` vocabulary a processor's arguments substitute against: the
/// install profile's own data table resolved for this side, plus the four
/// tokens the installer supplies rather than declaring.
fn data_table(
    installer: &Installer,
    ctx: &Install<'_>,
    staged: &HashMap<String, PathBuf>,
) -> Result<HashMap<String, String>> {
    let libraries = ctx.libraries();
    let mut table = HashMap::new();
    if let Some(data) = installer.profile.get("data").and_then(Value::as_object) {
        for (key, sided) in data {
            let Some(raw) = sided.get(side_key(ctx.side)).and_then(Value::as_str) else {
                continue;
            };
            table.insert(key.clone(), resolve(raw, &libraries, staged)?);
        }
    }
    table.insert("ROOT".into(), path_string(ctx.root));
    table.insert("SIDE".into(), side_key(ctx.side).to_string());
    table.insert("MINECRAFT_JAR".into(), path_string(ctx.minecraft_jar));
    table.insert("INSTALLER".into(), path_string(ctx.installer));
    Ok(table)
}

/// One data-table value: a maven coordinate in brackets resolves to its library
/// path, a leading `/` names a file inside the installer, anything else is a
/// literal (a version string, say).
fn resolve(raw: &str, libraries: &Path, staged: &HashMap<String, PathBuf>) -> Result<String> {
    if let Some(coord) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return Ok(path_string(&library_path(libraries, coord)?));
    }
    if raw.starts_with('/') {
        let path = staged
            .get(raw)
            .with_context(|| format!("the neoforge installer carries no {raw}"))?;
        return Ok(path_string(path));
    }
    Ok(raw.to_string())
}

/// A processor's arguments, substituted. Each input argument maps to exactly one
/// output argument — a bracketed coordinate becomes a path, and `{TOKEN}`s are
/// replaced in place, since a token may be embedded (`{ROOT}/run.sh`) rather
/// than standing alone.
fn arguments(
    processor: &Value,
    table: &HashMap<String, String>,
    libraries: &Path,
) -> Result<Vec<String>> {
    let args = processor
        .get("args")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    args.iter()
        .filter_map(Value::as_str)
        .map(|arg| {
            if let Some(coord) = arg.strip_prefix('[').and_then(|a| a.strip_suffix(']')) {
                return Ok(path_string(&library_path(libraries, coord)?));
            }
            let mut out = arg.to_string();
            for (key, value) in table {
                out = out.replace(&format!("{{{key}}}"), value);
            }
            Ok(out)
        })
        .collect()
}

/// A maven coordinate's path under the libraries root.
fn library_path(libraries: &Path, coord: &str) -> Result<PathBuf> {
    let relative = super::super::meta::maven_path(coord)
        .with_context(|| format!("'{coord}' is not a maven coordinate"))?;
    Ok(libraries.join(relative))
}

/// A jar's `Main-Class`, read from its manifest — the install profile names the
/// processor jar but never its entry point.
fn main_class(jar: &Path) -> Result<String> {
    let file = std::fs::File::open(jar)
        .with_context(|| format!("cannot open processor {}", jar.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("processor {} is not a readable jar", jar.display()))?;
    let mut manifest = String::new();
    archive
        .by_name("META-INF/MANIFEST.MF")
        .with_context(|| format!("processor {} carries no manifest", jar.display()))?
        .read_to_string(&mut manifest)?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("Main-Class:"))
        .map(|value| value.trim().to_string())
        .context("no Main-Class entry")
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A coordinate's artifact id, for progress text — the full coordinate is noise.
fn short_name(coord: &str) -> String {
    coord.split(':').nth(1).unwrap_or(coord).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table() -> HashMap<String, String> {
        HashMap::from([
            ("ROOT".to_string(), "/srv".to_string()),
            ("SIDE".to_string(), "server".to_string()),
            ("PATCHED".to_string(), "/libs/patched.jar".to_string()),
        ])
    }

    #[test]
    fn sides_filter_the_chain() {
        let both = json!({ "jar": "a:b:1" });
        let client = json!({ "jar": "a:b:1", "sides": ["client"] });
        assert!(runs_on(&both, Side::Server), "no sides means both");
        assert!(runs_on(&client, Side::Client));
        assert!(!runs_on(&client, Side::Server));
    }

    #[test]
    fn a_token_substitutes_even_when_embedded() {
        let processor = json!({ "args": ["--to", "{ROOT}/run.sh", "--side", "{SIDE}"] });
        let args = arguments(&processor, &table(), Path::new("/libs")).unwrap();
        assert_eq!(args, ["--to", "/srv/run.sh", "--side", "server"]);
    }

    #[test]
    fn a_bracketed_coordinate_becomes_a_library_path() {
        let processor = json!({ "args": ["--input", "[net.neoforged:neoform:1.21.1@zip]"] });
        let libs = Path::new("/libs");
        let args = arguments(&processor, &table(), libs).unwrap();
        assert_eq!(
            args[1],
            path_string(&libs.join("net/neoforged/neoform/1.21.1/neoform-1.21.1.zip"))
        );
    }

    #[test]
    fn a_data_value_resolves_by_its_form() {
        let staged = HashMap::from([(
            "/data/server.lzma".to_string(),
            PathBuf::from("/tmp/s.lzma"),
        )]);
        let libs = Path::new("/libs");
        assert_eq!(
            resolve("[net.neoforged:neoforge:21.1.244:server]", libs, &staged).unwrap(),
            path_string(&libs.join("net/neoforged/neoforge/21.1.244/neoforge-21.1.244-server.jar"))
        );
        assert_eq!(
            resolve("/data/server.lzma", libs, &staged).unwrap(),
            "/tmp/s.lzma"
        );
        assert_eq!(
            resolve("'1.21.1-20240808'", libs, &staged).unwrap(),
            "'1.21.1-20240808'",
            "a literal is left alone"
        );
    }

    #[test]
    fn the_patched_jar_is_read_from_the_profile_not_a_convention() {
        let installer = |coord: &str| Installer {
            version: json!({}),
            profile: json!({ "data": { "PATCHED": { "client": coord, "server": coord } } }),
            data: Vec::new(),
        };
        let libs = Path::new("/libs");
        // The 1.x line.
        assert_eq!(
            patched_output(
                &installer("[net.neoforged:neoforge:21.1.244:server]"),
                Side::Server,
                libs
            )
            .unwrap(),
            Path::new("/libs/net/neoforged/neoforge/21.1.244/neoforge-21.1.244-server.jar")
        );
        // The calendar-versioned line renamed it; a hard-coded name would have
        // missed here and re-run the whole chain on every launch.
        assert_eq!(
            patched_output(
                &installer("[net.neoforged:minecraft-server-patched:26.1.2.87]"),
                Side::Server,
                libs
            )
            .unwrap(),
            Path::new(
                "/libs/net/neoforged/minecraft-server-patched/26.1.2.87/minecraft-server-patched-26.1.2.87.jar"
            )
        );
    }

    #[test]
    fn a_missing_installer_file_is_an_error_not_a_literal() {
        assert!(resolve("/data/client.lzma", Path::new("/libs"), &HashMap::new()).is_err());
    }
}
