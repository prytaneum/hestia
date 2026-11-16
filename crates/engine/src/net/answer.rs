use anyhow::Result;
use kernel::vetting::Limits;
use proto::error::{ErrorInfo, Service};
use reqwest::header::HeaderMap;
use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;

/// An API call's answer, whose body can only be read under the request size
/// cap: the reason an API caller gets this rather than a bare response.
pub(crate) struct Answer {
    response: Response,
    service: Service,
}

/// Sends an API request through [`super::send`].
pub(crate) async fn call(service: Service, request: RequestBuilder) -> Result<Answer> {
    let response = super::send(Some(service.clone()), request).await?;
    Ok(Answer { response, service })
}

impl Answer {
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }

    /// This answer when it is a 2xx, an upstream failure naming its status otherwise.
    pub fn success(self) -> Result<Self> {
        if self.status().is_success() {
            return Ok(self);
        }
        let status = self.status();
        Err(super::status_failure(self.service, status))
    }

    pub fn headers(&self) -> &HeaderMap {
        self.response.headers()
    }

    pub async fn bytes(self) -> Result<Vec<u8>> {
        super::read(Some(self.service), self.response, Limits::REQUEST).await
    }

    pub async fn text(self) -> Result<String> {
        let service = self.service.clone();
        let body = self.bytes().await?;
        String::from_utf8(body).map_err(|error| malformed(service, error))
    }

    pub async fn json<T: DeserializeOwned>(self) -> Result<T> {
        let service = self.service.clone();
        let body = self.bytes().await?;
        serde_json::from_slice(&body).map_err(|error| malformed(service, error))
    }
}

pub(super) fn malformed(service: Service, error: impl std::fmt::Display) -> anyhow::Error {
    ErrorInfo::Upstream {
        service,
        detail: error.to_string(),
    }
    .into()
}
