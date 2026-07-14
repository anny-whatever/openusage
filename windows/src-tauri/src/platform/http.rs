use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Method, StatusCode, Url};
use tokio_util::sync::CancellationToken;

use async_trait::async_trait;

const DEFAULT_RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub response_limit: usize,
    pub proxy: Option<Url>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(20),
            response_limit: DEFAULT_RESPONSE_LIMIT,
            proxy: None,
        }
    }
}

#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: Url,
    pub headers: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>,
    pub body: Option<Vec<u8>>,
}

pub struct HttpResponse {
    pub status: StatusCode,
    pub headers: reqwest::header::HeaderMap,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)?.to_str().ok()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("HTTP client configuration failed: {0}")]
    Configuration(String),
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("HTTP request was cancelled")]
    Cancelled,
    #[error("HTTP response exceeded the configured size limit")]
    ResponseTooLarge,
}

#[derive(Clone)]
pub struct BoundedHttpClient {
    client: reqwest::Client,
    response_limit: usize,
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn send(
        &self,
        request: HttpRequest,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError>;
}

impl BoundedHttpClient {
    pub fn new(config: HttpClientConfig) -> Result<Self, HttpError> {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .timeout(config.request_timeout)
            .redirect(reqwest::redirect::Policy::limited(5));
        if let Some(proxy) = config.proxy {
            let proxy = reqwest::Proxy::all(proxy.as_str())
                .map_err(|error| HttpError::Configuration(error.to_string()))?;
            builder = builder.proxy(proxy);
        }
        let client = builder
            .build()
            .map_err(|error| HttpError::Configuration(error.to_string()))?;
        Ok(Self {
            client,
            response_limit: config.response_limit,
        })
    }

    async fn send_bounded(
        &self,
        request: HttpRequest,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        let mut builder = self.client.request(request.method, request.url);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }

        let response = tokio::select! {
            response = builder.send() => response.map_err(safe_request_error)?,
            () = cancellation.cancelled() => return Err(HttpError::Cancelled),
        };
        if response
            .content_length()
            .is_some_and(|size| size > self.response_limit as u64)
        {
            return Err(HttpError::ResponseTooLarge);
        }
        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = response.bytes_stream();
        let mut body = Vec::with_capacity(16 * 1024);
        loop {
            let next = tokio::select! {
                chunk = stream.next() => chunk,
                () = cancellation.cancelled() => return Err(HttpError::Cancelled),
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(safe_request_error)?;
            if body.len().saturating_add(chunk.len()) > self.response_limit {
                return Err(HttpError::ResponseTooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[async_trait]
impl HttpTransport for BoundedHttpClient {
    async fn send(
        &self,
        request: HttpRequest,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        self.send_bounded(request, cancellation).await
    }
}

fn safe_request_error(error: reqwest::Error) -> HttpError {
    let category = if error.is_timeout() {
        "request timed out"
    } else if error.is_connect() {
        "connection failed"
    } else if error.is_decode() {
        "response decoding failed"
    } else {
        "transport failed"
    };
    HttpError::Request(category.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_explicit_proxy_configuration_without_credentials_in_errors() {
        let config = HttpClientConfig {
            proxy: Some(Url::parse("http://proxy.example:8080").unwrap()),
            ..HttpClientConfig::default()
        };

        BoundedHttpClient::new(config).expect("proxy should configure");
    }

    #[test]
    fn safe_request_errors_do_not_include_request_urls() {
        let error = reqwest::Client::new()
            .get("not a url")
            .build()
            .expect_err("invalid URL must fail");

        assert!(!safe_request_error(error).to_string().contains("not a url"));
    }
}
