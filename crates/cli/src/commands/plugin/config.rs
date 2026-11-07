//! `hestia plugin config`: a plugin's declared settings.

use anyhow::{anyhow, bail, Context, Result};
use client::proto::plugin::{Setting, SettingKind, SettingValue};
use serde_json::Value;

use crate::ui::{self, View};

pub async fn run(
    plugins: client::Plugin<'_>,
    id: &str,
    key: Option<&str>,
    value: Option<&str>,
    unset: bool,
) -> Result<()> {
    let declared = plugins.info(id).await?.manifest.settings;
    let Some(key) = key else {
        return list(&declared, plugins.settings(id).await?);
    };
    let setting = declared
        .iter()
        .find(|setting| setting.key == key)
        .ok_or_else(|| anyhow!("'{id}' declares no setting '{key}'"))?;
    if unset {
        plugins.set_setting(id, key, Value::Null).await?;
        return ui::show(View::line(format!("{key} cleared")));
    }
    let raw = match value {
        Some(raw) => raw.to_string(),
        None if setting.kind == SettingKind::Secret && ui::is_interactive() => {
            ui::prompt_secret(&label(setting))?
        }
        None => {
            let current = plugins.settings(id).await?;
            let current = current
                .iter()
                .find(|current| current.key == key)
                .context("the daemon did not report the setting")?;
            return ui::show(View::line(show(setting, current)));
        }
    };
    plugins.set_setting(id, key, parse(setting, &raw)?).await?;
    ui::show(View::line(format!("{key} saved")))
}

fn list(declared: &[Setting], current: Vec<SettingValue>) -> Result<()> {
    if declared.is_empty() {
        return ui::show(View::note("this plugin has no settings"));
    }
    ui::show(View::table(
        "Settings",
        ["KEY", "VALUE", "DESCRIPTION"],
        declared
            .iter()
            .zip(&current)
            .map(|(setting, current)| {
                vec![
                    setting.key.clone(),
                    show(setting, current),
                    setting.label.clone(),
                ]
            })
            .collect(),
    ))
}

fn label(setting: &Setting) -> String {
    match setting.label.is_empty() {
        true => setting.key.clone(),
        false => setting.label.clone(),
    }
}

fn show(setting: &Setting, current: &SettingValue) -> String {
    match (&current.value, setting.kind) {
        (_, SettingKind::Secret) => match current.set {
            true => "set".into(),
            false => "not set".into(),
        },
        (Some(Value::String(text)), _) => text.clone(),
        (Some(value), _) => value.to_string(),
        (None, _) => "not set".into(),
    }
}

fn parse(setting: &Setting, raw: &str) -> Result<Value> {
    match setting.kind {
        SettingKind::String | SettingKind::Secret | SettingKind::Select => {
            Ok(Value::String(raw.to_string()))
        }
        SettingKind::Number => serde_json::from_str::<serde_json::Number>(raw)
            .map(Value::Number)
            .map_err(|_| anyhow!("'{raw}' is not a number")),
        SettingKind::Bool => match raw {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => bail!("'{raw}' is not true or false"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(kind: SettingKind) -> Setting {
        Setting {
            key: "k".into(),
            kind,
            ..Setting::default()
        }
    }

    #[test]
    fn a_value_parses_by_the_declared_kind() {
        assert_eq!(
            parse(&setting(SettingKind::Number), "4").unwrap(),
            Value::from(4)
        );
        assert!(parse(&setting(SettingKind::Number), "four").is_err());
        assert_eq!(
            parse(&setting(SettingKind::Bool), "true").unwrap(),
            Value::Bool(true)
        );
        assert!(parse(&setting(SettingKind::Bool), "yes").is_err());
    }

    #[test]
    fn a_secret_is_shown_as_set_or_not() {
        let secret = setting(SettingKind::Secret);
        let stored = SettingValue {
            key: "k".into(),
            set: true,
            value: None,
        };
        assert_eq!(show(&secret, &stored), "set");
        assert_eq!(show(&secret, &SettingValue::default()), "not set");
    }
}
