//! The provider seam. A flavor implements one trait for the sides it serves,
//! listing the game versions it supports and resolving a request into a full
//! launch profile. The `Minecraft` aggregate holds one boxed registry —
//! adding a flavor is a new impl plus one line in `Minecraft::default`.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::{Edition, GameVersion, Requirement};

use crate::cache::Cache;
use crate::minecraft::materialize::OnProgress;
use crate::minecraft::profile::{InstanceProfile, ServerProfile};
use crate::process::ProcessSupervisor;
use crate::vouch::Policy;

/// A resolution request: a game version and, for modloaders, an optional pinned
/// loader version (the newest stable loader is chosen when absent). `meta` and
/// `cache` let a flavor keep what it must read to resolve (NeoForge's installer).
pub struct ResolveRequest<'a> {
    pub version: String,
    pub loader_version: Option<String>,
    pub meta: &'a Path,
    pub cache: Option<&'a Cache>,
}

/// What a flavor needs to build whatever its profile cannot simply name — the
/// patched jar a NeoForge loader runs, or a Spigot server nobody may
/// redistribute. Given to [`FlavorProvider::install`].
pub struct InstallRequest<'a> {
    pub game_version: &'a str,
    pub loader_version: Option<&'a str>,
    /// Where this entry's install writes: `meta/` for a client, the server's
    /// own data directory otherwise.
    pub root: &'a Path,
    /// The shared `meta/` root, for work no single entry owns.
    pub meta: &'a Path,
    /// What the profile names as built, which the flavor's build must produce:
    /// the server jar's filename, or the path of each built client library.
    pub built: &'a [String],
    /// What a Java-based install builds with; `None` for a native server.
    pub java: Option<JavaInstall<'a>>,
    pub cache: Option<&'a Cache>,
    pub processes: &'a ProcessSupervisor,
    /// Where a native server install stores the `server.properties` it ships,
    /// the schema its settings are checked against; unused otherwise.
    pub schema: Option<&'a Path>,
}

pub struct JavaInstall<'a> {
    /// The vanilla jar for this side, already materialized; none for a server
    /// whose own jar is built.
    pub minecraft_jar: Option<&'a Path>,
    /// The Java major of `java`, from the profile.
    pub java_major: i32,
    pub java: &'a Path,
}

impl<'a> InstallRequest<'a> {
    /// For a flavor whose install runs Java tooling.
    pub fn java_install(&self) -> Result<&JavaInstall<'a>> {
        self.java
            .as_ref()
            .context("this install needs a Java runtime")
    }
}

/// The third-party content a flavor's own loader consumes: mods for a
/// modloader, plugins for a server platform, nothing for vanilla. Datapacks
/// (and, client-side, resourcepacks and shaders) belong to the game rather than
/// the flavor, so they are not named here — `edition::accepted_kinds` adds them.
pub type Loads = Option<ContentKind>;

/// Which half of the game an entry is, for the rules that differ between them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Server,
    Client,
}

impl Side {
    pub fn noun(self) -> &'static str {
        match self {
            Side::Server => "server",
            Side::Client => "instance",
        }
    }
}

/// A tool a flavor needs on the machine that Hestia cannot install itself: a
/// command on `PATH`, and what to tell the user when it is not there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prerequisite {
    pub command: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub url: Cow<'static, str>,
}

impl Prerequisite {
    /// Whether the command resolves on `PATH`.
    pub fn installed(&self) -> bool {
        resolve_on_path(&self.command).is_some()
    }
}

/// The first executable named `name` in a `PATH` directory. A relative
/// directory is skipped: it would resolve against wherever the daemon runs.
fn resolve_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// Which of `required` are not on this machine, in the shape the wire carries.
pub fn unmet(required: &[Prerequisite]) -> Vec<Requirement> {
    unmet_by(required, Prerequisite::installed)
}

/// [`unmet`] with `installed` deciding whether a prerequisite is there, so a
/// test need not read the real `PATH`.
pub fn unmet_by(
    required: &[Prerequisite],
    installed: impl Fn(&Prerequisite) -> bool,
) -> Vec<Requirement> {
    required
        .iter()
        .filter(|prerequisite| !installed(prerequisite))
        .map(|prerequisite| Requirement {
            name: prerequisite.name.to_string(),
            url: prerequisite.url.to_string(),
        })
        .collect()
}

pub struct FlavorInfo {
    pub id: Cow<'static, str>,
    pub name: Cow<'static, str>,
    /// One line describing the distribution — what it is, and anything about
    /// running it a user should know before choosing it. Rendered by every
    /// front-end, so a new flavor needs no front-end change to explain itself.
    pub summary: Cow<'static, str>,
    pub edition: Edition,
    pub loads: Loads,
    /// Tools this flavor needs on the machine; none for all but a flavor that
    /// builds its own artifacts.
    pub requires: Cow<'static, [Prerequisite]>,
    pub sides: &'static [Side],
    /// The name a content platform gives this flavor's loader; none for the
    /// flavor's own id.
    pub loader: Option<Cow<'static, str>>,
    /// Further loader names whose content this flavor also runs.
    pub compatible: Cow<'static, [Cow<'static, str>]>,
    /// How each interchange format names this flavor's loader, by format id.
    pub interchange: Interchange,
}

pub type Interchange = Cow<'static, [(Cow<'static, str>, Cow<'static, str>)]>;

impl FlavorInfo {
    /// The loader name content queries filter this flavor by.
    pub fn loader_name(&self) -> &str {
        self.loader.as_deref().unwrap_or(&self.id)
    }

    /// Every loader name a content version may carry and still run here, the
    /// flavor's own first.
    pub fn loader_names(&self) -> Vec<String> {
        std::iter::once(self.loader_name())
            .chain(self.compatible.iter().map(|name| name.as_ref()))
            .map(str::to_string)
            .collect()
    }
}

pub const BOTH_SIDES: &[Side] = &[Side::Server, Side::Client];
pub const SERVER_ONLY: &[Side] = &[Side::Server];
pub const CLIENT_ONLY: &[Side] = &[Side::Client];

#[async_trait]
pub trait FlavorProvider: Send + Sync {
    fn info(&self) -> &FlavorInfo;
    async fn versions(&self) -> Result<Vec<GameVersion>>;
    /// The loader builds available for a game version, newest first. A flavor
    /// with no loader concept (vanilla) reports none — the default.
    async fn loader_versions(&self, _game: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    async fn resolve_server(&self, _request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        bail!("unknown server flavor: {}", self.info().id)
    }
    async fn resolve_client(&self, _request: &ResolveRequest<'_>) -> Result<InstanceProfile> {
        bail!("unknown instance flavor: {}", self.info().id)
    }

    /// Build whatever the profile cannot name — the patched jar a NeoForge
    /// client runs, which exists nowhere to download. Idempotent: a client
    /// install runs on every launch and must cost nothing once done.
    /// The policy the artifact at `url` in this flavor's profile is downloaded
    /// under; `None` for one the launcher itself names. A profile persists, so
    /// this is asked again whenever its files are fetched.
    fn download_policy(&self, _url: &str) -> Option<Arc<Policy>> {
        None
    }

    async fn install(
        &self,
        _side: Side,
        _request: &InstallRequest<'_>,
        _on: OnProgress<'_>,
    ) -> Result<()> {
        Ok(())
    }
}
