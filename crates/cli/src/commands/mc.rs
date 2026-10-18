//! Shared helpers for the `server` and `instance` command trees: resolving a
//! flavor (interactively when not given) and rendering flavor/version lists.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::backup::BackupInfo;
use client::proto::error::{EntryKind, EntryTarget};
use client::proto::minecraft::{
    ConfigEntry, Flavor, GameVersion, ProvisionPhase, ProvisionProgress, VersionKind,
};
use client::proto::process::{ProcessInfo, ProcessState};
use client::Client;

use crate::commands::content::format::kind_plural;
use crate::ui::{self, Spinner, View};

/// The shared `config` grammar for a server/instance: `get`/`set`/`list`,
/// mirroring `hestia config`.
#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Print the value of a setting
    Get {
        /// Setting key (e.g. memory, jvm-args, or a server.properties key)
        key: String,
    },
    /// Set a setting (an empty value clears it)
    Set {
        /// Setting key
        key: String,
        /// New value; a JVM-flag string may start with '-'
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
    /// List every setting
    #[command(visible_alias = "ls")]
    List,
}

/// `<server|instance> <name> config get|set|list`.
pub async fn run_config(client: &Client, target: EntryTarget, cmd: ConfigCmd) -> Result<()> {
    let entry = client.entry();
    match cmd {
        ConfigCmd::Get { key } => match entry.config_get(&target, &key).await? {
            Some(value) => ui::show(View::line(value))?,
            None => bail!("'{key}' is not set"),
        },
        ConfigCmd::Set { key, value } => {
            entry.config_set(&target, &key, &value).await?;
            let next = match target.kind {
                EntryKind::Server => "start",
                EntryKind::Instance => "launch",
            };
            ui::show(View::note(format!("applies from the next {next}")))?;
        }
        ConfigCmd::List => {
            let entries = entry.config_list(&target).await?;
            let defaults = jvm_defaults(client).await;
            show_config_entries(format!("{} config", target.entry), entries, &defaults)?;
        }
    }
    Ok(())
}

/// The launcher-wide JVM defaults (`defaults.memory` / `defaults.jvm-args`), an
/// empty string when unset. An entry that leaves `memory`/`jvm-args` unset
/// inherits these at launch, so `config list` surfaces them.
#[derive(Default)]
struct JvmDefaults {
    memory: String,
    jvm_args: String,
}

/// Fetch the launcher-wide JVM defaults; a failed read yields empty values.
async fn jvm_defaults(client: &Client) -> JvmDefaults {
    async fn read(client: &Client, key: &str) -> String {
        client
            .config()
            .get(key)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_default()
    }
    JvmDefaults {
        memory: read(client, "defaults.memory").await,
        jvm_args: read(client, "defaults.jvm-args").await,
    }
}

/// Render `config list` entries as a KEY/VALUE table. A `memory`/`jvm-args`
/// entry left unset shows the inherited launcher default (annotated), so the
/// effective launch value is visible.
fn show_config_entries(
    title: impl Into<String>,
    entries: Vec<ConfigEntry>,
    defaults: &JvmDefaults,
) -> Result<()> {
    let rows = entries
        .into_iter()
        .map(|e| {
            let value = if !e.value.is_empty() {
                e.value
            } else {
                match e.key.as_str() {
                    "memory" if !defaults.memory.is_empty() => {
                        format!("{} (default)", defaults.memory)
                    }
                    "jvm-args" if !defaults.jvm_args.is_empty() => {
                        format!("{} (default)", defaults.jvm_args)
                    }
                    _ => e.value,
                }
            };
            vec![e.key, value]
        })
        .collect::<Vec<_>>();
    ui::show(View::table(title, ["KEY", "VALUE"], rows))
}

/// One-word state for a managed server/instance from its supervised process
/// snapshot (absent means it has never been started).
pub fn process_state_label(process: &Option<ProcessInfo>) -> String {
    match process {
        Some(p) if p.state == ProcessState::Running => format!("running (pid {})", p.pid),
        Some(_) | None => "stopped".to_string(),
    }
}

/// State for an instance, which may have several concurrent sessions: the count
/// of running ones, or `stopped`.
pub fn sessions_label(sessions: &[ProcessInfo]) -> String {
    let running = sessions
        .iter()
        .filter(|p| p.state == ProcessState::Running)
        .count();
    match running {
        0 => "stopped".to_string(),
        1 => "running".to_string(),
        n => format!("running ({n} sessions)"),
    }
}

/// Render backups (newest first) as an ID/KIND/SIZE/AGE table.
pub fn show_backups(title: impl Into<String>, backups: Vec<BackupInfo>) -> Result<()> {
    let rows = backups
        .iter()
        .map(|b| {
            vec![
                b.id.clone(),
                b.kind.as_str().to_string(),
                ui::human_bytes(b.size),
                age_label(b.created_unix),
            ]
        })
        .collect();
    ui::show(View::table(title, ["ID", "KIND", "SIZE", "AGE"], rows))
}

