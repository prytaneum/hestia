//! `hestia <plugin> <page>`: draws a page a plugin contributes and drives its
//! actions. Interactive, the page loops: pick an action, fill its form, apply
//! what the plugin answers. Otherwise it prints the first render and stops;
//! `--action <id>` with `--set name=value` runs one action instead.

use anyhow::{anyhow, bail, Result};
use client::proto::error::EntryKind;
use client::proto::plugin::{PageContext, PageSlot, PluginInfo, PluginUiActionParams, PluginUiRenderParams};
use client::proto::ui::{Effect, FormField, Tone, UiNode};
use client::Client;
use serde_json::{Map, Number, Value};
use tracing::debug;

use crate::ui::plugin::{choices, views, Choice};
use crate::ui::{self, View};

#[derive(Default)]
struct Options {
    entry: Option<String>,
    action: Option<String>,
    values: Vec<(String, String)>,
}

pub fn is_page(plugin: &PluginInfo, name: &str) -> bool {
    plugin
        .manifest
        .contributes
        .pages
        .iter()
        .any(|page| page.id == name)
}

pub async fn open(client: &Client, plugin: &PluginInfo, page: &str, argv: &[String]) -> Result<()> {
    let options = parse(argv)?;
    let id = plugin.manifest.id.as_str();
    let contribution = plugin
        .manifest
        .contributes
        .pages
        .iter()
        .find(|p| p.id == page)
        .ok_or_else(|| anyhow!("'{id}' has no page '{page}'"))?;
    let entry = match (contribution.slot, contribution.kind, options.entry.as_deref()) {
        (PageSlot::Entry, Some(kind), Some(given)) => {
            Some(entry_id(client, kind, given).await?)
        }
        (PageSlot::Entry, _, _) => {
            bail!("'{page}' is a page of a server or instance; pass --entry <name>")
        }
        _ => None,
    };
    let session = Session {
        client,
        plugin: id,
        context: PageContext { entry },
    };
    let mut page = page.to_string();
    let mut tree = session.render(&page).await?;
    show_tree(&tree)?;
    let mut scripted = options.action.as_deref();
    loop {
        let available = choices(&tree);
        let is_scripted = scripted.is_some();
        let choice = match scripted.take() {
            Some(action) => available
                .iter()
                .find(|c| c.action.id == action)
                .cloned()
                .ok_or_else(|| {
                    let ids: Vec<&str> = available.iter().map(|c| c.action.id.as_str()).collect();
                    anyhow!("no action '{action}' on this page; it offers: {}", ids.join(", "))
                })?,
            None if available.is_empty() || !ui::is_interactive() => return Ok(()),
            None => {
                let labels: Vec<String> = available.iter().map(|c| c.label.clone()).collect();
                match ui::select("action", &labels) {
                    Ok(index) => available[index].clone(),
                    Err(_) => return Ok(()),
                }
            }
        };
        let values = if is_scripted {
            scripted_values(client, &choice.fields, &options.values).await?
        } else {
            prompted_values(client, &choice.fields).await?
        };
        let effects = session.act(&page, &choice, values).await?;
        debug!(plugin = id, page, action = %choice.action.id, effects = effects.len(), "page action");
        let mut redraw = false;
        for effect in effects {
            match effect {
                Effect::Render { tree: next } => {
                    tree = next;
                    redraw = true;
                }
                Effect::Refresh => {
                    tree = session.render(&page).await?;
                    redraw = true;
                }
                Effect::Toast { tone, text } => ui::show(match tone {
                    Tone::Warning | Tone::Error => View::warning(text),
                    _ => View::note(text),
                })?,
                Effect::Navigate { page: target } => {
                    if !is_page(plugin, &target) {
                        ui::show(View::note(format!(
                            "opens '{target}', which is not a page of '{id}'"
                        )))?;
                        return Ok(());
                    }
                    page = target;
                    tree = session.render(&page).await?;
                    redraw = true;
                }
            }
        }
        if redraw {
            show_tree(&tree)?;
        }
        if options.action.is_some() {
            return Ok(());
        }
    }
}

fn show_tree(tree: &UiNode) -> Result<()> {
    views(tree).into_iter().try_for_each(ui::show)
}

struct Session<'a> {
    client: &'a Client,
    plugin: &'a str,
    context: PageContext,
}

impl Session<'_> {
    async fn render(&self, page: &str) -> Result<UiNode> {
        Ok(self
            .client
            .plugin()
            .render(&PluginUiRenderParams {
                plugin: self.plugin.to_string(),
                page: page.to_string(),
                context: self.context.clone(),
            })
            .await?)
    }

    async fn act(&self, page: &str, choice: &Choice, values: Value) -> Result<Vec<Effect>> {
        Ok(self
            .client
            .plugin()
            .action(&PluginUiActionParams {
                plugin: self.plugin.to_string(),
                page: page.to_string(),
                action: choice.action.clone(),
                values,
                context: self.context.clone(),
            })
            .await?)
    }
}

fn parse(argv: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut tokens = argv.iter();
    while let Some(token) = tokens.next() {
        let Some(flag) = token.strip_prefix("--") else {
            bail!("unexpected '{token}'; a page takes --entry, --action and --set");
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (flag, None),
        };
        let value = inline
            .or_else(|| tokens.next().cloned())
            .ok_or_else(|| anyhow!("--{name} needs a value"))?;
        match name {
            "entry" => options.entry = Some(value),
            "action" => options.action = Some(value),
            "set" => {
                let (key, text) = value
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--set takes name=value, not '{value}'"))?;
                options.values.push((key.to_string(), text.to_string()));
            }
            other => bail!("unknown argument --{other}; a page takes --entry, --action and --set"),
        }
    }
    Ok(options)
}

