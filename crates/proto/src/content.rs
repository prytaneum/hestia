//! Third-party content vocabulary shared by both sides of the socket. A *source*
//! is a platform (modrinth, curseforge, …); a source lists *projects* (mods,
//! modpacks, resourcepacks, shaders), each with its downloadable *versions*, and
//! resolves a modpack version into the file manifest a launcher installs. The
//! types are normalized so a front-end never sees a platform's raw shape, and all
//! carry `#[serde(default)]` so an older/newer peer decodes additive fields.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::change::Scope;
use crate::contract::{Contract, Empty};
use crate::error::{EntryTarget, ErrorInfo};
use crate::job::{Job, JobStarted};
use crate::minecraft::{Artifact, ProvisionProgress};

/// What a project is — the second selector level after the source.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    #[default]
    Mod,
    Modpack,
    ResourcePack,
    Shader,
    DataPack,
    Plugin,
}

impl fmt::Display for ContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ContentKind::Mod => "mod",
            ContentKind::Modpack => "modpack",
            ContentKind::ResourcePack => "resource pack",
            ContentKind::Shader => "shader",
            ContentKind::DataPack => "datapack",
            ContentKind::Plugin => "plugin",
        })
    }
}

/// Whether a project (or a modpack file) is meant for the client, the server,
/// both, or neither. `Unknown` when the source does not say.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SideSupport {
    Required,
    Optional,
    Unsupported,
    #[default]
    Unknown,
}

/// A version's release stability.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ReleaseChannel {
    #[default]
    Release,
    Beta,
    Alpha,
}

/// A source platform (modrinth, curseforge) — the first selector level. Only
/// sources that can actually serve requests are listed (a platform whose API
/// key is unset is not), and `kinds` is what this one catalogues, so a
/// front-end offers it for those kinds alone.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentSource {
    pub id: String,
    pub name: String,
    pub kinds: Vec<ContentKind>,
}

/// A gallery image on a project. Search hits carry only `url`; the detail call
/// fills the caption fields.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct GalleryImage {
    pub url: String,
    pub featured: bool,
    pub title: String,
    pub description: String,
}

/// A project, as a search hit or a detail. `body` (the long description) is only
/// filled by the detail call; `icon_url`/`gallery` carry images for the desktop UI.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentProject {
    pub source: String,
    pub id: String,
    pub slug: String,
    /// What to install this as — the requested kind whenever `kinds` has it,
    /// since Modrinth types a datapack project as a mod.
    pub kind: ContentKind,
    /// Every kind the project publishes, derived from its loaders.
    pub kinds: Vec<ContentKind>,
    pub title: String,
    pub description: String,
    pub body: String,
    pub author: String,
    pub categories: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: String,
    pub gallery: Vec<GalleryImage>,
    pub client_side: SideSupport,
    pub server_side: SideSupport,
}

/// One downloadable file of a version; `primary` marks the main artifact.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentFile {
    pub artifact: Artifact,
    pub primary: bool,
}

/// How a dependency relates to the version that declares it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    #[default]
    Required,
    Optional,
    Incompatible,
    Embedded,
}

/// A dependency on another project (and optionally a specific version of it).
/// Either id may be empty when the source pins only the other.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentDependency {
    pub project_id: String,
    pub version_id: String,
    pub kind: DependencyKind,
}

/// A downloadable version of a project.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentVersion {
    pub source: String,
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub channel: ReleaseChannel,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub featured: bool,
    pub date_published: String,
    pub downloads: u64,
    pub files: Vec<ContentFile>,
    pub dependencies: Vec<ContentDependency>,
}

/// How search results are ordered.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Newest,
    Updated,
}

/// A paginated search over a source. `source` empty selects the default source;
/// `limit` is clamped to `1..=100` by the provider.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SearchQuery {
    pub source: String,
    pub kind: ContentKind,
    pub query: String,
    pub loader: Option<String>,
    pub game_version: Option<String>,
    pub categories: Vec<String>,
    pub sort: SearchSort,
    pub limit: u32,
    pub offset: u32,
}

/// A page of search hits. `total` is the full match count for paging.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<ContentProject>,
    pub offset: u32,
    pub limit: u32,
    pub total: u32,
}

/// The versions of a project, optionally filtered by loader and game version.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct VersionQuery {
    pub source: String,
    pub project: String,
    pub loader: Option<String>,
    pub game_version: Option<String>,
}

/// One file a modpack pulls in, at its path relative to the game directory.
/// The identity fields are empty where the pack does not say which release it is.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ModpackFile {
    pub path: String,
    pub artifact: Artifact,
    pub client: SideSupport,
    pub server: SideSupport,
    pub source: String,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ContentProject>,
}

/// A resolved modpack: the loader/game version it targets and the files it
/// pulls from the network. A pack's own `overrides/` are carried inside the
/// archive rather than listed here, so they stay an install-time concern (see
/// [`crate::modpack`]) and never cross the wire.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ResolvedModpack {
    pub source: String,
    pub project_id: String,
    pub version_id: String,
    pub version_number: String,
    pub name: String,
    pub summary: String,
    pub game_version: String,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    pub files: Vec<ModpackFile>,
}