/// Return the chosen backup: validated when `provided`, otherwise picked from
/// an interactive selector (newest first).
pub fn pick_backup(backups: Vec<BackupInfo>, provided: Option<String>) -> Result<BackupInfo> {
    if backups.is_empty() {
        bail!("no backups yet (see `backup create`)");
    }
    if let Some(reference) = provided {
        return backups
            .into_iter()
            .find(|b| b.id == reference)
            .with_context(|| format!("no backup matches '{reference}'"));
    }
    let labels: Vec<String> = backups
        .iter()
        .map(|b| {
            format!(
                "{} ({}, {}, {})",
                b.id,
                b.kind.as_str(),
                ui::human_bytes(b.size),
                age_label(b.created_unix)
            )
        })
        .collect();
    let index = ui::select("select a backup", &labels)?;
    Ok(backups.into_iter().nth(index).expect("selector index"))
}

/// Interactive fallback for a missing `--force`; errors when stdin is not a
/// terminal so scripts must pass the flag explicitly.
pub fn confirm_restore(name: &str, data: &str, backup: &BackupInfo) -> Result<()> {
    let restore = ui::confirm(
        &format!(
            "restoring '{}' replaces the current {data} of '{name}'",
            backup.id
        ),
        "restore",
        "cancel",
    )
    .context("pass --force to restore without confirming")?;
    if !restore {
        bail!("restore cancelled");
    }
    Ok(())
}

/// The reporter state a backup/restore job opens with, so the gauge reads
/// "backing up…" from the first frame instead of the default "resolving…".
pub fn backup_phase() -> ProvisionProgress {
    ProvisionProgress {
        phase: ProvisionPhase::Backup,
        ..Default::default()
    }
}

/// The restore counterpart: the archive unpacks into staging, nothing is
/// backed up.
pub fn restore_phase() -> ProvisionProgress {
    ProvisionProgress {
        phase: ProvisionPhase::Extract,
        ..Default::default()
    }
}

