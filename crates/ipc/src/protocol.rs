//! The daemon protocol envelope, layered on the raw frame transport. Both sides
//! encode/decode through here, so the wire format lives in exactly one place.

use serde_json::{json, Value};
use thiserror::Error;

/// The protocol major version carried by every envelope. Bump on a breaking wire
/// change; additive fields do not need a bump.
pub const PROTOCOL_VERSION: i64 = 1;

/// Why an inbound envelope could not be turned into a request or response. The
/// seam fails **closed**: a missing or foreign-major `v` refuses to decode
/// rather than defaulting to the current version, so no caller can silently
/// consume a frame it does not understand. The type carries the invariant —
/// `decode_version()` is not an opt-in check a decode site can forget.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("malformed envelope: {0}")]
    Malformed(String),
    #[error("incompatible protocol version: got {got}, expected {want}")]
    IncompatibleVersion { got: i64, want: i64 },
}

/// Validate an envelope's major version. A missing `v` is malformed (never an
/// implicit "current"), and a foreign major is refused.
fn decode_version(frame: &Value) -> Result<(), DecodeError> {
    let version = frame
        .get("v")
        .and_then(Value::as_i64)
        .ok_or_else(|| DecodeError::Malformed("missing protocol version".into()))?;
    if version != PROTOCOL_VERSION {
        return Err(DecodeError::IncompatibleVersion {
            got: version,
            want: PROTOCOL_VERSION,
        });
    }
    Ok(())
}

/// A request: a channel name, a JSON payload, and an optional correlation id.
#[derive(Debug, Clone)]
pub struct Request {
    pub channel: String,
    pub payload: Value,
    pub id: Option<i64>,
}

impl Request {
    pub fn new(channel: impl Into<String>, payload: Value, id: Option<i64>) -> Self {
        Request {
            channel: channel.into(),
            payload,
            id,
        }
    }
}

/// A response: success carries a payload; failure carries the daemon's
/// structured error (a serialized `proto::error::ErrorInfo`) as opaque JSON —
/// `ipc` stays domain-free and never interprets it.
#[derive(Debug, Clone)]
pub struct Response {
    pub ok: bool,
    pub payload: Value,
    pub error: Option<Value>,
    pub id: Option<i64>,
}

impl Response {
    pub fn success(payload: Value) -> Self {
        Response {
            ok: true,
            payload,
            error: None,
            id: None,
        }
    }

    pub fn failure(error: Value) -> Self {
        Response {
            ok: false,
            payload: Value::Null,
            error: Some(error),
            id: None,
        }
    }
}

/// An unsolicited push from the daemon to a subscribed client. It carries no id.
#[derive(Debug, Clone)]
pub struct Event {
    pub topic: String,
    pub payload: Value,
}

pub fn encode_request(req: &Request) -> String {
    let mut j = json!({
        "v": PROTOCOL_VERSION,
        "channel": req.channel,
        "payload": req.payload,
    });
    if let Some(id) = req.id {
        j["id"] = json!(id);
    }
    j.to_string()
}

pub fn encode_response(res: &Response) -> String {
    let mut j = json!({ "v": PROTOCOL_VERSION, "ok": res.ok });
    if res.ok {
        j["payload"] = res.payload.clone();
    } else {
        j["error"] = res.error.clone().unwrap_or_else(|| json!({}));
    }
    if let Some(id) = res.id {
        j["id"] = json!(id);
    }
    j.to_string()
}

pub fn encode_event(event: &Event) -> String {
    json!({ "event": event.topic, "payload": event.payload }).to_string()
}

pub fn decode_request(frame: &str) -> Result<Request, DecodeError> {
    let j: Value =
        serde_json::from_str(frame).map_err(|e| DecodeError::Malformed(e.to_string()))?;
    decode_version(&j)?;
    let channel = j
        .get("channel")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let payload = match j.get("payload") {
        Some(p) if !p.is_null() => p.clone(),
        _ => json!({}),
    };
    let id = j.get("id").and_then(Value::as_i64);
    Ok(Request {
        channel,
        payload,
        id,
    })
}

/// Is this frame an event (an unsolicited push) rather than a response?
pub fn is_event(frame: &Value) -> bool {
    frame.get("event").map(Value::is_string).unwrap_or(false)
}

pub fn decode_response(frame: &Value) -> Result<Response, DecodeError> {
    decode_version(frame)?;
    let ok = frame.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let id = frame.get("id").and_then(Value::as_i64);
    if ok {
        Ok(Response {
            ok: true,
            payload: frame.get("payload").cloned().unwrap_or(json!({})),
            error: None,
            id,
        })
    } else {
        Ok(Response {
            ok: false,
            payload: Value::Null,
            error: Some(frame.get("error").cloned().unwrap_or_else(|| json!({}))),
            id,
        })
    }
}

pub fn decode_event(frame: &Value) -> Event {
    Event {
        topic: frame
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        payload: match frame.get("payload") {
            Some(p) if !p.is_null() => p.clone(),
            _ => json!({}),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_at_current_version() {
        let frame = encode_request(&Request::new("health.ping", json!({}), Some(7)));
        let req = decode_request(&frame).expect("current-major request decodes");
        assert_eq!(req.channel, "health.ping");
        assert_eq!(req.id, Some(7));
    }

    #[test]
    fn request_with_foreign_major_is_refused() {
        for v in [0, 99, PROTOCOL_VERSION + 1] {
            let frame = json!({ "v": v, "channel": "health.ping", "payload": {} }).to_string();
            match decode_request(&frame) {
                Err(DecodeError::IncompatibleVersion { got, want }) => {
                    assert_eq!(got, v);
                    assert_eq!(want, PROTOCOL_VERSION);
                }
                other => panic!("expected version rejection for v={v}, got {other:?}"),
            }
        }
    }

    #[test]
    fn request_without_version_is_malformed() {
        let frame = json!({ "channel": "health.ping", "payload": {} }).to_string();
        assert!(matches!(
            decode_request(&frame),
            Err(DecodeError::Malformed(_))
        ));
    }

    #[test]
    fn response_with_foreign_major_is_refused() {
        let frame = json!({ "v": 99, "ok": true, "payload": {} });
        assert!(matches!(
            decode_response(&frame),
            Err(DecodeError::IncompatibleVersion { got: 99, .. })
        ));
    }
}
