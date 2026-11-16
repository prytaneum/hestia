//! The plugin registry's wire types.

mod command;
mod grant;
mod registry;
mod repository;
mod surface;
mod update;

pub use command::{
    ArgKind, ArgSpec, CommandSpec, PluginRun, PluginRunDone, PluginRunParams, PluginRunProgress,
};
pub use grant::{
    Grant, GrantedPermission, GrantedSchedule, Inspection, PermissionScope, PluginInspect,
    PluginInstall, PluginInstallParams, PluginPackage, Program, ScopeKind, SignerInfo, Tier,
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
    PluginRevoked, PluginUpdate, PluginUpdateApply, PluginUpdateApplyParams, PluginUpdateInspect,
    PluginUpdates, PluginUpdatesResult, RepositoryFailure, UpdateInspection,
};
