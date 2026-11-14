//! Pack formats contributed by plugins. A plugin's `pack` provider declares the
//! archive members it reads and answers what the archive holds; the host opens
//! the archive, reads only those members as text, and vets everything the
//! guest answers before a byte is fetched or copied: file paths and URLs
//! through [`Policy::vet_artifact`], override trees against the pack's own
//! directory, and an export's mapping against the files the export holds. An
//! answer with one refused entry is refused whole.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};
use std::sync::Arc;

use anyhow::Result;
use proto::content::ModpackFile;
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use proto::plugin::Manifest;
use proto::provider::pack::{
    self as op, is_member_pattern, member_matches, ClaimQuery, Pack, PackInfo, PackRead,
    PackWritten, ReadQuery, WriteQuery,
};
use proto::warning::WarningInfo;
use proto::Empty;

use super::{connect_each, registered, Guest};
use crate::content::pack::is_safe_path;
use crate::engine::Engine;
use crate::jvm::{JavaSettings, JVM_ARGS_KEY, MEMORY_KEY};
use crate::minecraft::Loaders;
use crate::plugins::runtime::LoadedPlugin;
use crate::plugins::vouch::{Policy, Refused};
use crate::transfer::archive::Reader;
use crate::transfer::hestia;

const MAX_MEMBER_BYTES: usize = 8 * 1024 * 1024;
const MAX_DECLARED_MEMBERS: usize = 8;
const MAX_READ_MEMBERS: usize = 32;
const MAX_CLAIM_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 50_000;
const MAX_TREES: usize = 8;
const MAX_WRITTEN_MEMBERS: usize = 64;
const MAX_WARNINGS: usize = 32;
const MAX_WARNING_CHARS: usize = 512;
const MAX_EXTENSION_CHARS: usize = 16;

/// The id the launcher's own format answers to.
const RESERVED: &[&str] = &["hestia"];

/// Why a guest's answer was refused.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error("`{0}` is not a path inside the instance")]
    Unsafe(String),
    #[error("`{0}` is not a directory inside the pack")]
    Tree(String),
    #[error("`{0}` is not a file the export holds")]
    Unlisted(String),
    #[error("`{0}` is named twice")]
    Duplicate(String),
    #[error("the pack file `{path}`: {refused}")]
    File { path: String, refused: Refused },
    #[error("it delegates to `{0}` and also lists files")]
    Both(String),
    #[error("the pack names no game version")]
    NoGameVersion,
    #[error("it answers too many {0}")]
    TooMany(&'static str),
    #[error("`{0}` is over {MAX_MEMBER_BYTES} bytes")]
    Oversized(String),
}

/// A guest's answer after vetting, with what the host dropped from it.
pub(crate) struct Vetted {
    pub(crate) read: PackRead,
    pub(crate) warnings: Vec<WarningInfo>,
}

