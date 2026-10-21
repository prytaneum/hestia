//! The settings whose spelling or encoding changed between versions, and the
//! ones a machine is allowed to disagree about.

use super::keys;

pub const MODERN: (u64, u64, u64) = (9999, 0, 0);

const KEYBIND_PREFIX: &str = "key_";

#[derive(Clone, Copy, PartialEq)]
enum Form {
    Direct,
    GraphicsBool,
    GraphicsIndex,
    AmbientOcclusionIndex,
}

struct Key {
    name: &'static str,
    since: (u64, u64, u64),
    until: (u64, u64, u64),
    form: Form,
}

struct Setting {
    id: &'static str,
    keys: &'static [Key],
}

const GRAPHICS: &[Key] = &[
    Key {
        name: "fancyGraphics",
        since: (1, 0, 0),
        until: (1, 15, 2),
        form: Form::GraphicsBool,
    },
    Key {
        name: "graphicsMode",
        since: (1, 16, 0),
        until: MODERN,
        form: Form::GraphicsIndex,
    },
];

const AMBIENT_OCCLUSION: &[Key] = &[
    Key {
        name: "ao",
        since: (1, 0, 0),
        until: (1, 19, 2),
        form: Form::AmbientOcclusionIndex,
    },
    Key {
        name: "ao",
        since: (1, 19, 3),
        until: MODERN,
        form: Form::Direct,
    },
];

const SETTINGS: &[Setting] = &[
    Setting {
        id: "graphics",
        keys: GRAPHICS,
    },
    Setting {
        id: "ambient_occlusion",
        keys: AMBIENT_OCCLUSION,
    },
];

/// What a machine is allowed to disagree about: shared only if the user says so.
pub const LOCAL_BY_DEFAULT: &[&str] = &[
    "renderDistance",
    "viewDistance",
    "simulationDistance",
    "entityDistanceScaling",
    "preferredGraphicsBackend",
    "cloudRange",
    "exclusiveFullscreen",
    "macFullscreenMenuVisibility",
    "fpsLimit",
    "mipmapLevels",
    "chunkSectionFadeInTime",
    "textureFiltering",
    "maxAnisotropyBit",
    "weatherRadius",
    "advancedOpengl",
    "anisotropicFiltering",
    "useVbo",
    "rawMouseInput",
    "touchscreen",
    "ctrlClickEmulatesRightClick",
    "quitShortcuts",
];

pub fn decode(key: &str, raw: &str, version: (u64, u64, u64)) -> Option<(String, String)> {
    if let Some(setting) = setting_for(key, version) {
        let (id, form) = setting;
        return from_raw(form, raw).map(|value| (id.to_string(), value));
    }
    if key.starts_with(KEYBIND_PREFIX) {
        return Some((key.to_string(), bind_name(raw)));
    }
    Some((key.to_string(), raw.to_string()))
}

pub fn encode(id: &str, value: &str, version: (u64, u64, u64)) -> Option<(String, String)> {
    if let Some(setting) = SETTINGS.iter().find(|setting| setting.id == id) {
        let key = key_for(setting, version)?;
        return to_raw(key.form, value).map(|raw| (key.name.to_string(), raw));
    }
    if id.starts_with(KEYBIND_PREFIX) {
        return Some((id.to_string(), bind_raw(value, version)));
    }
    Some((id.to_string(), value.to_string()))
}

/// Every spelling a setting has ever had, so pinning one pins the setting.
pub fn spellings(id: &str) -> Vec<&'static str> {
    SETTINGS
        .iter()
        .find(|setting| setting.id == id)
        .map(|setting| setting.keys.iter().map(|key| key.name).collect())
        .unwrap_or_default()
}

fn setting_for(key: &str, version: (u64, u64, u64)) -> Option<(&'static str, Form)> {
    SETTINGS.iter().find_map(|setting| {
        setting
            .keys
            .iter()
            .find(|candidate| {
                candidate.name == key && candidate.since <= version && version <= candidate.until
            })
            .map(|candidate| (setting.id, candidate.form))
    })
}