/// `kind` is the browse context, stamped onto the answer when supported.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectParams {
    pub source: String,
    pub project: String,
    pub kind: Option<ContentKind>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ResolveUrlParams {
    pub url: String,
}

/// A source page URL resolved to what it names: the project, and the version it
/// pins when the URL is a version page (empty for a project page).
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ResolvedUrl {
    pub project: ContentProject,
    pub version_id: String,
}

/// One installed content item, as the entry's index records it. `source` is a
/// platform id (`modrinth`) or the literal `file` for a local import — imports
/// carry empty project/version ids and cannot be updated.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct InstalledContent {
    pub kind: ContentKind,
    pub source: String,
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub version_id: String,
    pub version_number: String,
    pub filename: String,
    pub sha1: String,
    pub url: String,
    /// The project's icon, carried for the desktop UI. A local-file import has
    /// no project, so it carries a `data:` URI of the icon its archive embeds,
    /// or nothing.
    pub icon_url: String,
    pub installed_unix: i64,
    /// For datapacks: the save worlds (relative to the entry's `data/`) the
    /// managed file is mirrored into, empty meaning every world the entry has.
    /// Always empty for the other kinds, which mirror into one flat dir.
    pub worlds: Vec<String>,
    /// Who put the item in the pool: empty = user-installed; a modpack tags
    /// its installs `modpack:<project>`.
    pub origin: String,
    /// Whether the launch-time mirror places this item in the game's load dirs.
    /// A disabled item keeps its managed copy and provenance, and is not loaded.
    pub enabled: bool,
    /// For datapacks: worlds it targets but is not loaded in — the per-world
    /// twin of `enabled`. Empty for the other kinds.
    pub disabled_worlds: Vec<String>,
}

/// A file found in the entry's game directory that no index entry accounts for
/// — dropped in by hand, so the launcher never touches it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct UntrackedFile {
    /// As it reads in the load dir; a datapack is `<world>/<file>`.
    pub name: String,
    /// Absolute, on the daemon's host — what a co-located front-end reveals.
    pub path: String,
}

/// The installed items of one kind, plus the untracked files beside them.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentListResult {
    pub items: Vec<InstalledContent>,
    pub untracked: Vec<UntrackedFile>,
}

pub const LOCAL_SOURCE: &str = "file";

/// One thing to install: exactly one of `project` (a platform project,
/// optionally pinned by `version`), `url` (a project/version page URL on a
/// supported source), or `path` (a daemon-local file to import). `source`
/// overrides the batch's for this item, so one batch may mix platforms; a URL
/// always names its own source; for a `path` it picks whose match to track.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentAddItem {
    pub project: String,
    pub version: String,
    pub url: String,
    pub path: String,
    pub filename: String,
    pub source: String,
}

/// What to install: one or more items of one `kind` from one `source`,
/// installed in a single job. Items that fail are reported per item on the
/// done event; the rest of the batch proceeds.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentAddSpec {
    pub kind: ContentKind,
    pub source: String,
    pub items: Vec<ContentAddItem>,
    /// For datapacks on an instance: the save worlds to mirror into, empty
    /// meaning every world. Ignored for other kinds; a server always uses its
    /// single `level-name` world.
    pub worlds: Vec<String>,
}

/// One item of a batch that could not be installed.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentFailure {
    /// The selector as given (project slug/id, URL, or path).
    pub item: String,
    /// The resolved project title, when resolution got that far.
    pub title: String,
    /// The structured cause a front-end localizes from.
    pub error: ErrorInfo,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentAddParams {
    pub target: EntryTarget,
    #[serde(flatten)]
    pub spec: ContentAddSpec,
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentListParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
}

/// `items` names what to uninstall, resolved together so a reference matching
/// nothing removes none of them. `worlds` narrows a datapack removal to those
/// save worlds — the pack stops loading in them and its copies there go; it is
/// uninstalled outright when no world is left targeting it. Rejected for the
/// kinds that have no worlds.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentRemoveParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
    pub items: Vec<String>,
    pub worlds: Vec<String>,
}

/// `items` empty updates every platform-sourced item of the kind; naming
/// several updates exactly those, in one job. A `path` instead updates the one
/// named local item from that file, keeping how the entry holds it.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentUpdateParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
    pub items: Vec<String>,
    pub path: String,
    pub id: String,
}

/// Enable or disable one installed item (matched by project id, slug, filename,
/// or title). Disabling drops it from the game's load dirs while keeping the
/// managed copy and provenance; enabling restores the mirror. `worlds` scopes a
/// datapack toggle to those save worlds, leaving the others as they are.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentEnableParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
    pub item: String,
    pub enabled: bool,
    pub worlds: Vec<String>,
}

/// Ask whether each platform-sourced item of the kind has a newer compatible
/// version. Resolves upstream, so it is a plain (network-bound) call rather
/// than an install job.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentCheckUpdatesParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
}