/// A path of the archive, relative to the pack's root: plain segments only,
/// so it cannot name the root itself, a parent or a drive.
fn is_plain(path: &str) -> bool {
    is_safe_path(path)
        && !path.contains('\\')
        && path.split('/').all(|segment| !segment.is_empty())
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn vet_read(policy: &Policy, pack: &str, mut read: PackRead) -> Result<Vetted, Refusal> {
    if read.pack.files.len() > MAX_FILES {
        return Err(Refusal::TooMany("files"));
    }
    if read.pack.game_version.trim().is_empty() {
        return Err(Refusal::NoGameVersion);
    }
    if let Some(delegate) = &read.resolve {
        if !read.pack.files.is_empty() {
            return Err(Refusal::Both(delegate.source.clone()));
        }
    }
    let trees = &read.overrides;
    for side in [&trees.shared, &trees.client, &trees.server] {
        if side.len() > MAX_TREES {
            return Err(Refusal::TooMany("override trees"));
        }
        if let Some(bad) = side.iter().find(|dir| !is_plain(dir)) {
            return Err(Refusal::Tree(bad.clone()));
        }
    }
    for file in &mut read.pack.files {
        if !is_safe_path(&file.path) {
            return Err(Refusal::Unsafe(file.path.clone()));
        }
        if !file.artifact.url.is_empty() {
            policy
                .vet_artifact(&file.artifact)
                .map_err(|refused| Refusal::File {
                    path: file.path.clone(),
                    refused,
                })?;
        }
        *file = ModpackFile {
            path: std::mem::take(&mut file.path),
            artifact: std::mem::take(&mut file.artifact),
            client: file.client,
            server: file.server,
            ..ModpackFile::default()
        };
    }
    let mut warnings = Vec::new();
    let mut jvm = JavaSettings::default();
    for (key, value) in [
        (MEMORY_KEY, &mut read.memory),
        (JVM_ARGS_KEY, &mut read.jvm_args),
    ] {
        let Some(text) = value.as_deref().filter(|text| !text.trim().is_empty()) else {
            *value = None;
            continue;
        };
        match jvm.set(key, text) {
            Ok(_) => *value = jvm.get(key).flatten(),
            Err(error) => {
                tracing::warn!(pack, setting = key, error = %format!("{error:#}"), "dropping a pack's setting");
                warnings.push(WarningInfo::PackSettingDropped {
                    pack: pack.to_string(),
                    setting: key.to_string(),
                    detail: truncated(&format!("{error:#}")),
                });
                *value = None;
            }
        }
    }
    read.pack.source.clear();
    read.pack.project_id.clear();
    read.pack.version_id.clear();
    Ok(Vetted { read, warnings })
}

fn truncated(text: &str) -> String {
    text.chars().take(MAX_WARNING_CHARS).collect()
}

/// An icon only dresses the instance, so one that is not a plain path of the
/// archive is dropped, with the reason, rather than refusing the read.
fn vet_icon(vetted: &mut Vetted, pack: &str, files: &[String]) {
    let Some(icon) = vetted.read.icon.take() else {
        return;
    };
    let problem = match (is_plain(&icon), files.contains(&icon)) {
        (false, _) => format!("`{}` is not a path inside the pack", truncated(&icon)),
        (true, false) => format!("`{icon}` is not a file of the archive"),
        (true, true) => {
            vetted.read.icon = Some(icon);
            return;
        }
    };
    tracing::warn!(pack, %problem, "dropping a pack's icon");
    vetted.warnings.push(WarningInfo::PackIconDropped {
        pack: pack.to_string(),
        detail: problem,
    });
}

fn vet_write(held: &HashSet<&str>, written: &mut PackWritten) -> Result<(), Refusal> {
    if written.copies.len() > MAX_FILES {
        return Err(Refusal::TooMany("copies"));
    }
    if written.members.len() > MAX_WRITTEN_MEMBERS {
        return Err(Refusal::TooMany("manifests"));
    }
    let mut named = HashSet::new();
    for member in &written.members {
        if !is_safe_path(&member.path) {
            return Err(Refusal::Unsafe(member.path.clone()));
        }
        if member.text.len() > MAX_MEMBER_BYTES {
            return Err(Refusal::Oversized(member.path.clone()));
        }
        if !named.insert(member.path.as_str()) {
            return Err(Refusal::Duplicate(member.path.clone()));
        }
    }
    for copy in &written.copies {
        if !is_safe_path(&copy.archive_path) {
            return Err(Refusal::Unsafe(copy.archive_path.clone()));
        }
        if !held.contains(copy.entry_path.as_str()) {
            return Err(Refusal::Unlisted(copy.entry_path.clone()));
        }
        if !named.insert(copy.archive_path.as_str()) {
            return Err(Refusal::Duplicate(copy.archive_path.clone()));
        }
    }
    written.warnings.truncate(MAX_WARNINGS);
    for warning in &mut written.warnings {
        if warning.chars().count() > MAX_WARNING_CHARS {
            *warning = warning.chars().take(MAX_WARNING_CHARS).collect();
        }
    }
    Ok(())
}

/// The directory prefixes under which `pattern` matches a member.
fn roots_of(names: &[String], pattern: &str) -> HashSet<String> {
    let depth = pattern.split('/').count();
    names
        .iter()
        .filter(|name| !name.ends_with('/'))
        .filter_map(|name| {
            let segments: Vec<&str> = name.split('/').collect();
            let split = segments.len().checked_sub(depth)?;
            member_matches(pattern, &segments[split..].join("/")).then(|| match split {
                0 => String::new(),
                _ => format!("{}/", segments[..split].join("/")),
            })
        })
        .collect()
}

/// The shallowest directory holding a member for every pattern, so a pack
/// nested in a folder is found and one buried deeper never outranks it.
fn root_of(names: &[String], patterns: &[String]) -> Option<String> {
    let (first, rest) = patterns.split_first()?;
    let mut roots = roots_of(names, first);
    for pattern in rest {
        let found = roots_of(names, pattern);
        roots.retain(|root| found.contains(root));
    }
    roots
        .into_iter()
        .min_by_key(|root| (root.matches('/').count(), root.clone()))
}

/// A pack format a plugin contributes, registered as `<plugin>.<id>`, or bare
/// when official.
pub struct PluginPack {
    guest: Guest,
    info: PackInfo,
    policy: Arc<Policy>,
}

impl PluginPack {
    /// Asks the guest to describe its format and refuses one the host could not
    /// tell apart from its own formats.
    async fn connect(guest: Guest) -> Result<Self> {
        if RESERVED.contains(&guest.local.as_str()) || RESERVED.contains(&guest.id()) {
            return Err(
                guest.failed("that id belongs to a format the launcher writes itself".into())
            );
        }
        let info = guest.ask::<op::Info>(&Empty {}).await?;
        if info.members.is_empty() || info.members.len() > MAX_DECLARED_MEMBERS {
            return Err(guest.failed(format!(
                "it must declare between 1 and {MAX_DECLARED_MEMBERS} members"
            )));
        }
        if let Some(bad) = info
            .members
            .iter()
            .find(|pattern| !is_member_pattern(pattern))
        {
            return Err(guest.failed(format!("`{bad}` is not a usable member")));
        }
        if let Some(taken) = info
            .members
            .iter()
            .find(|pattern| member_matches(pattern, hestia::MANIFEST))
        {
            return Err(guest.failed(format!(
                "`{taken}` would claim a format the launcher reads itself"
            )));
        }
        let extension = &info.extension;
        if extension.len() > MAX_EXTENSION_CHARS
            || !extension
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(guest.failed(format!("`{extension}` is not a usable extension")));
        }
        let policy = Arc::new(Policy::new(guest.plugin().grant()));
        Ok(PluginPack {
            guest,
            info,
            policy,
        })
    }

    pub(crate) fn id(&self) -> &str {
        self.guest.id()
    }

    pub(crate) fn info(&self) -> &PackInfo {
        &self.info
    }

    /// The policy the files this format names are fetched under.
    pub(crate) fn policy(&self) -> Arc<Policy> {
        self.policy.clone()
    }

    /// The directory the archive's pack starts at, when it holds every member
    /// the format declared.
    pub(crate) fn root_of(&self, names: &[String]) -> Option<String> {
        root_of(names, &self.info.members)
    }

    /// The archive's member names under `root`, named relative to it.
    pub(crate) fn files_under(names: &[String], root: &str) -> Vec<String> {
        names
            .iter()
            .filter(|name| !name.ends_with('/'))
            .filter_map(|name| name.strip_prefix(root).map(str::to_string))
            .collect()
    }

    /// The archive members a pattern the format declared matches.
    fn declared<'a>(&'a self, files: &'a [String]) -> impl Iterator<Item = &'a String> {
        files.iter().filter(|file| {
            self.info
                .members
                .iter()
                .any(|pattern| member_matches(pattern, file))
        })
    }

    /// The text of the declared members a claim may look inside: one that is
    /// not UTF-8, is over the member bound or would pass the total bound is
    /// left out rather than refusing the archive.
    fn claim_members(
        &self,
        reader: &mut Reader,
        root: &str,
        files: &[String],
    ) -> BTreeMap<String, String> {
        let mut members = BTreeMap::new();
        let mut total = 0;
        for file in self.declared(files).take(MAX_READ_MEMBERS) {
            let budget = MAX_MEMBER_BYTES.min(MAX_CLAIM_BYTES - total);
            let text = reader
                .read_capped(&format!("{root}{file}"), budget)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok());
            let Some(text) = text else {
                tracing::debug!(pack = %self.id(), member = %file, "a member is not offered to the claim");
                continue;
            };
            total += text.len();
            members.insert(file.clone(), text);
        }
        members
    }

    /// Whether the guest accepts an archive that holds the declared members,
    /// given the text of those members. A guest that does not implement
    /// `claims` accepts it; one that fails is passed over, so a broken
    /// detector cannot block another format.
    pub(crate) async fn claims(&self, reader: &mut Reader, root: &str) -> bool {
        let files = Self::files_under(&reader.names(), root);
        let members = self.claim_members(reader, root, &files);
        match self
            .guest
            .try_ask::<op::Claims>(&ClaimQuery { files, members })
            .await
        {
            Ok(answer) => answer.is_none_or(|claim| claim.claimed),
            Err(error) => {
                tracing::warn!(pack = %self.id(), error = %format!("{error:#}"), "a pack format's claim failed");
                false
            }
        }
    }

    fn refused(&self, refusal: Refusal) -> anyhow::Error {
        tracing::warn!(pack = %self.id(), %refusal, "pack answer refused");
        self.guest.failed(format!("answer refused: {refusal}"))
    }

    /// Hands the guest the member names, the text of the declared members and
    /// the loader keys its format has flavors for, and returns what it says
    /// the archive holds. The loader comes back as the flavor its key maps to.
    pub(crate) async fn read(
        &self,
        reader: &mut Reader,
        root: &str,
        loaders: &Loaders,
    ) -> Result<Vetted> {
        let files = Self::files_under(&reader.names(), root);
        let mut members = BTreeMap::new();
        for file in self.declared(&files) {
            if members.len() >= MAX_READ_MEMBERS {
                return Err(self.refused(Refusal::TooMany("matching members")));
            }
            let malformed = || ErrorInfo::ArchiveInvalid {
                format: ArchiveFormat::Zip,
                reason: ArchiveFault::ManifestMalformed,
            };
            let bytes = reader
                .read_capped(&format!("{root}{file}"), MAX_MEMBER_BYTES)
                .map_err(|error| {
                    tracing::warn!(member = %file, error = %format!("{error:#}"), "cannot read a pack member");
                    malformed()
                })?;
            let text = String::from_utf8(bytes).map_err(|_| malformed())?;
            members.insert(file.clone(), text);
        }
        let query = ReadQuery {
            loaders: loaders.keys_of(self.id()),
            files,
            members,
        };
        let read = self.guest.ask::<op::Read>(&query).await?;
        let mut vetted = vet_read(&self.policy, &self.info.name, read)
            .map_err(|refusal| self.refused(refusal))?;
        vet_icon(&mut vetted, &self.info.name, &query.files);
        let pack = &mut vetted.read.pack;
        if let Some(flavor) = pack
            .loader
            .as_deref()
            .and_then(|key| loaders.flavor_of(self.id(), key))
        {
            pack.loader = Some(flavor.to_string());
        }
        Ok(vetted)
    }

    /// Asks the guest to write an export. `held` is every path of the
    /// instance the export can carry, which the answer's copies may name.
    pub(crate) async fn write(&self, query: &WriteQuery) -> Result<PackWritten> {
        let mut written = self
            .guest
            .try_ask::<op::Write>(query)
            .await?
            .ok_or_else(|| self.guest.failed("it does not write archives".into()))?;
        let held: HashSet<&str> = query
            .files
            .iter()
            .map(String::as_str)
            .chain(query.content.iter().map(|item| item.path.as_str()))
            .collect();
        vet_write(&held, &mut written).map_err(|refusal| self.refused(refusal))?;
        Ok(written)
    }
}

