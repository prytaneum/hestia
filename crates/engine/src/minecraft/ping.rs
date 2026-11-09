//! Server List Ping: the status handshake the multiplayer list uses, over the
//! game port. Varint-length-framed packets; the status reply body is a
//! varint-prefixed JSON string.
//!
//! Two callers reach the same exchange: a *managed* server, always on loopback
//! at its allocated port, and an arbitrary entry of an instance's multiplayer
//! list, which is a host the player typed. The host is what the handshake
//! carries, so a virtual-host proxy answers for the right server rather than
//! for whatever the address resolves to.
//!
//! The address is used verbatim: a domain that publishes only a
//! `_minecraft._tcp` SRV record (and no A record on the default port) will not
//! answer here, since resolving one would mean taking on a DNS-record
//! dependency for a status line.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use proto::server::ServerPingResult;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const IO_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PAYLOAD: usize = 256 * 1024;
const STATUS_INTENT: i32 = 1;
const ANY_PROTOCOL: i32 = -1;
const FAVICON_PREFIX: &str = "data:image/png;base64,";
/// The protocol's icon is a 64×64 PNG — around 8 KB base64. The cap is what
/// stops a hostile status reply from inlining a megabyte into every row.
const MAX_FAVICON: usize = 64 * 1024;
pub const DEFAULT_PORT: u16 = 25565;

/// Ping a managed Java server on loopback.
pub async fn ping(port: u16) -> Result<ServerPingResult> {
    status("127.0.0.1", port)
        .await
        .context("cannot reach the server's game port")
}

/// Ping `host` or `host:port` — an entry of a multiplayer list. The port
/// defaults to Minecraft's 25565.
pub async fn ping_address(address: &str) -> Result<ServerPingResult> {
    let (host, port) = split_address(address)?;
    status(&host, port)
        .await
        .with_context(|| format!("cannot reach {address}"))
}

/// Split `host`, `host:port`, or a bracketed IPv6 literal into its parts. An
/// address is user input, so it is validated here rather than trusted to fail
/// later at connect time with a worse message.
pub fn split_address(address: &str) -> Result<(String, u16)> {
    let address = address.trim();
    if address.is_empty() {
        bail!("a server address is required");
    }
    let (host, port) = match address.strip_prefix('[') {
        Some(rest) => {
            let (host, tail) = rest
                .split_once(']')
                .context("unclosed '[' in the address")?;
            (host, tail.strip_prefix(':'))
        }
        // A bare IPv6 literal has several colons and no port; anything with one
        // colon is host:port.
        None if address.matches(':').count() > 1 => (address, None),
        None => match address.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (address, None),
        },
    };
    if host.is_empty() || host.contains(char::is_whitespace) {
        bail!("'{address}' is not a server address");
    }
    let port = match port {
        None | Some("") => DEFAULT_PORT,
        Some(port) => port
            .parse()
            .with_context(|| format!("'{port}' is not a port number"))?,
    };
    Ok((host.to_string(), port))
}

async fn status(host: &str, port: u16) -> Result<ServerPingResult> {
    let mut stream = tokio::time::timeout(IO_TIMEOUT, TcpStream::connect((host, port)))
        .await
        .context("ping connect timed out")??;

    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0x00);
    write_varint(&mut handshake, ANY_PROTOCOL);
    write_string(&mut handshake, host);
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, STATUS_INTENT);
    write_packet(&mut stream, &handshake).await?;
    write_packet(&mut stream, &[0x00]).await?;

    let status = tokio::time::timeout(IO_TIMEOUT, read_status(&mut stream))
        .await
        .context("ping read timed out")??;
    parse_status(&status)
}

fn parse_status(json: &str) -> Result<ServerPingResult> {
    let status: Value = serde_json::from_str(json).context("ping status is not JSON")?;
    let players = |field: &str| {
        status
            .get("players")
            .and_then(|p| p.get(field))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32
    };
    Ok(ServerPingResult {
        players_online: players("online"),
        players_max: players("max"),
        motd: flatten_text(status.get("description").unwrap_or(&Value::Null)),
        version: status
            .get("version")
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        favicon: parse_favicon(&status),
    })
}

/// The status reply's icon, reduced to the bare base64 an entry's cached icon
/// already carries. This is unvalidated input from a host the player typed,
/// and a front-end inlines it into an `img`, so only a PNG data URI whose
/// payload is base64 of a plausible size travels any further.
fn parse_favicon(status: &Value) -> String {
    let Some(encoded) = status
        .get("favicon")
        .and_then(Value::as_str)
        .and_then(|icon| icon.trim().strip_prefix(FAVICON_PREFIX))
    else {
        return String::new();
    };
    // Servers older than 1.13 wrapped the payload in newlines.
    let encoded: String = encoded.chars().filter(|c| !c.is_whitespace()).collect();
    let usable = (1..=MAX_FAVICON).contains(&encoded.len())
        && encoded
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
    if !usable {
        tracing::debug!(bytes = encoded.len(), "ignoring an unusable server favicon");
        return String::new();
    }
    encoded
}