/// One installed item's update status: its current pin against the newest
/// compatible version resolved upstream.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentUpdate {
    pub filename: String,
    pub project_id: String,
    pub current_version_id: String,
    pub current_version_number: String,
    pub latest_version_id: String,
    pub latest_version_number: String,
    /// True when the newest compatible version differs from the current pin.
    pub updatable: bool,
}

/// Re-pin one installed item to a specific published `version` (id or number).
/// The swap re-installs that version like an update, applying at the next
/// start/launch; runs as a content job.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentSetVersionParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
    pub item: String,
    pub version: String,
    /// Empty keeps the item's own source.
    pub source: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct EntryContentMatchesParams {
    pub target: EntryTarget,
    pub kind: ContentKind,
    pub item: String,
}

pub struct EntryContentAdd;
impl Contract for EntryContentAdd {
    const CHANNEL: &'static str = "content.add";
    type Params = EntryContentAddParams;
    type Result = JobStarted;
}

impl Job for EntryContentAdd {
    type Progress = ProvisionProgress;
    type Done = ContentDone;
}

pub struct EntryContentList;
impl Contract for EntryContentList {
    const CHANNEL: &'static str = "content.list";
    type Params = EntryContentListParams;
    type Result = ContentListResult;
}

pub struct EntryContentRemove;
impl Contract for EntryContentRemove {
    const CHANNEL: &'static str = "content.remove";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = EntryContentRemoveParams;
    type Result = Empty;
}

pub struct EntryContentUpdate;
impl Contract for EntryContentUpdate {
    const CHANNEL: &'static str = "content.update";
    type Params = EntryContentUpdateParams;
    type Result = JobStarted;
}

impl Job for EntryContentUpdate {
    type Progress = ProvisionProgress;
    type Done = ContentDone;
}

pub struct EntryContentEnable;
impl Contract for EntryContentEnable {
    const CHANNEL: &'static str = "content.enable";
    const CHANGES: Option<Scope> = Some(Scope::Entries);
    type Params = EntryContentEnableParams;
    type Result = Empty;
}

pub struct EntryContentCheckUpdates;
impl Contract for EntryContentCheckUpdates {
    const CHANNEL: &'static str = "content.check_updates";
    const TIMEOUT: Duration = Duration::from_secs(120);
    type Params = EntryContentCheckUpdatesParams;
    type Result = Vec<ContentUpdate>;
}

pub struct EntryContentMatches;
impl Contract for EntryContentMatches {
    const CHANNEL: &'static str = "content.matches";
    const TIMEOUT: Duration = Duration::from_secs(120);
    type Params = EntryContentMatchesParams;
    type Result = Vec<ContentMatch>;
}

pub struct EntryContentSetVersion;
impl Contract for EntryContentSetVersion {
    const CHANNEL: &'static str = "content.set_version";
    type Params = EntryContentSetVersionParams;
    type Result = JobStarted;
}

impl Job for EntryContentSetVersion {
    type Progress = ProvisionProgress;
    type Done = ContentDone;
}

/// What a content install, update or version change settled with. Cancelling
/// one keeps the items already installed — the batch is per-item, and each one
/// that completed is real.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct ContentDone {
    pub items: Vec<InstalledContent>,
    #[serde(default)]
    pub failures: Vec<ContentFailure>,
}

pub struct ContentSources;
impl Contract for ContentSources {
    const CHANNEL: &'static str = "content.sources";
    type Params = Empty;
    type Result = Vec<ContentSource>;
}

pub struct ContentSearch;
impl Contract for ContentSearch {
    const CHANNEL: &'static str = "content.search";
    type Params = SearchQuery;
    type Result = SearchResult;
}

pub struct ContentProjectGet;
impl Contract for ContentProjectGet {
    const CHANNEL: &'static str = "content.project";
    type Params = ProjectParams;
    type Result = ContentProject;
}

pub struct ContentVersions;
impl Contract for ContentVersions {
    const CHANNEL: &'static str = "content.versions";
    type Params = VersionQuery;
    type Result = Vec<ContentVersion>;
}

pub struct ContentResolveUrl;
impl Contract for ContentResolveUrl {
    const CHANNEL: &'static str = "content.resolve_url";
    type Params = ResolveUrlParams;
    type Result = ResolvedUrl;
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentInspectParams {
    pub path: String,
}

/// A local file's import classification: `valid` when installable as
/// single-file content, `kind` the detected kind (absent for a valid archive of
/// an unrecognised shape — the caller then chooses), and `reason` set when not.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentInspectResult {
    pub valid: bool,
    pub kind: Option<ContentKind>,
    pub filename: String,
    pub reason: String,
    pub matches: Vec<ContentMatch>,
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ContentMatch {
    pub project: ContentProject,
    pub version_number: String,
}

pub struct ContentInspect;
impl Contract for ContentInspect {
    const CHANNEL: &'static str = "content.inspect";
    type Params = ContentInspectParams;
    type Result = ContentInspectResult;
}