pub(super) async fn connect(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
    official: bool,
) -> Vec<PluginPack> {
    connect_each::<Pack, _, _, _>(plugin, manifest, official, PluginPack::connect).await
}

pub(super) fn register(engine: &Engine, owner: &str, official: bool, packs: Vec<PluginPack>) {
    for pack in packs {
        let id = pack.id().to_string();
        let live = pack.guest.live();
        let outcome = engine
            .packs()
            .register(owner, official, live, &id, Arc::new(pack));
        registered(owner, "pack", &id, outcome);
    }
}

#[cfg(test)]
mod tests {
    use proto::content::{ResolvedModpack, SideSupport};
    use proto::minecraft::Artifact;
    use proto::plugin::{Grant, Permission, PermissionScope, ScopeKind};
    use proto::provider::pack::{ManifestDelegate, OverrideTrees, PackCopy, PackMember};

    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    fn patterns(list: &[&str]) -> Vec<String> {
        names(list)
    }

    fn policy(hosts: &[&str]) -> Policy {
        let mut grant = Grant::default();
        grant.permissions.insert(
            Permission::NetworkConnect,
            PermissionScope::of(ScopeKind::Hosts, hosts.iter().copied()),
        );
        Policy::new(&grant)
    }

    fn file(path: &str, url: &str, sha1: &str) -> ModpackFile {
        ModpackFile {
            path: path.into(),
            artifact: Artifact {
                url: url.into(),
                filename: "a.jar".into(),
                checksum: (!sha1.is_empty()).then(|| proto::download::Checksum {
                    algorithm: proto::download::HashAlgorithm::Sha1,
                    hex: sha1.into(),
                }),
                ..Artifact::default()
            },
            client: SideSupport::Required,
            server: SideSupport::Unsupported,
            source: "spoofed".into(),
            project_id: "spoofed".into(),
            ..ModpackFile::default()
        }
    }

