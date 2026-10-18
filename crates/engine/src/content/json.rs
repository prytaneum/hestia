use proto::content::SideSupport;
use serde_json::Value;

pub(super) fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// CurseForge serialises some counters as floats.
pub(super) fn u64_field(v: &Value, key: &str) -> u64 {
    let Some(value) = v.get(key) else {
        return 0;
    };
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|n| n as u64))
        .unwrap_or_default()
}

pub(super) fn non_empty(opt: &Option<String>) -> Option<&str> {
    opt.as_deref().filter(|s| !s.is_empty())
}

pub(super) fn parse_side(s: &str) -> SideSupport {
    match s {
        "required" => SideSupport::Required,
        "optional" => SideSupport::Optional,
        "unsupported" => SideSupport::Unsupported,
        _ => SideSupport::Unknown,
    }
}
