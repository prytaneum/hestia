//! The plugin registry's wire types.

mod grant;
mod registry;
mod repository;
mod surface;
mod update;

pub use grant::{
    Grant, GrantedPermission, Inspection, PermissionScope, PluginInspect, PluginInstall,
    PluginInstallParams, PluginPackage, Program, ScopeKind, Tier,
};
pub use registry::{
    PluginDisable, PluginEnable, PluginInfo, PluginKind, PluginList, PluginRemove,
    PluginSecretParams, PluginSecretSet, PluginSource, PluginState, PluginTarget,
};
pub use repository::{
    PluginInspectRelease, PluginInstallRelease, PluginInstallReleaseParams, PluginReleaseParams,
    PluginRepoAdd, PluginRepoAddParams, PluginRepoList, PluginRepoRemove, RepositoryInfo,
};
pub use surface::{
    NoticeLevel, PluginAnswer, PluginAnswerParams, PluginLink, PluginNotice, PluginQuestion,
    PluginQuestionClosed,
};
pub use update::{
    PluginUpdate, PluginUpdateApply, PluginUpdateApplyParams, PluginUpdateInspect, PluginUpdates,
    PluginUpdatesResult, RepositoryFailure, UpdateInspection,
};