    fn read_of(files: Vec<ModpackFile>) -> PackRead {
        PackRead {
            pack: ResolvedModpack {
                source: "spoofed".into(),
                game_version: "1.21".into(),
                files,
                ..ResolvedModpack::default()
            },
            ..PackRead::default()
        }
    }

    const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn the_shallowest_root_holding_every_member_is_the_packs() {
        let archive = names(&[
            "nested/deeper/instance.cfg",
            "nested/deeper/mmc-pack.json",
            "Pack/instance.cfg",
            "Pack/mmc-pack.json",
            "Pack/.minecraft/options.txt",
        ]);
        assert_eq!(
            root_of(&archive, &patterns(&["instance.cfg", "mmc-pack.json"])).as_deref(),
            Some("Pack/")
        );
        assert_eq!(
            root_of(&archive, &patterns(&["instance.cfg", "missing.json"])),
            None
        );
        assert_eq!(
            root_of(&names(&["a.cfg", "b/c.cfg"]), &patterns(&["*.cfg"])).as_deref(),
            Some("")
        );
    }

    #[test]
    fn a_member_pattern_matches_at_the_packs_root_only() {
        assert_eq!(
            root_of(
                &names(&["Pack/sub/manifest.json"]),
                &patterns(&["sub/manifest.json"])
            )
            .as_deref(),
            Some("Pack/")
        );
        assert_eq!(root_of(&names(&["dir/"]), &patterns(&["dir"])), None);
    }

