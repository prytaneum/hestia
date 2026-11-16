use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use http_body::Body as _;
use http_body_util::{BodyDataStream, BodyExt, LengthLimitError, Limited};
use wasmtime_wasi_http::{Error, RequestOptions, WasiBody, WasiHttpHooks};

use super::reach::Reach;
use crate::vetting::{was_refused, Limits, Refusal};

type Sent = Box<dyn Future<Output = Result<(), Error>> + Send>;

/// How `wasi:http` requests leave: through the plugin's own policy, the same
/// one core requests pass. The guest follows redirects itself, so every hop is a
/// new request that the policy sees.
pub(super) struct Hooks {
    pub plugin: String,
    pub reach: Arc<Reach>,
}

impl WasiHttpHooks for Hooks {
    fn set_host_header(&mut self) -> bool {
        false
    }

    fn send_request(
        &mut self,
        request: http::Request<WasiBody>,
        _options: Option<RequestOptions>,
        _fut: Sent,
    ) -> Box<dyn Future<Output = Result<(http::Response<WasiBody>, Sent), Error>> + Send> {
        let (plugin, reach) = (self.plugin.clone(), self.reach.clone());
        Box::new(async move {
            let response = send(&plugin, &reach, request).await?;
            Ok((response, Box::new(async { Ok(()) }) as Sent))
        })
    }
}

async fn send(
    plugin: &str,
    reach: &Reach,
    request: http::Request<WasiBody>,
) -> Result<http::Response<WasiBody>, Error> {
    let limits = Limits::REQUEST;
    let (parts, body) = request.into_parts();
    let url = parts.uri.to_string();
    let builder = reach
        .policy
        .request(parts.method, &url, limits)
        .map_err(|refusal| {
            tracing::warn!(plugin, %refusal, "plugin request refused");
            match refusal {
                Refusal::BadUrl => Error::HttpRequestUriInvalid,
                _ => Error::HttpRequestDenied,
            }
        })?;
    let mut builder = builder.headers(parts.headers);
    if !body.is_end_stream() {
        builder = builder.body(reqwest::Body::wrap_stream(BodyDataStream::new(body)));
    }
    let response = builder.send().await.map_err(|error| {
        if was_refused(&error) {
            tracing::warn!(plugin, "plugin request resolved to a non-public address");
            Error::DestinationIpProhibited
        } else if error.is_timeout() {
            Error::ConnectionTimeout
        } else if error.is_connect() {
            Error::ConnectionRefused
        } else {
            Error::HttpProtocolError
        }
    })?;
    if let Some(length) = response.content_length().filter(|n| *n > limits.bytes) {
        return Err(Error::HttpResponseBodySize(Some(length)));
    }
    let (parts, body) = http::Response::<reqwest::Body>::from(response).into_parts();
    let body = Limited::new(body, limits.bytes as usize)
        .map_err(|error| match error.is::<LengthLimitError>() {
            true => Error::HttpResponseBodySize(None),
            false => Error::HttpProtocolError,
        })
        .map_frame(|frame| frame.map_data(Bytes::from))
        .boxed_unsync();
    Ok(http::Response::from_parts(parts, body))
}
