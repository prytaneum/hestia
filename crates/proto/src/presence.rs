//! The status card Discord shows, as a plugin sets it
//! ([0106](../../../docs/decisions/0106-presence-is-a-host-service.md)).

use serde::{Deserialize, Serialize};

pub const MAX_BUTTONS: usize = 2;
pub const MIN_TEXT_CHARS: usize = 2;
pub const MAX_TEXT_CHARS: usize = 128;
pub const MAX_LABEL_CHARS: usize = 32;
pub const MAX_URL_CHARS: usize = 512;
pub const MAX_IMAGE_KEY_CHARS: usize = 256;

/// A link under the card. The URL is `https`.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Button {
    pub label: String,
    pub url: String,
}

/// What Discord shows. Image keys name assets uploaded to Hestia's Discord
/// application; `start` is a Unix timestamp in seconds for the elapsed timer.
/// [`Activity::checked`] enforces Discord's documented limits.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Activity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub large_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_text: Option<String>,
    pub buttons: Vec<Button>,
}

impl Activity {
    /// The card with every text trimmed and an empty one omitted, or the first
    /// limit it breaks. Nothing is truncated.
    pub fn checked(mut self) -> Result<Activity, String> {
        self.details = bounded("details", self.details, MIN_TEXT_CHARS, MAX_TEXT_CHARS)?;
        self.state = bounded("state", self.state, MIN_TEXT_CHARS, MAX_TEXT_CHARS)?;
        self.large_text = bounded("largeText", self.large_text, MIN_TEXT_CHARS, MAX_TEXT_CHARS)?;
        self.small_text = bounded("smallText", self.small_text, MIN_TEXT_CHARS, MAX_TEXT_CHARS)?;
        self.large_image = bounded("largeImage", self.large_image, 1, MAX_IMAGE_KEY_CHARS)?;
        self.small_image = bounded("smallImage", self.small_image, 1, MAX_IMAGE_KEY_CHARS)?;
        if self.buttons.len() > MAX_BUTTONS {
            return Err(format!("at most {MAX_BUTTONS} buttons"));
        }
        for button in &mut self.buttons {
            button.label = button.label.trim().to_string();
            let length = button.label.chars().count();
            if length == 0 || length > MAX_LABEL_CHARS {
                return Err(format!(
                    "a button label is 1 to {MAX_LABEL_CHARS} characters"
                ));
            }
            if !button.url.starts_with("https://") || button.url.chars().count() > MAX_URL_CHARS {
                return Err(format!(
                    "a button url is https and at most {MAX_URL_CHARS} characters"
                ));
            }
        }
        Ok(self)
    }
}

fn bounded(
    field: &str,
    value: Option<String>,
    min: usize,
    max: usize,
) -> Result<Option<String>, String> {
    let Some(value) = value.map(|value| value.trim().to_string()) else {
        return Ok(None);
    };
    match value.chars().count() {
        0 => Ok(None),
        length if (min..=max).contains(&length) => Ok(Some(value)),
        _ => Err(format!("{field} is {min} to {max} characters")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(chars: usize) -> Option<String> {
        Some("a".repeat(chars))
    }

    fn button(label: &str, url: &str) -> Button {
        Button {
            label: label.into(),
            url: url.into(),
        }
    }

    #[test]
    fn text_is_trimmed_and_empty_is_omitted() {
        let card = Activity {
            details: Some("  Playing  ".into()),
            state: Some("   ".into()),
            ..Activity::default()
        }
        .checked()
        .unwrap();
        assert_eq!(card.details.as_deref(), Some("Playing"));
        assert_eq!(card.state, None);
    }

    #[test]
    fn text_outside_two_to_128_characters_is_refused() {
        type Build = fn(Option<String>) -> Activity;
        let fields: [(&str, Build); 4] = [
            ("details", |v| Activity {
                details: v,
                ..Activity::default()
            }),
            ("state", |v| Activity {
                state: v,
                ..Activity::default()
            }),
            ("largeText", |v| Activity {
                large_text: v,
                ..Activity::default()
            }),
            ("smallText", |v| Activity {
                small_text: v,
                ..Activity::default()
            }),
        ];
        for (field, build) in fields {
            assert!(build(long(1)).checked().is_err(), "{field} 1");
            assert!(build(long(2)).checked().is_ok(), "{field} 2");
            assert!(build(long(128)).checked().is_ok(), "{field} 128");
            assert!(build(long(129)).checked().is_err(), "{field} 129");
        }
    }

    #[test]
    fn an_image_key_is_capped() {
        let card = |large| Activity {
            large_image: large,
            ..Activity::default()
        };
        assert!(card(long(256)).checked().is_ok());
        assert!(card(long(257)).checked().is_err());
    }

    #[test]
    fn at_most_two_buttons() {
        let ok = button("Join", "https://example.com");
        let card = |count| Activity {
            buttons: vec![ok.clone(); count],
            ..Activity::default()
        };
        assert!(card(2).checked().is_ok());
        assert!(card(3).checked().is_err());
    }

    #[test]
    fn a_button_label_and_url_are_bounded() {
        let card = |label: &str, url: &str| Activity {
            buttons: vec![button(label, url)],
            ..Activity::default()
        };
        assert!(card(&"a".repeat(32), "https://x.io").checked().is_ok());
        assert!(card(&"a".repeat(33), "https://x.io").checked().is_err());
        assert!(card("  ", "https://x.io").checked().is_err());
        assert!(card("a", "http://x.io").checked().is_err());
        assert!(card("a", "javascript:alert(1)").checked().is_err());
        let url = |n: usize| format!("https://{}", "a".repeat(n - 8));
        assert!(card("a", &url(512)).checked().is_ok());
        assert!(card("a", &url(513)).checked().is_err());
    }
}
