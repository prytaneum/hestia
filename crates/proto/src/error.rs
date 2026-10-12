//! The canonical daemon error — a structured, exhaustive value carried whole
//! across the socket. Nobody authors prose at a call site: the English text
//! (`Display`) and the coarse `code` are projections of the variant, and a
//! front-end renders its own localized string from the tag + typed fields.
//!
//! Semantic variants translate fully with their fields; operational variants
//! (`MalformedRequest`, `Io`, `Upstream`, `DownloadFailed`, `RconFailed`,
//! `Internal`) carry an unbounded English `detail` — a per-path filesystem
//! message is not localizable and is shown as secondary text under a translated
//! headline. A semantic variant never carries prose: what it would have said is
//! a closed enum a front-end words itself, and the specifics go to the log.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::content::ContentKind;

/// A launcher entry that resolves by reference.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Server,
    Instance,
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EntryKind::Server => "server",
            EntryKind::Instance => "instance",
        })
    }
}

/// A uniquely-named thing that can already exist.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Nameable {
    Server,
    Instance,
}

impl fmt::Display for Nameable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Nameable::Server => "server",
            Nameable::Instance => "instance",
        })
    }
}

/// A required-or-invalid input, named so a front-end can label it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Name,
    Project,
    Version,
    Item,
    Backup,
    Command,
    Program,
    Url,
    Path,
    Flavor,
    World,
    Address,
    Memory,
    JvmArgs,
    Port,
    Players,
    Player,
    BackupInterval,
    BackupRetention,
    JavaVersion,
    Order,
    Realm,
    ProcessId,
}

impl Field {
    /// The ways a content item names where it comes from; exactly one is given.
    pub const CONTENT_SOURCES: [Field; 3] = [Field::Project, Field::Url, Field::Path];
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Field::Name => "a name",
            Field::Project => "a project",
            Field::Version => "a version",
            Field::Item => "an item",
            Field::Backup => "a backup",
            Field::Command => "a command",
            Field::Program => "a program",
            Field::Url => "a download url",
            Field::Path => "a file path",
            Field::Flavor => "a flavor",
            Field::World => "a world",
            Field::Address => "a server address",
            Field::Memory => "memory",
            Field::JvmArgs => "jvm arguments",
            Field::Port => "a port",
            Field::Players => "players",
            Field::Player => "a player name",
            Field::Order => "a list order",
            Field::Realm => "a realm",
            Field::BackupInterval => "backup-interval",
            Field::BackupRetention => "backup-retention",
            Field::JavaVersion => "a java version",
            Field::ProcessId => "a process id",
        })
    }
}

/// A closed reason an otherwise-present value was rejected.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    MemoryFormat,
    JvmArgsPrefix,
    PortNumber,
    PortRange,
    WholeNumber,
    IntervalFormat,
    IntervalTooShort,
    RetentionPositive,
    MinPlayers,
    MinBackups,
    JavaMajor,
    AbsolutePath,
    ServerAddress,
    ListOrder,
    RealmId,
    ProcessIdCharset,
    PlayerName,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Reason::MemoryFormat => "memory must look like 4G or 2048M",
            Reason::JvmArgsPrefix => "jvm arguments must start with '-'",
            Reason::PortNumber => "port must be a number",
            Reason::PortRange => "port is out of range",
            Reason::WholeNumber => "enter a whole number",
            Reason::IntervalFormat => "backup-interval must look like 30m, 6h, or 1d",
            Reason::IntervalTooShort => "backup-interval must be at least 5m",
            Reason::RetentionPositive => "backup-retention must be a positive integer",
            Reason::MinPlayers => "at least one player is required",
            Reason::MinBackups => "keep at least one backup",
            Reason::JavaMajor => "not a valid java major version",
            Reason::AbsolutePath => {
                "the path must be absolute — the daemon is a separate process and does not share \
                 your working directory"
            }
            Reason::ServerAddress => "a server address looks like host or host:port",
            Reason::ListOrder => "the order must name every server in the list exactly once",
            Reason::RealmId => "a realm is named by its numeric id",
            Reason::ProcessIdCharset => {
                "a process id may only contain letters, digits, '-', '_' and '.'"
            }
            Reason::PlayerName => {
                "a player name is up to 32 letters, digits, '_' or '.', with no spaces"
            }
        })
    }
}

