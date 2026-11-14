//! A modpack archive: the zip a pack is distributed as and the trees it writes
//! straight into the game directory. This module owns the [`Archive`] every
//! pack is installed from. The launcher reads no pack format itself: a pack
//! format a plugin contributes hands over the trees it names and the policy its
//! files are fetched under, so no caller is told which platform served it.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use proto::content::{ModpackFile, SideSupport};
use proto::error::ArchiveFault;
use proto::minecraft::{ConfigEntry, ProvisionPhase};
use proto::provider::pack::OverrideTrees;
use proto::warning::WarningInfo;

use crate::cancel::Job;
use crate::icon;
use crate::plugins::vouch::Policy;
use crate::transfer::archive::{Extracted, Reader};

/// Which side an install is for — which of the two side-specific override trees
/// applies, and which `env` field decides whether a file is wanted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Client,
    Server,
}

impl Side {
    /// The directories whose files this side takes, in write order: the shared
    /// trees first, so a tree of the side's own wins a path both name.
    fn trees(self, trees: &OverrideTrees) -> impl Iterator<Item = &String> {
        let own = match self {
            Side::Client => &trees.client,
            Side::Server => &trees.server,
        };
        trees.shared.iter().chain(own)
    }

    /// Whether the pack wants this file on this side. A file the pack marks
    /// unsupported for the side is not an error — a client-only shader in a
    /// pack installed on a server is simply not that server's file.
    pub fn wants(self, file: &ModpackFile) -> bool {
        let support = match self {
            Side::Client => file.client,
            Side::Server => file.server,
        };
        support != SideSupport::Unsupported
    }
}

/// An opened pack archive, handed to the modpack flow without being reopened.
pub(crate) struct Archive {
    reader: Reader,
    /// The directory the pack starts at.
    root: String,
    trees: OverrideTrees,
    /// What the files the pack names are fetched under: the grant of the plugin
    /// that read it, or of the source it delegated its files to.
    policy: Option<Arc<Policy>>,
    /// The vetted JVM settings the pack asks for, as `config` entries, for the
    /// instance an install creates.
    config: Vec<ConfigEntry>,
    /// The format's name, for what is reported about its pack.
    format: String,
    /// The image the pack named, as an archive member under `root`.
    icon: Option<String>,
    /// What reading the pack dropped, to be reported with the install.
    warnings: Vec<WarningInfo>,
}

/// What a pack format a plugin contributes hands the install beside the pack
/// itself.
pub(crate) struct Contributed {
    pub(crate) trees: OverrideTrees,
    pub(crate) policy: Option<Arc<Policy>>,
    pub(crate) config: Vec<ConfigEntry>,
    pub(crate) format: String,
    pub(crate) icon: Option<String>,
    pub(crate) warnings: Vec<WarningInfo>,
}

impl Archive {
    pub(crate) fn contributed(reader: Reader, root: String, contributed: Contributed) -> Self {
        Archive {
            reader,
            root,
            trees: contributed.trees,
            policy: contributed.policy,
            config: contributed.config,
            format: contributed.format,
            icon: contributed.icon,
            warnings: contributed.warnings,
        }
    }

    pub(crate) fn take_warnings(&mut self) -> Vec<WarningInfo> {
        std::mem::take(&mut self.warnings)
    }

    pub(crate) fn icon_dropped(&self, error: &anyhow::Error) -> WarningInfo {
        WarningInfo::PackIconDropped {
            pack: self.format.clone(),
            detail: format!("{error:#}"),
        }
    }

    /// The bytes of the image the pack named, read under the size an entry's
    /// icon may be; `None` when it named none.
    pub(crate) fn icon_image(&mut self) -> Option<Result<Vec<u8>>> {
        let member = format!("{}{}", self.root, self.icon.as_deref()?);
        Some(self.reader.read_at_most(&member, icon::MAX_BYTES))
    }

    pub(crate) fn config(&self) -> &[ConfigEntry] {
        &self.config
    }

    pub(crate) fn policy(&self) -> Option<Arc<Policy>> {
        self.policy.clone()
    }

    /// `place` maps a path in the override trees to where it goes; `None` leaves what is on disk untouched.
    pub(crate) fn extract_overrides(
        &mut self,
        side: Side,
        job: &Job,
        place: impl FnMut(&str) -> Option<PathBuf>,
    ) -> Result<Vec<Extracted>> {
        let trees: Vec<String> = side
            .trees(&self.trees)
            .map(|dir| format!("{}{dir}/", self.root))
            .collect();
        self.reader
            .extract(&trees, ProvisionPhase::Overrides, true, job, place)
    }
}

/// The directory prefix of the shallowest member named `marker`, so a nested
/// export is found but a marker buried deeper never outranks one at the root.
pub(crate) fn shallowest(names: &[String], marker: &str) -> Option<String> {
    names
        .iter()
        .filter_map(|name| match name.as_str() == marker {
            true => Some(String::new()),
            false => name
                .strip_suffix(marker)
                .filter(|root| root.ends_with('/'))
                .map(str::to_string),
        })
        .min_by_key(|root| root.matches('/').count())
}

