//! Drawing the pages the manifest contributes under `contributes.pages`, and
//! answering what the user does on them. The host calls
//! [`Plugin::page`](crate::Plugin::page) to draw a page and again for each click
//! or submit; route each page id to its two handlers with [`Pages::on`]. A page
//! no handler takes answers `unsupported`.
//!
//! What a button or form sends back is the plugin's own enum, written to the
//! node by the [`ui`](crate::ui) builders and read back typed in the action
//! handler. Tag it so it matches the wire form of an
//! [`Action`](crate::ui::Action): `id` is the variant and `payload` its data.
//!
//! ```no_run
//! use hestia::pages::{Pages, Paged, Values};
//! use hestia::ui::{self, Effect, Tone};
//! use hestia::{Error, Plugin};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! #[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
//! enum Act {
//!     Backup,
//!     Remove(String),
//! }
//!
//! #[derive(Deserialize)]
//! struct Rename {
//!     name: String,
//! }
//!
//! struct Backups;
//!
//! impl Plugin for Backups {
//!     fn page(call: Pages) -> Paged {
//!         call.on(
//!             "overview",
//!             |context| {
//!                 Ok(ui::stack([
//!                     ui::text(context.entry.unwrap_or_default()),
//!                     ui::button("Back up now", Act::Backup),
//!                 ]))
//!             },
//!             |act: Act, values: Values, _context| match act {
//!                 Act::Backup => Ok(vec![Effect::Refresh]),
//!                 Act::Remove(world) => {
//!                     let rename: Rename = values.parse()?;
//!                     Ok(vec![Effect::Toast {
//!                         tone: Tone::Success,
//!                         text: format!("{world} -> {}", rename.name),
//!                     }])
//!                 }
//!             },
//!         )
//!         .finish()
//!     }
//! }
//!
//! hestia::plugin!(Backups);
//! ```

use proto::plugin::PageContext;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::bindings::hestia::plugin::types::PluginError;
use crate::ui::{Action, Effect, UiNode};
use crate::Error;

/// The fields a form submitted, by name; empty after a button.
///
/// ```
/// use hestia::pages::Values;
///
/// #[derive(serde::Deserialize)]
/// struct Form {
///     name: String,
///     keep: u32,
/// }
///
/// let values: Values =
///     serde_json::from_str(r#"{"name":"smp","keep":7}"#).unwrap();
/// assert_eq!(values.get::<u32>("keep")?, Some(7));
/// assert_eq!(values.parse::<Form>()?.name, "smp");
/// # Ok::<(), hestia::Error>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
pub struct Values(Value);

impl Values {
    /// One field decoded as `T`, or `None` when the form did not submit it. A
    /// field of another type is [`Error::Invalid`].
    pub fn get<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, Error> {
        match self.0.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => serde_json::from_value(value.clone())
                .map(Some)
                .map_err(|error| Error::Invalid(format!("field `{name}`: {error}"))),
        }
    }

    /// Every field decoded into the plugin's own struct, one field per form
    /// field; a mismatch is [`Error::Invalid`].
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_value(self.0.clone())
            .map_err(|error| Error::Invalid(format!("form values: {error}")))
    }
}

enum Request {
    Render {
        context: String,
    },
    Action {
        action: String,
        values: String,
        context: String,
    },
}

/// One request about a page, waiting for the handlers of that page.
pub struct Pages {
    page: String,
    request: Request,
    answer: Option<Result<String, Error>>,
}

/// What [`Pages::finish`] hands back to the host.
pub struct Paged(Result<String, PluginError>);

impl Paged {
    #[doc(hidden)]
    pub fn into_answer(self) -> Result<String, PluginError> {
        self.0
    }
}

impl Pages {
    #[doc(hidden)]
    pub fn render(page: String, context: String) -> Self {
        Pages {
            page,
            request: Request::Render { context },
            answer: None,
        }
    }

    #[doc(hidden)]
    pub fn action(page: String, action: String, values: String, context: String) -> Self {
        Pages {
            page,
            request: Request::Action {
                action,
                values,
                context,
            },
            answer: None,
        }
    }

    /// The id of the page being asked about.
    pub fn page(&self) -> &str {
        &self.page
    }