/// A domain rule that forbids an otherwise well-formed operation.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    WorldsForDatapacksOnly,
    DatapacksPerWorld,
    ModpackNotSingleFile,
    ModpackNotUpdatable,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Unsupported::WorldsForDatapacksOnly => "worlds apply to datapacks only",
            Unsupported::DatapacksPerWorld => "only datapacks are installed per world",
            Unsupported::ModpackNotSingleFile => {
                "modpack content cannot be installed as a single file"
            }
            Unsupported::ModpackNotUpdatable => {
                "this modpack came from a file, so there is no source to update it against"
            }
        })
    }
}

/// What an archive claims to be, so a refusal can name it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    Zip,
    Hestia,
    Mrpack,
    Prism,
    #[serde(rename = "curseforge")]
    CurseForge,
    Backup,
}

impl fmt::Display for ArchiveFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ArchiveFormat::Zip => "zip",
            ArchiveFormat::Hestia => "hestia",
            ArchiveFormat::Mrpack => "mrpack",
            ArchiveFormat::Prism => "prism",
            ArchiveFormat::CurseForge => "curseforge",
            ArchiveFormat::Backup => "backup",
        })
    }
}

/// Why an archive or a pack could not be read.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFault {
    /// Cut short, or not a zip at all.
    NotAnArchive,
    ManifestMissing,
    ManifestMalformed,
    /// A member path that would land outside the directory it is unpacked into.
    UnsafePath,
    FormatVersion,
    ManifestType,
    NoGameVersion,
    NoComponents,
    /// The pack lists a file its source no longer publishes.
    FileUnpublished,
}

impl fmt::Display for ArchiveFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ArchiveFault::NotAnArchive => "the file is incomplete or not a zip archive",
            ArchiveFault::ManifestMissing => "its manifest is missing",
            ArchiveFault::ManifestMalformed => "its manifest is malformed",
            ArchiveFault::UnsafePath => "it holds a file path that escapes its directory",
            ArchiveFault::FormatVersion => "its format version is not supported",
            ArchiveFault::ManifestType => "its manifest is of an unsupported type",
            ArchiveFault::NoGameVersion => "it pins no Minecraft version",
            ArchiveFault::NoComponents => "it lists no components",
            ArchiveFault::FileUnpublished => "it lists a file that is not published any more",
        })
    }
}

/// Why a path in an entry's file tree was refused.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FileFault {
    /// Absolute, traversing, or reaching out of the tree through a link.
    OutsideEntry,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    /// Past the size a file is read or written whole at.
    TooLarge,
    NotText,
    /// Neither a `.zip` nor a `.tar.gz`.
    ArchiveFormat,
    /// A directory moved or copied to somewhere beneath itself.
    IntoItself,
}

impl fmt::Display for FileFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FileFault::OutsideEntry => "it is outside the server's directory",
            FileFault::AlreadyExists => "something with that name already exists",
            FileFault::NotADirectory => "it is not a directory",
            FileFault::IsADirectory => "it is a directory",
            FileFault::TooLarge => "it is too large to edit",
            FileFault::NotText => "it is not a text file",
            FileFault::ArchiveFormat => "it is not a .zip or .tar.gz archive",
            FileFault::IntoItself => "a directory cannot be placed inside itself",
        })
    }
}

/// The JSON kind a config key holds.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ValueKind::Null => "null",
            ValueKind::Boolean => "boolean",
            ValueKind::Number => "number",
            ValueKind::String => "string",
            ValueKind::Array => "array",
            ValueKind::Object => "object",
        })
    }
}