    #[test]
    fn a_read_keeps_the_hosts_identity_and_drops_the_guests() {
        let vetted = vet_read(
            &policy(&["cdn.example"]),
            "pack",
            read_of(vec![file("mods/a.jar", "https://cdn.example/a.jar", SHA1)]),
        )
        .expect("a vetted answer");
        let kept = &vetted.read.pack.files[0];
        assert!(kept.source.is_empty() && kept.project_id.is_empty());
        assert_eq!(kept.server, SideSupport::Unsupported);
        assert!(vetted.read.pack.source.is_empty());
    }

    #[test]
    fn an_escaping_path_refuses_the_whole_answer() {
        for path in ["../evil.jar", "/etc/passwd", "mods/../../x", ""] {
            let refused = vet_read(
                &policy(&["cdn.example"]),
                "pack",
                read_of(vec![
                    file("mods/ok.jar", "https://cdn.example/ok.jar", SHA1),
                    file(path, "https://cdn.example/a.jar", SHA1),
                ]),
            );
            assert!(matches!(refused, Err(Refusal::Unsafe(_))), "{path}");
        }
    }

    #[test]
    fn an_unvetted_file_refuses_the_whole_answer() {
        let vet = |url: &str, sha1: &str| {
            vet_read(
                &policy(&["cdn.example"]),
                "pack",
                read_of(vec![file("mods/a.jar", url, sha1)]),
            )
        };
        assert!(vet("https://cdn.example/a.jar", SHA1).is_ok());
        assert!(
            matches!(
                vet("https://other.example/a.jar", SHA1),
                Err(Refusal::File { .. })
            ),
            "a host the grant does not name"
        );
        assert!(
            matches!(
                vet("https://cdn.example/a.jar", ""),
                Err(Refusal::File { .. })
            ),
            "no checksum"
        );
        assert!(
            matches!(
                vet("https://cdn.example/a.jar", "xyz"),
                Err(Refusal::File { .. })
            ),
            "a malformed checksum"
        );
        assert!(
            matches!(
                vet("http://cdn.example/a.jar", SHA1),
                Err(Refusal::File { .. })
            ),
            "plain http"
        );
        assert!(
            vet("", "").is_ok(),
            "a file its author withholds names no URL"
        );
    }