    /// Handles `page`: `render` draws it, and `action` answers a click or submit
    /// with the effects to apply in order. `A` is the plugin's own action enum, as
    /// the [`ui`](crate::ui) builders wrote it onto the node. `context.entry` holds
    /// the id of the server or instance on an `entry` page. Render runs each
    /// time the page is shown or refreshed, so keep state in
    /// [`data`](crate::data).
    pub fn on<A: DeserializeOwned>(
        mut self,
        page: &str,
        render: impl FnOnce(PageContext) -> Result<UiNode, Error>,
        action: impl FnOnce(A, Values, PageContext) -> Result<Vec<Effect>, Error>,
    ) -> Self {
        if self.answer.is_some() || self.page != page {
            return self;
        }
        self.answer = Some(match &self.request {
            Request::Render { context } => decode::<PageContext>(context)
                .and_then(render)
                .and_then(|tree| Ok(serde_json::to_string(&tree)?)),
            Request::Action {
                action: wire,
                values,
                context,
            } => decode::<Action>(wire)
                .and_then(|wire| {
                    serde_json::from_value(serde_json::to_value(wire)?)
                        .map_err(|error| Error::Invalid(format!("action on `{page}`: {error}")))
                })
                .and_then(|typed| Ok((typed, decode::<Values>(values)?, decode(context)?)))
                .and_then(|(typed, values, context)| action(typed, values, context))
                .and_then(|effects| Ok(serde_json::to_string(&effects)?)),
        });
        self
    }

    /// Handles a page that only displays: a click on it answers `unsupported`.
    pub fn view(
        self,
        page: &str,
        render: impl FnOnce(PageContext) -> Result<UiNode, Error>,
    ) -> Self {
        let action_page = page.to_string();
        self.on(page, render, move |_: Action, _, _| {
            Err(Error::Unsupported(format!(
                "page `{action_page}` has no actions"
            )))
        })
    }

    /// The answer of the handler that matched, or `unsupported` naming the page.
    pub fn finish(self) -> Paged {
        Paged(match self.answer {
            Some(answer) => answer.map_err(Into::into),
            None => Err(PluginError::Unsupported(format!("page {}", self.page))),
        })
    }
}

fn decode<T: DeserializeOwned>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text).map_err(|error| Error::Invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::ui;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
    enum Act {
        Go,
        Remove(String),
    }

    fn render(page: &str, context: Value) -> Pages {
        Pages::render(page.into(), context.to_string())
    }

    fn act(page: &str, action: Value, values: Value) -> Pages {
        Pages::action(
            page.into(),
            action.to_string(),
            values.to_string(),
            json!({ "entry": "srv-1" }).to_string(),
        )
    }

    fn handled(call: Pages) -> Result<Value, PluginError> {
        call.on(
            "main",
            |context| Ok(ui::text(context.entry.unwrap_or_default())),
            |typed: Act, values: Values, context| {
                Ok(vec![Effect::Navigate {
                    page: format!(
                        "{typed:?}/{}/{}",
                        values.get::<String>("name")?.unwrap_or_default(),
                        context.entry.unwrap_or_default()
                    ),
                }])
            },
        )
        .finish()
        .into_answer()
        .map(|text| serde_json::from_str(&text).unwrap())
    }

    #[test]
    fn a_page_renders_with_its_typed_context() {
        let tree = handled(render("main", json!({ "entry": "srv-1" }))).unwrap();
        assert_eq!(tree["text"], "srv-1");
    }

    #[test]
    fn an_action_arrives_as_the_plugins_enum() {
        let wire = serde_json::to_value(ui::action(Act::Remove("w".into()))).unwrap();
        let effects = handled(act("main", wire, json!({ "name": "x" }))).unwrap();
        assert_eq!(effects[0]["page"], r#"Remove("w")/x/srv-1"#);

        let wire = serde_json::to_value(ui::action(Act::Go)).unwrap();
        assert_eq!(wire, json!({ "id": "go", "payload": null }));
        let effects = handled(act("main", wire, json!({}))).unwrap();
        assert_eq!(effects[0]["page"], "Go//srv-1");
    }

    #[test]
    fn an_action_the_enum_does_not_know_is_invalid() {
        let reply = handled(act("main", json!({ "id": "nope" }), json!({})));
        assert!(matches!(reply, Err(PluginError::Invalid(_))));
    }

    #[test]
    fn an_unrouted_page_is_unsupported_for_both_requests() {
        for call in [
            render("other", json!({})),
            act("other", json!({}), json!({})),
        ] {
            assert!(matches!(
                handled(call),
                Err(PluginError::Unsupported(message)) if message.contains("other")
            ));
        }
    }

    #[test]
    fn a_view_page_refuses_actions() {
        let reply = |call: Pages| {
            call.view("main", |_| Ok(ui::text("hi")))
                .finish()
                .into_answer()
        };
        assert!(reply(render("main", json!({}))).is_ok());
        assert!(matches!(
            reply(act("main", json!({ "id": "x" }), json!({}))),
            Err(PluginError::Unsupported(_))
        ));
    }

    #[test]
    fn values_read_a_field_or_a_whole_form() {
        let values: Values = serde_json::from_value(json!({ "n": 2, "on": null })).unwrap();
        assert_eq!(values.get::<u32>("n").unwrap(), Some(2));
        assert_eq!(values.get::<bool>("on").unwrap(), None);
        assert_eq!(values.get::<bool>("missing").unwrap(), None);
        assert!(matches!(values.get::<String>("n"), Err(Error::Invalid(_))));
    }
}