/// An upstream service the daemon depends on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Adoptium,
    Mojang,
    Fabric,
    NeoForge,
    Paper,
    Spigot,
    Modrinth,
    CurseForge,
    Microsoft,
    Xbox,
    /// The release feed self-update follows.
    Release,
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Service::Adoptium => "Adoptium",
            Service::Mojang => "Mojang",
            Service::Fabric => "Fabric",
            Service::NeoForge => "NeoForge",
            Service::Paper => "PaperMC",
            Service::Spigot => "SpigotMC",
            Service::Modrinth => "Modrinth",
            Service::CurseForge => "CurseForge",
            Service::Microsoft => "Microsoft",
            Service::Xbox => "Xbox",
            Service::Release => "the release feed",
        })
    }
}

/// The filesystem action an `Io` failure was performing.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum IoOp {
    Create,
    Read,
    Write,
    Remove,
    Move,
    Copy,
    Open,
    Link,
    Unlink,
    Extract,
}

impl fmt::Display for IoOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IoOp::Create => "create",
            IoOp::Read => "read",
            IoOp::Write => "write",
            IoOp::Remove => "remove",
            IoOp::Move => "move",
            IoOp::Copy => "copy",
            IoOp::Open => "open",
            IoOp::Link => "link",
            IoOp::Unlink => "unlink",
            IoOp::Extract => "extract",
        })
    }
}

/// What there was nothing to do.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Install,
    Modify,
    BackUp,
}

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Task::Install => "install",
            Task::Modify => "change",
            Task::BackUp => "back up",
        })
    }
}