/// Coarse "how long ago" label for a unix timestamp.
pub fn age_label(created_unix: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let secs = now.saturating_sub(created_unix).max(0);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// "how long ago" for an optional last-played stamp; `-` when never played.
pub fn last_played_label(last_played_unix: Option<i64>) -> String {
    last_played_unix.map_or_else(|| "-".to_string(), age_label)
}

/// Compact cumulative-duration label, e.g. `3h 20m`, `12m`, `-` when zero.
pub fn playtime_label(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds == 0 {
        return "-".to_string();
    }
    let (h, m) = (seconds / 3600, (seconds % 3600) / 60);
    match (h, m) {
        (0, 0) => "<1m".to_string(),
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

pub async fn fetch_flavors(client: &Client, kind: EntryKind) -> Result<Vec<Flavor>> {
    let _spinner = Spinner::start("fetching flavors");
    Ok(client.entry().flavors(kind).await?)
}

pub async fn fetch_versions(
    client: &Client,
    kind: EntryKind,
    flavor: &str,
) -> Result<Vec<GameVersion>> {
    let _spinner = Spinner::start("fetching versions");
    Ok(client.entry().versions(kind, flavor).await?)
}

pub fn ensure_flavor(flavors: &[Flavor], id: &str) -> Result<()> {
    if flavors.iter().any(|f| f.id == id) {
        return Ok(());
    }
    let ids: Vec<&str> = flavors.iter().map(|f| f.id.as_str()).collect();
    bail!("unknown flavor '{id}' (available: {})", ids.join(", "));
}

/// The first item whose id or name the user's reference names.
pub fn find_by_reference<T>(
    items: impl IntoIterator<Item = T>,
    reference: &str,
    id_and_name: impl Fn(&T) -> (&str, &str),
) -> Option<T> {
    items.into_iter().find(|item| {
        let (id, name) = id_and_name(item);
        client::proto::naming::reference_matches(reference, id, name)
    })
}

/// `name (flavor game_version)`, the selector line for a server or instance.
pub fn entry_label(name: &str, flavor: &str, game_version: &str) -> String {
    format!("{name} ({flavor} {game_version})")
}

/// Return the chosen flavor id: validated when `provided`, otherwise picked from
/// an interactive selector.
pub fn pick_flavor(flavors: Vec<Flavor>, provided: Option<String>) -> Result<String> {
    if flavors.is_empty() {
        bail!("no flavors are available");
    }
    if let Some(id) = provided {
        ensure_flavor(&flavors, &id)?;
        return Ok(id);
    }
    let labels: Vec<String> = flavors.iter().map(flavor_label).collect();
    let details: Vec<String> = flavors.iter().map(flavor_detail).collect();
    let index = ui::select_detailed("select a flavor", &labels, &details)?;
    Ok(flavors[index].id.clone())
}

/// Return the chosen version id: validated against the flavor's catalogue when
/// `provided`, otherwise picked from a searchable picker — releases lead, Tab
/// pulls snapshots and old versions into the pool.
pub fn pick_version(versions: Vec<GameVersion>, provided: Option<String>) -> Result<String> {
    if versions.is_empty() {
        bail!("no versions are available");
    }
    if let Some(id) = provided {
        if versions.iter().any(|v| v.id == id) {
            return Ok(id);
        }
        bail!("unknown version '{id}' (see `hestia server|instance versions`)");
    }
    let items: Vec<ui::PickerItem> = versions
        .iter()
        .map(|v| ui::PickerItem {
            label: v.id.clone(),
            tag: kind_label(v.kind).to_string(),
            stable: v.kind == VersionKind::Release,
        })
        .collect();
    let index = ui::pick("version", items)?;
    Ok(versions[index].id.clone())
}

/// Interactive fallback for a missing `--downgrade`; errors when stdin is not
/// a terminal so scripts must pass the flag explicitly.
pub fn confirm_downgrade(kind: EntryKind, name: &str, from: &str, to: &str) -> Result<()> {
    let (data, safety) = match kind {
        EntryKind::Server => (
            "a world",
            "the current data is backed up automatically first",
        ),
        EntryKind::Instance => (
            "saves",
            "nothing is backed up first (instances have no backups)",
        ),
    };
    let downgrade = ui::confirm(
        &format!(
            "{to} is older than {from}, and Minecraft cannot load {data} \
             written by a newer version; {safety}"
        ),
        &format!("downgrade '{name}'"),
        "cancel",
    )
    .context("pass --downgrade to allow a downgrade")?;
    if !downgrade {
        bail!("downgrade cancelled");
    }
    Ok(())
}

/// The non-interactive form of the selector: the available flavors as a table.
/// Every column comes off the wire, so a flavor the daemon ships needs nothing
/// here to describe itself.
fn show_flavors(flavors: &[Flavor]) -> Result<()> {
    if flavors.is_empty() {
        return ui::show(View::note("no flavors are available"));
    }
    let rows: Vec<Vec<String>> = flavors
        .iter()
        .map(|f| {
            vec![
                f.id.clone(),
                f.name.clone(),
                accepts_label(f),
                flavor_detail(f),
            ]
        })
        .collect();
    ui::show(View::table(
        "flavors",
        ["ID", "NAME", "TAKES", "SUMMARY"],
        rows,
    ))
}

/// One selector line: the name and what it takes; the daemon's summary is the
/// row's second line.
pub fn flavor_label(flavor: &Flavor) -> String {
    format!("{} ({})", flavor.name, accepts_label(flavor))
}

/// The summary, prefixed with anything this machine is missing — a flavor that
/// cannot be used yet has to say so before it is picked, not after.
pub fn flavor_detail(flavor: &Flavor) -> String {
    let missing: Vec<String> = flavor
        .requires
        .iter()
        .map(|r| format!("needs {} ({})", r.name, r.url))
        .collect();
    match missing.is_empty() {
        true => flavor.summary.clone(),
        false => format!("{} — {}", missing.join(", "), flavor.summary),
    }
}

fn accepts_label(flavor: &Flavor) -> String {
    flavor
        .accepts
        .iter()
        .map(|kind| kind_plural(*kind))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `<server|instance> versions`: pick a flavor, then list its game versions.
pub async fn list_versions(
    client: &Client,
    kind: EntryKind,
    flavor: Option<String>,
    all: bool,
) -> Result<()> {
    let flavors = fetch_flavors(client, kind).await?;
    let flavor = pick_flavor(flavors, flavor)?;
    let versions = fetch_versions(client, kind, &flavor).await?;
    show_versions(&flavor, versions, all)
}

pub async fn list_flavors(client: &Client, kind: EntryKind) -> Result<()> {
    show_flavors(&fetch_flavors(client, kind).await?)
}

/// Show a version table, releases only unless `all` includes snapshots and old
/// versions. Long lists page on a terminal and dump plainly when piped.
fn show_versions(flavor: &str, versions: Vec<GameVersion>, all: bool) -> Result<()> {
    let rows: Vec<Vec<String>> = versions
        .iter()
        .filter(|v| all || v.kind == VersionKind::Release)
        .map(|v| vec![v.id.clone(), kind_label(v.kind).to_string()])
        .collect();
    if rows.is_empty() {
        return ui::show(View::note("no versions available"));
    }
    ui::show(View::table(
        format!("{flavor} versions"),
        ["VERSION", "TYPE"],
        rows,
    ))
}

pub fn kind_label(kind: VersionKind) -> &'static str {
    match kind {
        VersionKind::Release => "release",
        VersionKind::Snapshot => "snapshot",
        VersionKind::OldBeta => "beta",
        VersionKind::OldAlpha => "alpha",
    }
}
