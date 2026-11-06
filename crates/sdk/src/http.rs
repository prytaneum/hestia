//! Outbound HTTPS to the hosts the manifest grants, over `wasi:http`.
//!
//! The plugin needs the `network:connect` permission and lists each host in
//! `network`. Only `https://` URLs to named hosts are allowed. A refused
//! request is [`Error::PermissionDenied`]; the host does not follow redirects,
//! so a `3xx` answer comes back as it is and the plugin decides whether to
//! request the `Location` (which must be granted too).
//!
//! ```no_run
//! let reply = hestia::http::get("https://api.example.com/v1/status")?;
//! if reply.status == 200 {
//!     let status: hestia::Value = reply.json()?;
//! }
//! # Ok::<(), hestia::Error>(())
//! ```

use serde::de::DeserializeOwned;

use crate::Error;

mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "http-client",
        generate_all,
    });
}

use bindings::wasi::http::outgoing_handler;
use bindings::wasi::http::types::{
    ErrorCode, Fields, IncomingBody, Method, OutgoingBody, OutgoingRequest, Scheme,
};
use bindings::wasi::io::streams::StreamError;

const WRITE_CHUNK: usize = 4096;
const READ_CHUNK: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> Result<&str, Error> {
        std::str::from_utf8(&self.body)
            .map_err(|error| Error::Invalid(format!("the body is not UTF-8: {error}")))
    }

    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        Ok(serde_json::from_slice(&self.body)?)
    }
}

pub fn get(url: &str) -> Result<Response, Error> {
    send(Method::Get, url, None)
}

pub fn post(url: &str, content_type: &str, body: &[u8]) -> Result<Response, Error> {
    send(Method::Post, url, Some((content_type, body)))
}

fn send(method: Method, url: &str, body: Option<(&str, &[u8])>) -> Result<Response, Error> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| Error::Invalid("only https:// URLs can be requested".into()))?;
    let rest = rest.split('#').next().unwrap_or_default();
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(split);

    let headers = Fields::new();
    if let Some((content_type, _)) = body {
        headers
            .append("content-type", content_type.as_bytes())
            .map_err(|_| Error::Invalid("the content type is not a valid header value".into()))?;
    }
    let request = OutgoingRequest::new(headers);
    let invalid = |_| Error::Invalid("the URL is not valid".into());
    request.set_method(&method).map_err(invalid)?;
    request.set_scheme(Some(&Scheme::Https)).map_err(invalid)?;
    request.set_authority(Some(authority)).map_err(invalid)?;
    request
        .set_path_with_query(Some(if path.is_empty() { "/" } else { path }))
        .map_err(invalid)?;

    let outgoing = request
        .body()
        .map_err(|()| Error::Failed("no request body".into()))?;
    let pending = outgoing_handler::handle(request, None).map_err(refusal)?;
    if let Some((_, bytes)) = body {
        write_body(&outgoing, bytes)?;
    }
    OutgoingBody::finish(outgoing, None).map_err(refusal)?;

    pending.subscribe().block();
    let reply = pending
        .get()
        .ok_or_else(|| Error::Failed("the response never arrived".into()))?
        .map_err(|()| Error::Failed("the response was already taken".into()))?
        .map_err(refusal)?;
    let status = reply.status();
    let incoming = reply
        .consume()
        .map_err(|()| Error::Failed("no response body".into()))?;
    Ok(Response {
        status,
        body: read_body(&incoming)?,
    })
}

fn write_body(outgoing: &OutgoingBody, bytes: &[u8]) -> Result<(), Error> {
    let stream = outgoing
        .write()
        .map_err(|()| Error::Failed("no request body stream".into()))?;
    for chunk in bytes.chunks(WRITE_CHUNK) {
        stream
            .blocking_write_and_flush(chunk)
            .map_err(|error| Error::Failed(format!("writing the request body: {error:?}")))?;
    }
    Ok(())
}

fn read_body(incoming: &IncomingBody) -> Result<Vec<u8>, Error> {
    let stream = incoming
        .stream()
        .map_err(|()| Error::Failed("no response body stream".into()))?;
    let mut body = Vec::new();
    loop {
        match stream.blocking_read(READ_CHUNK) {
            Ok(chunk) => body.extend_from_slice(&chunk),
            Err(StreamError::Closed) => return Ok(body),
            Err(StreamError::LastOperationFailed(error)) => {
                return Err(Error::Failed(format!(
                    "reading the response body: {}",
                    error.to_debug_string()
                )))
            }
        }
    }
}

fn refusal(code: ErrorCode) -> Error {
    match code {
        ErrorCode::HttpRequestDenied | ErrorCode::DestinationIpProhibited => {
            Error::PermissionDenied(format!("http request refused: {code:?}"))
        }
        other => Error::Failed(format!("http request failed: {other:?}")),
    }
}