/// The one daemon error type — every failure the socket surfaces. The `kind`
/// tag is the wire discriminant; front-ends switch on it exhaustively.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorInfo {
    // --- validation ---
    FieldRequired {
        field: Field,
    },
    FieldsRequired {
        fields: Vec<Field>,
    },
    InvalidValue {
        field: Field,
        reason: Reason,
    },
    MutuallyExclusive {
        options: Vec<Field>,
    },
    NothingToDo {
        what: Task,
    },
    EulaRequired,
    UnsupportedOperation {
        reason: Unsupported,
    },
    /// What an entry can take is a property of its side *and* its flavor — a
    /// paper server takes plugins where a fabric one takes mods — so the
    /// accepted set travels with the refusal rather than being spelled out in
    /// a sentence that goes stale when a flavor is added.
    ContentKindRejected {
        entry: EntryKind,
        flavor: String,
        requested: ContentKind,
        accepts: Vec<ContentKind>,
    },
    /// A flavor needs something installed that Hestia cannot install itself
    /// (Spigot and CraftBukkit are compiled on the machine, which needs Git).
    /// Carries where to get it, so no front-end has to know.
    MissingRequirement {
        flavor: String,
        name: String,
        url: String,
    },
    TextureNotPng,
    TextureSizeUnsupported {
        width: u32,
        height: u32,
    },
    /// A launch asked to join a world or server directly, but the game only
    /// learned the Quick Play arguments in 1.20 — an older client would ignore
    /// them and open to the title screen, which is not what was asked for.
    QuickPlayUnsupported {
        version: String,
    },

    // --- not found ---
    EntryNotFound {
        entry: EntryKind,
        reference: String,
    },
    ProcessNotFound {
        id: String,
    },
    BackupNotFound {
        reference: String,
    },
    ContentNotFound {
        reference: String,
    },
    SkinNotFound {
        key: String,
    },
    WorldNotFound {
        world: String,
    },
    /// No entry of the instance's multiplayer list (`servers.dat`) matches, by
    /// name or by address.
    ServerListEntryNotFound {
        reference: String,
    },
    AccountNotFound {
        reference: String,
    },
    VersionNotFound {
        reference: String,
    },
    /// No file or directory at `path` in the entry's tree.
    FileNotFound {
        path: String,
    },
    FileRejected {
        path: String,
        reason: FileFault,
    },
    ConfigKeyUnknown {
        key: String,
    },
    ConfigKeyUnset {
        key: String,
    },
    ConfigTypeMismatch {
        key: String,
        expected: ValueKind,
    },
    /// The value is the right JSON kind but not one the key accepts.
    ConfigRejected {
        key: String,
    },
    /// A hardcore server pins `key` to `value`; hardcore has to go first.
    HardcoreLocked {
        key: String,
        value: String,
    },
    /// Hardcore was asked for while these `server.properties` settings forbid it.
    HardcoreRequires {
        settings: Vec<String>,
    },

    // --- conflict ---
    AlreadyExists {
        entry: Nameable,
        name: String,
    },
    PortUnavailable {
        port: u16,
    },

    // --- state ---
    EntryRunning {
        entry: EntryKind,
        name: String,
    },
    /// A concurrent session was asked for while `instance.multi-session` is
    /// off. Distinct from `EntryRunning`: nothing is in the way, the capability
    /// is simply switched off, and the fix is a setting rather than a stop.
    MultiSessionDisabled {
        name: String,
    },
    NotRunning {
        entry: EntryKind,
        name: String,
    },
    Provisioning {
        name: String,
    },
    UpdateInProgress {
        name: String,
    },
    ContentInProgress {
        name: String,
    },
    BackupInProgress {
        name: String,
    },
    ExportInProgress {
        name: String,
    },
    ImportInProgress,
    CreateInProgress,
    JavaInstallInProgress {
        major: i32,
    },
    NoConsole {
        name: String,
    },
    NoGamePort {
        name: String,
    },

    // --- auth ---
    SignInRequired,
    SessionExpired {
        reference: String,
    },
    LoginDeclined,
    LoginTimedOut,

    // --- content / modpack ---
    NotAModpack {
        reference: String,
    },
    ModpackInvalid {
        reason: ArchiveFault,
    },
    /// The pack pins a mod loader this launcher has no flavor for. Carried as
    /// the loader's own name, since the set of shipped flavors moves.
    ModpackLoaderUnsupported {
        loader: String,
    },
    /// The pack targets a different game version or loader than the entry it
    /// was aimed at. Both are baked into the entry's resolved profile, so the
    /// pack cannot be installed there — a new entry has to be created instead.
    /// The entry is not named: the caller named it in the request, and a fifth
    /// string here would make `ErrorInfo` large enough to be worth boxing at
    /// every call site that returns one.
    // The enum's `rename_all` renames variants, not the fields inside one, so a
    // multi-word field here needs its own attribute or it ships snake_case.
    #[serde(rename_all = "camelCase")]
    ModpackEntryMismatch {
        entry: EntryKind,
        flavor: String,
        game_version: String,
        pack_flavor: String,
        pack_game_version: String,
    },
    /// An entry with no pack was asked to update or remove one.
    ModpackNotInstalled {
        entry: EntryKind,
        name: String,
    },
    UnsupportedContentUrl {
        url: String,
    },

    // --- import / export ---
    /// The archive carries no marker file for any format hestia imports. Named
    /// by filename rather than full path: the caller chose the file and the
    /// path is theirs, but the name is what identifies it in a message.
    ArchiveUnrecognised {
        filename: String,
    },
    /// The archive cannot be read as what it claims to be.
    ArchiveInvalid {
        format: ArchiveFormat,
        reason: ArchiveFault,
    },
    /// The archive pins a loader this launcher has no flavor for.
    ArchiveUnsupported {
        loader: String,
    },
    /// The archive was written by a newer hestia than this one reads.
    ArchiveTooNew {
        format: ArchiveFormat,
        schema: u32,
    },
    ContentKindMismatch {
        title: String,
        actual: ContentKind,
        expected: ContentKind,
    },
    /// A registered source that cannot serve requests as configured — today,
    /// a platform whose API key is unset.
    ContentSourceUnavailable {
        source: String,
    },
    /// The source lists the file but publishes no download for it: CurseForge
    /// lets an author opt out of third-party distribution. Nothing to retry —
    /// the file has to come from the project page by hand.
    ContentDownloadBlocked {
        title: String,
        source: String,
    },

    // --- sync ---
    /// Turning a unit on has to start the shared copy from *someone's*, and
    /// more than one instance holds that file. Letting the launcher pick is how
    /// settings go missing quietly, so the choice is refused back to the caller
    /// with the instances it could have meant.
    SyncSourceRequired {
        unit: crate::sync::SyncUnit,
        candidates: Vec<String>,
    },
    /// An edit to a shared copy nobody reads: the unit is off in the catalogue.
    SyncUnitDisabled {
        unit: crate::sync::SyncUnit,
    },

    // --- protocol ---
    UnknownChannel {
        channel: String,
    },
    MalformedRequest {
        detail: String,
    },
    IncompatibleVersion {
        got: i64,
        want: i64,
    },

    // --- operational (unbounded English `detail`) ---
    /// Applying an update needs root and the daemon could not obtain it — no
    /// polkit agent answered and there is no terminal to prompt on. Carries the
    /// exact command, so a front-end with a terminal can hand it to the user
    /// rather than describing what they ought to type.
    ElevationRequired {
        command: String,
    },
    Io {
        operation: IoOp,
        detail: String,
    },
    /// The request never left the machine. Distinct from `Upstream`, where the
    /// service answered and the answer was a failure: only this one means "try
    /// again when you are back online".
    Offline {
        /// Absent for a plain file download, aimed at whatever URL a profile named.
        service: Option<Service>,
    },
    /// Nothing was attempted: the user pinned `network.offline`. Its own variant
    /// rather than a flag on `Offline` because the remedy is different — a
    /// setting to turn off, not a connection to wait for.
    OfflineMode,
    Upstream {
        service: Service,
        detail: String,
    },
    DownloadFailed {
        detail: String,
    },
    RconFailed {
        detail: String,
    },
    Internal {
        detail: String,
    },
}