fn flatten_text(component: &Value) -> String {
    fn walk(component: &Value, out: &mut String) {
        match component {
            Value::String(text) => out.push_str(text),
            Value::Array(parts) => parts.iter().for_each(|part| walk(part, out)),
            Value::Object(fields) => {
                if let Some(Value::String(text)) = fields.get("text") {
                    out.push_str(text);
                }
                if let Some(extra) = fields.get("extra") {
                    walk(extra, out);
                }
            }
            _ => {}
        }
    }
    let mut raw = String::new();
    walk(component, &mut raw);
    strip_codes(&raw)
}

fn strip_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

fn write_varint(buf: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

fn write_string(buf: &mut Vec<u8>, text: &str) {
    write_varint(buf, text.len() as i32);
    buf.extend_from_slice(text.as_bytes());
}

async fn write_packet(stream: &mut TcpStream, payload: &[u8]) -> Result<()> {
    let mut frame = Vec::with_capacity(payload.len() + 5);
    write_varint(&mut frame, payload.len() as i32);
    frame.extend_from_slice(payload);
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(&frame))
        .await
        .context("ping write timed out")?
        .context("ping write failed")?;
    Ok(())
}

async fn read_status(stream: &mut TcpStream) -> Result<String> {
    let length = read_varint_stream(stream).await?;
    if !(0..=MAX_PAYLOAD as i32).contains(&length) {
        bail!("malformed ping packet (length {length})");
    }
    let mut payload = vec![0u8; length as usize];
    stream
        .read_exact(&mut payload)
        .await
        .context("ping connection closed mid-packet")?;
    let mut cursor = payload.as_slice();
    let packet_id = read_varint_slice(&mut cursor)?;
    if packet_id != 0 {
        bail!("unexpected ping packet id {packet_id}");
    }
    let text_len = read_varint_slice(&mut cursor)? as usize;
    if text_len > cursor.len() {
        bail!("ping status string overruns its packet");
    }
    Ok(String::from_utf8_lossy(&cursor[..text_len]).into_owned())
}

async fn read_varint_stream(stream: &mut TcpStream) -> Result<i32> {
    let mut value = 0u32;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        stream
            .read_exact(&mut byte)
            .await
            .context("ping connection closed")?;
        value |= u32::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            return Ok(value as i32);
        }
        shift += 7;
        if shift >= 32 {
            bail!("malformed varint");
        }
    }
}

fn read_varint_slice(cursor: &mut &[u8]) -> Result<i32> {
    let mut value = 0u32;
    let mut shift = 0;
    loop {
        let (&byte, rest) = cursor.split_first().context("truncated varint")?;
        *cursor = rest;
        value |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
        shift += 7;
        if shift >= 32 {
            bail!("malformed varint");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_splits_into_host_and_port() {
        assert_eq!(
            split_address("smp.example.net").unwrap(),
            ("smp.example.net".to_string(), DEFAULT_PORT)
        );
        assert_eq!(
            split_address(" smp.example.net:25566 ").unwrap(),
            ("smp.example.net".to_string(), 25566)
        );
        assert_eq!(
            split_address("[::1]:25566").unwrap(),
            ("::1".to_string(), 25566)
        );
        assert_eq!(
            split_address("fe80::1").unwrap(),
            ("fe80::1".to_string(), DEFAULT_PORT)
        );
    }

    #[test]
    fn a_malformed_address_is_refused() {
        for address in ["", "   ", "host:port", "host:99999", "two hosts", "[::1"] {
            assert!(
                split_address(address).is_err(),
                "'{address}' should not parse"
            );
        }
    }

    #[test]
    fn varint_round_trips() {
        for value in [0, 1, 127, 128, 300, 25565, i32::MAX, -1] {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            let mut cursor = buf.as_slice();
            assert_eq!(read_varint_slice(&mut cursor).unwrap(), value);
            assert!(cursor.is_empty());
        }
    }

    #[test]
    fn status_parses_plain_and_component_motds() {
        let plain =
            r#"{"players":{"online":3,"max":20},"description":"hi","version":{"name":"1.21.4"}}"#;
        let parsed = parse_status(plain).unwrap();
        assert_eq!(parsed.players_online, 3);
        assert_eq!(parsed.players_max, 20);
        assert_eq!(parsed.motd, "hi");
        assert_eq!(parsed.version, "1.21.4");

        let component = r#"{"description":{"text":"§aA cozy ","extra":[{"text":"server"}]}}"#;
        assert_eq!(parse_status(component).unwrap().motd, "A cozy server");
    }

    #[test]
    fn a_favicon_arrives_as_bare_base64() {
        let png = r#"{"favicon":"data:image/png;base64,iVBORw0K\nGgo="}"#;
        assert_eq!(parse_status(png).unwrap().favicon, "iVBORw0KGgo=");
    }

    #[test]
    fn a_favicon_that_is_not_an_inlinable_png_is_dropped() {
        let oversized = format!(
            r#"{{"favicon":"{FAVICON_PREFIX}{}"}}"#,
            "A".repeat(MAX_FAVICON + 1)
        );
        for status in [
            r#"{"favicon":""}"#,
            r#"{"favicon":"https://example.net/icon.png"}"#,
            r#"{"favicon":"data:image/svg+xml;base64,PHN2Zz4="}"#,
            &format!(r#"{{"favicon":"{FAVICON_PREFIX}<script>"}}"#),
            &oversized,
        ] {
            assert!(
                parse_status(status).unwrap().favicon.is_empty(),
                "{status} should not carry an icon"
            );
        }
    }
}