fn name(field: &FormField) -> &str {
    match field {
        FormField::Text { name, .. }
        | FormField::Number { name, .. }
        | FormField::Toggle { name, .. }
        | FormField::Select { name, .. }
        | FormField::Entry { name, .. } => name,
    }
}

fn default(field: &FormField) -> Option<Value> {
    match field {
        FormField::Text { value, .. } | FormField::Select { value, .. } => Some(Value::from(value.clone())),
        FormField::Number { value, .. } => Number::from_f64(*value).map(Value::Number),
        FormField::Toggle { value, .. } => Some(Value::Bool(*value)),
        FormField::Entry { .. } => None,
    }
}

fn coerce(field: &FormField, raw: &str) -> Result<Value> {
    match field {
        FormField::Number { name, .. } => raw
            .parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| anyhow!("{name} takes a number, not '{raw}'")),
        FormField::Toggle { name, .. } => raw
            .parse()
            .map(Value::Bool)
            .map_err(|_| anyhow!("{name} takes true or false, not '{raw}'")),
        _ => Ok(Value::from(raw.to_string())),
    }
}

async fn scripted_values(
    client: &Client,
    fields: &[FormField],
    given: &[(String, String)],
) -> Result<Value> {
    let mut values = Map::new();
    for field in fields {
        let key = name(field);
        let raw = given.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v);
        let value = match (raw, field) {
            (Some(raw), FormField::Entry { kind, .. }) => Value::from(entry_id(client, *kind, raw).await?),
            (Some(raw), field) => coerce(field, raw)?,
            (None, field) => {
                default(field).ok_or_else(|| anyhow!("pass a value with --set {key}=<name>"))?
            }
        };
        values.insert(key.to_string(), value);
    }
    Ok(Value::Object(values))
}

async fn prompted_values(client: &Client, fields: &[FormField]) -> Result<Value> {
    let mut values = Map::new();
    for field in fields {
        let value = match field {
            FormField::Text { label, value, .. } => Value::from(ui::input(label, value)?),
            FormField::Number { label, value, .. } => {
                coerce(field, &ui::input(label, &value.to_string())?)?
            }
            FormField::Toggle { label, value, .. } => {
                let items = if *value { ["yes", "no"] } else { ["no", "yes"] }.map(String::from);
                Value::Bool(items[ui::select(label, &items)?] == "yes")
            }
            FormField::Select { label, options, .. } => {
                let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
                Value::from(options[ui::select(label, &labels)?].value.clone())
            }
            FormField::Entry { label, kind, .. } => {
                let entries = entries(client, *kind).await?;
                let names: Vec<String> = entries.iter().map(|(_, name)| name.clone()).collect();
                Value::from(entries[ui::select(label, &names)?].0.clone())
            }
        };
        values.insert(name(field).to_string(), value);
    }
    Ok(Value::Object(values))
}

async fn entries(client: &Client, kind: EntryKind) -> Result<Vec<(String, String)>> {
    Ok(match kind {
        EntryKind::Server => client
            .server()
            .list()
            .await?
            .into_iter()
            .map(|s| (s.id, s.name))
            .collect(),
        EntryKind::Instance => client
            .instance()
            .list()
            .await?
            .into_iter()
            .map(|i| (i.id, i.name))
            .collect(),
    })
}

async fn entry_id(client: &Client, kind: EntryKind, given: &str) -> Result<String> {
    entries(client, kind)
        .await?
        .into_iter()
        .find(|(id, name)| id == given || name == given)
        .map(|(id, _)| id)
        .ok_or_else(|| anyhow!("no {kind} '{given}'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn flags_collect_the_entry_action_and_every_set() {
        let options = parse(&argv(&[
            "--entry", "smp", "--action=save", "--set", "a=1", "--set=b=x=y",
        ]))
        .unwrap();
        assert_eq!(options.entry.as_deref(), Some("smp"));
        assert_eq!(options.action.as_deref(), Some("save"));
        assert_eq!(
            options.values,
            [("a".into(), "1".into()), ("b".into(), "x=y".into())]
        );
    }

    #[test]
    fn a_stray_word_an_unknown_flag_and_a_bad_set_are_refused() {
        assert!(parse(&argv(&["stray"])).is_err());
        assert!(parse(&argv(&["--nope", "x"])).is_err());
        assert!(parse(&argv(&["--set", "novalue"])).is_err());
        assert!(parse(&argv(&["--action"])).is_err());
    }

    #[test]
    fn values_are_typed_by_their_field() {
        let number = FormField::Number {
            name: "n".into(),
            label: String::new(),
            value: 0.0,
        };
        assert_eq!(coerce(&number, "2.5").unwrap(), serde_json::json!(2.5));
        assert!(coerce(&number, "x").is_err());
        let toggle = FormField::Toggle {
            name: "t".into(),
            label: String::new(),
            value: false,
        };
        assert_eq!(coerce(&toggle, "true").unwrap(), Value::Bool(true));
        assert!(coerce(&toggle, "maybe").is_err());
    }
}