/// A neutral fallback so structs embedding an `ErrorInfo` can keep the file's
/// defensive `#[serde(default)]` — never authored deliberately.
impl Default for ErrorInfo {
    fn default() -> Self {
        ErrorInfo::Internal {
            detail: String::new(),
        }
    }
}

impl ErrorInfo {
    /// The coarse `ipc::errors` category this variant answers with.
    pub fn code(&self) -> &'static str {
        use ErrorInfo::*;
        match self {
            EntryNotFound { .. }
            | ProcessNotFound { .. }
            | BackupNotFound { .. }
            | ContentNotFound { .. }
            | SkinNotFound { .. }
            | WorldNotFound { .. }
            | ServerListEntryNotFound { .. }
            | AccountNotFound { .. }
            | VersionNotFound { .. }
            | FileNotFound { .. }
            | ConfigKeyUnknown { .. }
            | ConfigKeyUnset { .. } => "not_found",
            SignInRequired | SessionExpired { .. } | LoginDeclined | LoginTimedOut => {
                "unauthorized"
            }
            UnknownChannel { .. } => "unknown_channel",
            IncompatibleVersion { .. } => "version_mismatch",
            Offline { .. } | OfflineMode => "offline",
            ElevationRequired { .. }
            | Io { .. }
            | Upstream { .. }
            | DownloadFailed { .. }
            | RconFailed { .. }
            | Internal { .. } => "handler_error",
            _ => "bad_request",
        }
    }
}

