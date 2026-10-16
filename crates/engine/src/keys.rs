use proto::error::{ErrorInfo, Reason};
use proto::keys::{KeyKind, KeyRule};

use crate::backup::parse_interval;
use crate::minecraft::launch::{normalize_memory, parse_jvm_args};

pub(crate) fn rule(key: &str, kind: KeyKind) -> KeyRule {
    KeyRule {
        key: key.to_string(),
        kind,
    }
}

pub(crate) fn invalid(key: &str) -> impl FnOnce(Reason) -> ErrorInfo + '_ {
    move |reason| ErrorInfo::ConfigInvalid {
        key: key.to_string(),
        reason,
    }
}

pub(crate) fn validate(kind: &KeyKind, value: &str) -> Result<(), Reason> {
    match kind {
        KeyKind::Boolean => match value.to_ascii_lowercase().as_str() {
            "true" | "false" => Ok(()),
            _ => Err(Reason::NotBoolean),
        },
        KeyKind::Integer { min, max } => {
            let number: i64 = value.parse().map_err(|_| Reason::NotWholeNumber)?;
            if (*min..=*max).contains(&number) {
                Ok(())
            } else {
                Err(Reason::OutOfRange {
                    min: *min,
                    max: *max,
                })
            }
        }
        KeyKind::Choice { values } => {
            if values.iter().any(|v| v == value) {
                Ok(())
            } else {
                Err(Reason::NotOneOf {
                    values: values.clone(),
                })
            }
        }
        KeyKind::Memory => normalize_memory(value).map(drop),
        KeyKind::Interval => parse_interval(&value.to_ascii_lowercase()).map(drop),
        KeyKind::JvmArgs => parse_jvm_args(value).map(drop),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_report_the_kind_of_mistake() {
        let kind = KeyKind::Integer { min: 3, max: 32 };
        assert_eq!(validate(&kind, "10"), Ok(()));
        assert_eq!(validate(&kind, "abc"), Err(Reason::NotWholeNumber));
        assert_eq!(validate(&kind, "1.5"), Err(Reason::NotWholeNumber));
        assert_eq!(
            validate(&kind, "900"),
            Err(Reason::OutOfRange { min: 3, max: 32 })
        );
    }

    #[test]
    fn booleans_ignore_case_and_choices_do_not() {
        assert_eq!(validate(&KeyKind::Boolean, "TRUE"), Ok(()));
        assert_eq!(validate(&KeyKind::Boolean, "yes"), Err(Reason::NotBoolean));
        let choice = KeyKind::Choice {
            values: vec!["easy".into(), "hard".into()],
        };
        assert_eq!(validate(&choice, "hard"), Ok(()));
        assert!(matches!(
            validate(&choice, "Hard"),
            Err(Reason::NotOneOf { .. })
        ));
    }
}
