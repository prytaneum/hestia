//! The `content` provider kind: a source of mods, packs and other content. The
//! guest only describes what exists; the host downloads the files it lists, and
//! only from hosts the plugin was granted.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{ProviderKind, ProviderOp};
use crate::content::{
    ContentKind, ContentProject, ContentVersion, ResolvedModpack, SearchQuery, SearchResult,
    SourceFileRef, SourceInfo, SourceUrl, VersionQuery,
};
use crate::contract::Empty;
use crate::plugin::Permission;

pub struct Source;
impl ProviderKind for Source {
    const KIND: &'static str = "content";
    const PERMISSIONS: &'static [Permission] =
        &[Permission::ContentRead, Permission::NetworkConnect];
}

/// A URL to recognise.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct UrlQuery {
    pub url: String,
}

/// One project by id or slug, optionally narrowed to a kind.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectQuery {
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ContentKind>,
}

/// Several projects or versions by id.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct IdQuery {
    pub ids: Vec<String>,
}

/// A local file being identified: its sha1, and where the guest can read it
/// during the call, read-only.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct IdentifyFile {
    pub sha1: String,
    pub path: String,
}

/// The local files to identify.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct IdentifyQuery {
    pub files: Vec<IdentifyFile>,
}

/// A pack manifest, as the pack's archive carries it.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(default, rename_all = "camelCase")]
pub struct PackManifestQuery {
    #[cfg_attr(feature = "ts", ts(type = "unknown"))]
    pub manifest: serde_json::Value,
}

/// Describes the source. Asked once, when the plugin is enabled.
pub struct Info;
impl ProviderOp for Info {
    type Kind = Source;
    const OP: &'static str = "info";
    type Args = Empty;
    type Reply = SourceInfo;
}

/// Recognises a page URL on the source's site; `None` when it is not one.
/// Optional: unsupported reads as `None`.
pub struct ParseUrl;
impl ProviderOp for ParseUrl {
    type Kind = Source;
    const OP: &'static str = "parse-url";
    type Args = UrlQuery;
    type Reply = Option<SourceUrl>;
}

/// Recognises the project and version behind a file's URL, such as a `.mrpack`
/// download; `None` when the file is not this source's. Optional.
pub struct RecogniseFileUrl;
impl ProviderOp for RecogniseFileUrl {
    type Kind = Source;
    const OP: &'static str = "recognise-file-url";
    type Args = UrlQuery;
    type Reply = Option<SourceFileRef>;
}

pub struct Search;
impl ProviderOp for Search {
    type Kind = Source;
    const OP: &'static str = "search";
    type Args = SearchQuery;
    type Reply = SearchResult;
}

pub struct Project;
impl ProviderOp for Project {
    type Kind = Source;
    const OP: &'static str = "project";
    type Args = ProjectQuery;
    type Reply = ContentProject;
}

/// Several projects at once. Optional: unsupported asks [`Project`] for each.
pub struct Projects;
impl ProviderOp for Projects {
    type Kind = Source;
    const OP: &'static str = "projects";
    type Args = IdQuery;
    type Reply = Vec<ContentProject>;
}

/// The versions of a project. A file whose URL is not on one of the plugin's
/// network hosts is dropped by the host.
pub struct Versions;
impl ProviderOp for Versions {
    type Kind = Source;
    const OP: &'static str = "versions";
    type Args = VersionQuery;
    type Reply = Vec<ContentVersion>;
}

/// Versions by id. Optional: unsupported reads as none found.
pub struct VersionsById;
impl ProviderOp for VersionsById {
    type Kind = Source;
    const OP: &'static str = "versions-by-id";
    type Args = IdQuery;
    type Reply = Vec<ContentVersion>;
}

/// The version each file is a release of, keyed by the file's sha1. During the
/// call, and only then, every file is mounted read-only at its `path`, so the
/// guest can compute the hash its source indexes by. Optional: unsupported
/// reads as none identified.
pub struct Identify;
impl ProviderOp for Identify {
    type Kind = Source;
    const OP: &'static str = "identify";
    type Args = IdentifyQuery;
    type Reply = HashMap<String, ContentVersion>;
}

/// Resolves a pack manifest the source owns into the files it names, with the
/// game version and loader it targets. A file is held to the same rule as any
/// download the source names: its path stays inside the entry, and its URL is
/// on one of the plugin's network hosts and verified by a checksum, or empty
/// for a file its author withholds. Optional: unsupported fails the import with
/// the source named.
pub struct ResolveManifest;
impl ProviderOp for ResolveManifest {
    type Kind = Source;
    const OP: &'static str = "resolve-manifest";
    type Args = PackManifestQuery;
    type Reply = ResolvedModpack;
}
