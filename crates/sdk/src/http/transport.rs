use super::request::Method;
use super::{Headers, Request, Response};
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
    ErrorCode, Fields, IncomingBody, Method as WasiMethod, OutgoingBody, OutgoingRequest, Scheme,
};
use bindings::wasi::io::streams::StreamError;

const WRITE_CHUNK: usize = 4096;
const READ_CHUNK: u64 = 64 * 1024;

pub(super) fn send(request: Request) -> Result<Response, Error> {
    let Request {
        method,
        url,
        headers,
        body,
    } = request;
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| Error::Invalid("only https:// URLs can be requested".into()))?;
    let rest = rest.split('#').next().unwrap_or_default();
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(split);

    let fields = Fields::from_list(
        &headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
            .collect::<Vec<_>>(),
    )
    .map_err(|error| Error::Invalid(format!("a header is not valid: {error:?}")))?;
    let outgoing_request = OutgoingRequest::new(fields);
    let invalid = |_| Error::Invalid("the URL is not valid".into());
    outgoing_request
        .set_method(&wasi_method(method))
        .map_err(invalid)?;
    outgoing_request
        .set_scheme(Some(&Scheme::Https))
        .map_err(invalid)?;
    outgoing_request
        .set_authority(Some(authority))
        .map_err(invalid)?;
    outgoing_request
        .set_path_with_query(Some(if path.is_empty() { "/" } else { path }))
        .map_err(invalid)?;

    let outgoing = outgoing_request
        .body()
        .map_err(|()| Error::Failed("no request body".into()))?;
    let pending = outgoing_handler::handle(outgoing_request, None).map_err(refusal)?;
    if !body.is_empty() {
        write_body(&outgoing, &body)?;
    }
    OutgoingBody::finish(outgoing, None).map_err(refusal)?;

    pending.subscribe().block();
    let reply = pending
        .get()
        .ok_or_else(|| Error::Failed("the response never arrived".into()))?
        .map_err(|()| Error::Failed("the response was already taken".into()))?
        .map_err(refusal)?;
    let status = reply.status();
    let headers: Headers = reply
        .headers()
        .entries()
        .into_iter()
        .map(|(name, value)| (name, String::from_utf8_lossy(&value).into_owned()))
        .collect();
    let incoming = reply
        .consume()
        .map_err(|()| Error::Failed("no response body".into()))?;
    Ok(Response {
        status,
        headers,
        body: read_body(&incoming)?,
    })
}

fn wasi_method(method: Method) -> WasiMethod {
    match method {
        Method::Get => WasiMethod::Get,
        Method::Head => WasiMethod::Head,
        Method::Post => WasiMethod::Post,
        Method::Put => WasiMethod::Put,
        Method::Patch => WasiMethod::Patch,
        Method::Delete => WasiMethod::Delete,
    }
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
            Error::Denied(format!("http request refused: {code:?}"))
        }
        ErrorCode::ConnectionTimeout | ErrorCode::DestinationUnavailable => {
            Error::Offline(Some(format!("http request failed: {code:?}")))
        }
        other => Error::Failed(format!("http request failed: {other:?}")),
    }
}
