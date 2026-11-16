//! What a plugin asks a front-end to put in front of the user. The front-end
//! draws every one of these itself and names the plugin; the plugin supplies
//! text only.

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, Empty, Topic};

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    #[default]
    Info,
    Warn,
    Error,
}

/// A message from a plugin.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginNotice {
    pub plugin: String,
    pub level: NoticeLevel,
    pub message: String,
    /// The job whose run drew this, when one did: front-ends that started the
    /// run subscribe to it by this id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Topic for PluginNotice {
    const TOPIC: &'static str = "plugin.notice";
}

/// A question from a plugin that waits for `plugin.answer`. Unanswered, it
/// counts as declined; `plugin.question.closed` says when it no longer waits.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginQuestion {
    pub request: String,
    pub plugin: String,
    pub message: String,
    /// The job whose run drew this, when one did: front-ends that started the
    /// run subscribe to it by this id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Topic for PluginQuestion {
    const TOPIC: &'static str = "plugin.question";
}

/// A question that no longer waits: answered, timed out, or its call ended.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginQuestionClosed {
    pub request: String,
    /// The job whose run drew this, when one did: front-ends that started the
    /// run subscribe to it by this id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Topic for PluginQuestionClosed {
    const TOPIC: &'static str = "plugin.question.closed";
}

/// An https address a plugin offers. The front-end shows the destination and
/// opens it only when the user agrees.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginLink {
    pub plugin: String,
    pub url: String,
    /// The job whose run drew this, when one did: front-ends that started the
    /// run subscribe to it by this id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Topic for PluginLink {
    const TOPIC: &'static str = "plugin.link";
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct PluginAnswerParams {
    pub request: String,
    /// The plugin the question came from; an answer naming another is ignored.
    pub plugin: String,
    pub accepted: bool,
}

/// The user's answer to a `plugin.question`; the first answer wins and a
/// request nobody is waiting on is ignored.
pub struct PluginAnswer;
impl Contract for PluginAnswer {
    const CHANNEL: &'static str = "plugin.answer";
    type Params = PluginAnswerParams;
    type Result = Empty;
}
