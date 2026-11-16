//! The plugin registry's wire types.

mod grant;
mod registry;
mod surface;

pub use grant::{
    Grant, GrantedPermission, Inspection, PermissionScope, PluginInspect, PluginInstall,
    PluginInstallParams, PluginPackage, Program, ScopeKind, Tier,
};
pub use registry::{
    PluginDisable, PluginEnable, PluginInfo, PluginKind, PluginList, PluginRemove,
    PluginSecretParams, PluginSecretSet, PluginState, PluginTarget,
};
pub use surface::{
    NoticeLevel, PluginAnswer, PluginAnswerParams, PluginLink, PluginNotice, PluginQuestion,
    PluginQuestionClosed,
};