pub(crate) fn invalid(reason: ArchiveFault) -> anyhow::Error {
    proto::error::ErrorInfo::ModpackInvalid { reason }.into()
}

/// A relative path that stays inside the game directory: not empty, not
/// absolute, and with no parent (`..`), root, or drive-prefix components.
pub fn is_safe_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return false;
    }
    p.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(entries: &[(&str, &str)], root: &str) -> Archive {
        let dir = temp("zip");
        let path = dir.path().join("pack.zip");
        let mut writer = crate::transfer::archive::Writer::create(&path).unwrap();
        for (name, body) in entries {
            writer.add_bytes(name, body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        let reader = Reader::open(&path).unwrap();
        Archive::contributed(
            reader,
            root.to_string(),
            Contributed {
                trees: OverrideTrees {
                    shared: vec!["overrides".to_string()],
                    client: vec!["client-overrides".to_string()],
                    server: vec!["server-overrides".to_string()],
                },
                policy: None,
                config: Vec::new(),
                format: String::new(),
                icon: None,
                warnings: Vec::new(),
            },
        )
    }

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-pack-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn extract(archive: &mut Archive, side: Side, dest: &Path) -> Vec<Extracted> {
        let cancel = crate::cancel::Cancel::new();
        let noop = |_: &proto::minecraft::ProvisionProgress| {};
        let job = Job::new(&noop, &cancel);
        archive
            .extract_overrides(side, &job, |path| Some(dest.join(path)))
            .unwrap()
    }

    #[test]
    fn a_side_takes_the_shared_tree_and_its_own() {
        let mut pack = archive(
            &[
                ("overrides/config/shared.toml", "shared"),
                ("client-overrides/options.txt", "client"),
                ("server-overrides/server-icon.png", "server"),
            ],
            "",
        );
        let dest_dir = temp("sides");
        let dest = dest_dir.path();

        let written = extract(&mut pack, Side::Client, dest);

        assert!(dest.join("config/shared.toml").is_file(), "shared tree");
        assert!(dest.join("options.txt").is_file(), "the client's own tree");
        assert!(
            !dest.join("server-icon.png").exists(),
            "the other side's tree is not this side's"
        );
        assert_eq!(written.len(), 2);
    }

    #[test]
    fn a_side_tree_wins_where_both_name_the_same_path() {
        let mut pack = archive(
            &[
                ("overrides/config/cozy.toml", "shared value"),
                ("client-overrides/config/cozy.toml", "client value"),
            ],
            "",
        );
        let dest_dir = temp("precedence");
        let dest = dest_dir.path();

        extract(&mut pack, Side::Client, dest);

        assert_eq!(
            std::fs::read_to_string(dest.join("config/cozy.toml")).unwrap(),
            "client value",
            "the side tree is written after the shared one, so it wins"
        );
    }

    #[test]
    fn a_pack_zipped_inside_a_folder_is_read_from_that_folder() {
        let mut pack = archive(&[("Cozy/overrides/config/a.toml", "a")], "Cozy/");
        let dest_dir = temp("nested");
        let written = extract(&mut pack, Side::Client, dest_dir.path());
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].path, "config/a.toml");
    }

    #[test]
    fn refusing_a_file_leaves_what_is_there() {
        let mut pack = archive(
            &[
                ("overrides/config/mine.toml", "the pack's"),
                ("overrides/config/theirs.toml", "the pack's"),
            ],
            "",
        );
        let dest_dir = temp("keep");
        let dest = dest_dir.path();
        std::fs::create_dir_all(dest.join("config")).unwrap();
        std::fs::write(dest.join("config/mine.toml"), "my edit").unwrap();

        let cancel = crate::cancel::Cancel::new();
        let noop = |_: &proto::minecraft::ProvisionProgress| {};
        let job = Job::new(&noop, &cancel);
        let written = pack
            .extract_overrides(Side::Client, &job, |path| {
                (path != "config/mine.toml").then(|| dest.join(path))
            })
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("config/mine.toml")).unwrap(),
            "my edit",
            "a refused file is untouched"
        );
        assert_eq!(written.len(), 1, "and is not reported as written");
        assert_eq!(written[0].path, "config/theirs.toml");
    }

    #[test]
    fn an_override_escaping_the_game_directory_is_refused() {
        let mut pack = archive(&[("overrides/../../escape.txt", "no")], "");
        let dest_dir = temp("escape");
        let dest = dest_dir.path();
        let cancel = crate::cancel::Cancel::new();
        let noop = |_: &proto::minecraft::ProvisionProgress| {};
        let job = Job::new(&noop, &cancel);

        assert!(pack
            .extract_overrides(Side::Client, &job, |path| Some(dest.join(path)))
            .is_err());
    }
}