fn key_for(setting: &'static Setting, version: (u64, u64, u64)) -> Option<&'static Key> {
    setting
        .keys
        .iter()
        .find(|key| key.since <= version && version <= key.until)
}

fn from_raw(form: Form, raw: &str) -> Option<String> {
    let value = match form {
        Form::Direct => raw.to_string(),
        Form::GraphicsBool => match raw {
            "true" => "fancy".to_string(),
            "false" => "fast".to_string(),
            _ => return None,
        },
        Form::GraphicsIndex => match raw {
            "0" => "fast".to_string(),
            "1" => "fancy".to_string(),
            "2" => "fabulous".to_string(),
            _ => return None,
        },
        Form::AmbientOcclusionIndex => match raw {
            "0" => "false".to_string(),
            "1" | "2" => "true".to_string(),
            _ => return None,
        },
    };
    Some(value)
}

fn to_raw(form: Form, value: &str) -> Option<String> {
    let raw = match form {
        Form::Direct => value.to_string(),
        Form::GraphicsBool => match value {
            "fast" => "false".to_string(),
            "fancy" | "fabulous" => "true".to_string(),
            _ => return None,
        },
        Form::GraphicsIndex => match value {
            "fast" => "0".to_string(),
            "fancy" => "1".to_string(),
            "fabulous" => "2".to_string(),
            _ => return None,
        },
        Form::AmbientOcclusionIndex => match value {
            "false" => "0".to_string(),
            "true" => "2".to_string(),
            _ => return None,
        },
    };
    Some(raw)
}

fn bind_name(raw: &str) -> String {
    raw.parse::<i64>()
        .ok()
        .and_then(keys::name)
        .map(str::to_string)
        .unwrap_or_else(|| raw.to_string())
}

const KEY_NAMES_SINCE: (u64, u64, u64) = (1, 13, 0);

fn bind_raw(value: &str, version: (u64, u64, u64)) -> String {
    if version >= KEY_NAMES_SINCE {
        return value.to_string();
    }
    keys::code(value)
        .map(|code| code.to_string())
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renamed_setting_is_one_setting() {
        let old = decode("fancyGraphics", "true", (1, 12, 2)).unwrap();
        let new = decode("graphicsMode", "1", (1, 21, 4)).unwrap();
        assert_eq!(old, new);
        assert_eq!(old.1, "fancy");
    }

    #[test]
    fn a_value_is_written_in_the_targets_own_spelling() {
        assert_eq!(
            encode("graphics", "fabulous", (1, 21, 4)),
            Some(("graphicsMode".to_string(), "2".to_string()))
        );
        assert_eq!(
            encode("graphics", "fabulous", (1, 12, 2)),
            Some(("fancyGraphics".to_string(), "true".to_string()))
        );
    }

    #[test]
    fn a_keybind_crosses_the_1_13_respelling_both_ways() {
        let legacy = decode("key_key.forward", "17", (1, 12, 2)).unwrap();
        assert_eq!(legacy.1, "key.keyboard.w");
        assert_eq!(
            encode("key_key.forward", "key.keyboard.w", (1, 12, 2)),
            Some(("key_key.forward".to_string(), "17".to_string()))
        );
        assert_eq!(
            encode("key_key.forward", "key.keyboard.w", (1, 21, 4)),
            Some(("key_key.forward".to_string(), "key.keyboard.w".to_string()))
        );
    }

    #[test]
    fn ambient_occlusion_survives_becoming_a_boolean() {
        assert_eq!(decode("ao", "2", (1, 18, 2)).unwrap().1, "true");
        assert_eq!(decode("ao", "true", (1, 21, 4)).unwrap().1, "true");
        assert_eq!(
            encode("ambient_occlusion", "true", (1, 18, 2)),
            Some(("ao".to_string(), "2".to_string()))
        );
    }
}