impl fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ErrorInfo::*;
        match self {
            FieldRequired { field } => write!(f, "{field} is required"),
            FieldsRequired { fields } => {
                let joined = fields
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(" and ");
                write!(f, "{joined} are required")
            }
            InvalidValue { reason, .. } => write!(f, "{reason}"),
            MutuallyExclusive { options } => {
                let joined = options
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "choose exactly one of: {joined}")
            }
            NothingToDo { what } => write!(f, "nothing to {what}"),
            EulaRequired => write!(f, "accept the EULA to create a server"),
            UnsupportedOperation { reason } => write!(f, "{reason}"),
            ContentKindRejected {
                entry,
                flavor,
                requested,
                accepts,
            } => {
                let taken = accepts
                    .iter()
                    .map(|k| format!("{k}s"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let taken = if taken.is_empty() {
                    "no content".to_string()
                } else {
                    taken
                };
                write!(
                    f,
                    "a {flavor} {entry} cannot take {requested}s — it takes {taken}"
                )
            }
            MissingRequirement { flavor, name, url } => write!(
                f,
                "a {flavor} server is built on this computer, and that needs {name}, \
                 which Hestia cannot install for you — get it from {url}, then try again"
            ),
            TextureNotPng => write!(f, "the skin texture is not a PNG file"),
            TextureSizeUnsupported { width, height } => write!(
                f,
                "a skin texture must be 64×64 (or the legacy 64×32), got {width}×{height}"
            ),
            QuickPlayUnsupported { version } => write!(
                f,
                "joining a world or server on launch needs Minecraft 1.20 or newer; this instance \
                 runs {version}"
            ),
            EntryNotFound { entry, reference } => write!(f, "no {entry} matches '{reference}'"),
            ProcessNotFound { id } => write!(f, "no process '{id}'"),
            BackupNotFound { reference } => write!(f, "no backup matches '{reference}'"),
            ContentNotFound { reference } => {
                write!(f, "no installed content matches '{reference}'")
            }
            SkinNotFound { key } => write!(f, "no skin matches '{key}'"),
            WorldNotFound { world } => write!(f, "no world '{world}' in this instance"),
            ServerListEntryNotFound { reference } => write!(
                f,
                "no server named '{reference}' in this instance's multiplayer list"
            ),
            AccountNotFound { reference } => write!(f, "no account matches '{reference}'"),
            VersionNotFound { reference } => write!(f, "no version matches '{reference}'"),
            FileNotFound { path } => write!(f, "no file or directory at '{path}'"),
            FileRejected { path, reason } => write!(f, "'{path}' was refused: {reason}"),
            ConfigKeyUnknown { key } => write!(f, "unknown config key '{key}'"),
            ConfigKeyUnset { key } => write!(f, "'{key}' is not set"),
            ConfigTypeMismatch { key, expected } => write!(f, "{key} expects a {expected}"),
            ConfigRejected { key } => write!(f, "invalid value for {key}"),
            HardcoreLocked { key, value } => write!(
                f,
                "{key} is locked to '{value}' while hardcore is enabled; disable hardcore first"
            ),
            HardcoreRequires { settings } => write!(
                f,
                "hardcore requires {}; set them first",
                settings.join(" and ")
            ),
            AlreadyExists { entry, name } => write!(f, "a {entry} named '{name}' already exists"),
            PortUnavailable { port } => write!(f, "port {port} is unavailable"),
            EntryRunning { name, .. } => write!(f, "{name} is running — stop it first"),
            MultiSessionDisabled { name } => write!(
                f,
                "{name} is already running — running several sessions at once is off; turn it on \
                 with 'config set instance.multi-session true'"
            ),
            NotRunning { name, .. } => write!(f, "{name} is not running"),
            Provisioning { name } => write!(f, "{name} is still being set up"),
            UpdateInProgress { name } => write!(f, "{name} is being updated"),
            ContentInProgress { name } => write!(f, "{name} has a content change in progress"),
            BackupInProgress { name } => {
                write!(f, "{name} has a backup or restore in progress")
            }
            ExportInProgress { name } => write!(f, "{name} is being exported"),
            ImportInProgress => write!(f, "that import is already running"),
            CreateInProgress => write!(f, "that server is already being created"),
            JavaInstallInProgress { major } => {
                write!(f, "java {major} is already being installed")
            }
            NoConsole { name } => write!(f, "{name} has no console yet — restart it"),
            NoGamePort { name } => write!(f, "{name} has no game port allocated"),
            SignInRequired => write!(f, "sign in with a Microsoft account first"),
            SessionExpired { reference } => {
                write!(f, "your sign-in for '{reference}' expired — sign in again")
            }
            LoginDeclined => write!(f, "the sign-in was declined"),
            LoginTimedOut => write!(f, "the sign-in timed out — try again"),
            NotAModpack { reference } => write!(f, "'{reference}' is not a modpack"),
            ModpackInvalid { reason } => write!(f, "this modpack could not be read: {reason}"),
            ModpackLoaderUnsupported { loader } => {
                write!(
                    f,
                    "this modpack needs the {loader} loader, which hestia does not have"
                )
            }
            ModpackEntryMismatch {
                entry,
                flavor,
                game_version,
                pack_flavor,
                pack_game_version,
            } => write!(
                f,
                "this modpack is for {pack_flavor} {pack_game_version}, but that {entry} is \
                 {flavor} {game_version} — create a new {entry} from the pack instead"
            ),
            ModpackNotInstalled { entry, name } => {
                write!(f, "{entry} '{name}' was not built from a modpack")
            }
            UnsupportedContentUrl { url } => {
                write!(
                    f,
                    "'{url}' is not a project URL on a supported content source"
                )
            }
            ArchiveUnrecognised { filename } => write!(
                f,
                "'{filename}' is not an instance hestia can import — it carries no hestia, \
                 Modrinth, CurseForge or Prism manifest"
            ),
            ArchiveInvalid { format, reason } => {
                write!(f, "this {format} archive could not be read: {reason}")
            }
            ArchiveUnsupported { loader } => write!(
                f,
                "this archive needs the {loader} loader, which hestia does not have"
            ),
            ArchiveTooNew { format, schema } => write!(
                f,
                "this {format} archive was written by a newer hestia (schema {schema}) — \
                 update hestia to read it"
            ),
            ContentKindMismatch {
                title,
                actual,
                expected,
            } => write!(f, "'{title}' is {actual} content, not {expected}"),
            ContentSourceUnavailable { source } => {
                write!(f, "the {source} content source is not configured")
            }
            ContentDownloadBlocked { title, source } => write!(
                f,
                "'{title}' cannot be downloaded through the {source} API — \
                 get the file from its project page and import it"
            ),
            SyncSourceRequired { unit, candidates } => write!(
                f,
                "name the instance to start {unit} from: {}",
                candidates.join(", ")
            ),
            SyncUnitDisabled { unit } => {
                write!(f, "{unit} is not shared between your instances")
            }
            UnknownChannel { channel } => write!(f, "unknown channel: {channel}"),
            MalformedRequest { detail } => write!(f, "malformed request: {detail}"),
            IncompatibleVersion { got, want } => {
                write!(
                    f,
                    "unsupported protocol version {got}; this daemon speaks version {want}"
                )
            }
            ElevationRequired { command } => write!(
                f,
                "installing this update needs administrator rights — run: {command}"
            ),
            Io { operation, detail } => write!(f, "could not {operation}: {detail}"),
            Offline {
                service: Some(service),
            } => {
                write!(f, "{service} is unreachable — you appear to be offline")
            }
            Offline { service: None } => {
                write!(f, "the download could not start — you appear to be offline")
            }
            OfflineMode => write!(f, "hestia is in offline mode"),
            Upstream { service, detail } => write!(f, "{service} request failed: {detail}"),
            DownloadFailed { detail } => write!(f, "download failed: {detail}"),
            RconFailed { detail } => write!(f, "server console command failed: {detail}"),
            Internal { detail } => write!(f, "{detail}"),
        }
    }
}

impl std::error::Error for ErrorInfo {}