    #[test]
    fn a_tree_cannot_leave_the_pack() {
        let with = |dir: &str| {
            let mut read = read_of(Vec::new());
            read.overrides = OverrideTrees {
                shared: vec![dir.into()],
                ..OverrideTrees::default()
            };
            vet_read(&policy(&[]), "pack", read)
        };
        for good in ["overrides", ".minecraft", "a/b"] {
            assert!(with(good).is_ok(), "{good}");
        }
        for bad in [
            "", ".", "..", "../up", "/abs", "a/", "a\\b", "a/../b", "a//b",
        ] {
            assert!(matches!(with(bad), Err(Refusal::Tree(_))), "{bad}");
        }
    }

    #[test]
    fn a_pack_resolves_its_files_itself_or_through_a_source() {
        let mut read = read_of(vec![file("mods/a.jar", "", "")]);
        read.resolve = Some(ManifestDelegate {
            source: "curseforge".into(),
            ..ManifestDelegate::default()
        });
        assert!(matches!(
            vet_read(&policy(&[]), "pack", read),
            Err(Refusal::Both(_))
        ));
    }

    #[test]
    fn a_write_names_only_files_the_export_holds() {
        let held: HashSet<&str> = ["options.txt", "mods/a.jar"].into();
        let written = |entry: &str, archive: &str| PackWritten {
            members: vec![PackMember {
                path: "manifest.json".into(),
                text: "{}".into(),
            }],
            copies: vec![PackCopy {
                entry_path: entry.into(),
                archive_path: archive.into(),
            }],
            warnings: Vec::new(),
        };
        assert!(vet_write(&held, &mut written("options.txt", "overrides/options.txt")).is_ok());
        assert!(matches!(
            vet_write(&held, &mut written("secret.txt", "overrides/secret.txt")),
            Err(Refusal::Unlisted(_))
        ));
        assert!(matches!(
            vet_write(&held, &mut written("options.txt", "../out.txt")),
            Err(Refusal::Unsafe(_))
        ));
        assert!(matches!(
            vet_write(&held, &mut written("options.txt", "manifest.json")),
            Err(Refusal::Duplicate(_))
        ));
    }

    #[test]
    fn warnings_are_kept_but_bounded() {
        let mut written = PackWritten {
            warnings: vec!["x".repeat(MAX_WARNING_CHARS * 2); MAX_WARNINGS + 5],
            ..PackWritten::default()
        };
        vet_write(&HashSet::new(), &mut written).unwrap();
        assert_eq!(written.warnings.len(), MAX_WARNINGS);
        assert!(written
            .warnings
            .iter()
            .all(|warning| warning.chars().count() == MAX_WARNING_CHARS));
    }
}
