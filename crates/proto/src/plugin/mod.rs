//! The plugin registry's wire types.

mod registry;
mod surface;

pub use registry::{
    PluginDisable, PluginEnable, PluginInfo, PluginKind, PluginList, PluginState, PluginTarget,
};
pub use surface::{
    NoticeLevel, PluginAnswer, PluginAnswerParams, PluginLink, PluginNotice, PluginQuestion,
    PluginQuestionClosed,
};
