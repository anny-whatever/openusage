use std::sync::Arc;

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Method, StatusCode, Url};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::contracts::ErrorCategory;
use crate::platform::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, authorization_header};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const SCOPES: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

#[derive(Clone)]
pub struct ClaudeClient {
    http: Arc<dyn HttpTransport>,
}

pub struct RefreshedClaudeTokens {
    pub access_token: SecretBytes,
    pub refresh_token: Option<SecretBytes>,
    pub expires_in_seconds: f64,
}

impl ClaudeClient {
    pub fn new(http: Arc<dyn HttpTransport>) -> Self {
        Self { http }
    }

    pub async fn usage(
        &self,
        access_token: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        let headers = vec![
            authorization_header(access_token)?,
            (ACCEPT, HeaderValue::from_static("application/json")),
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (
                HeaderName::from_static("anthropic-beta"),
                HeaderValue::from_static("oauth-2025-04-20"),
            ),
            (USER_AGENT, HeaderValue::from_static("claude-code/2.1.69")),
        ];
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url: Url::parse(USAGE_URL).expect("Claude usage URL is static and valid"),
                    headers,
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "Claude usage request failed."))
    }

    pub async fn refresh(
        &self,
        refresh_token: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<RefreshedClaudeTokens, ProviderError> {
        let refresh_text = std::str::from_utf8(refresh_token.expose()).map_err(|_| {
            ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Claude refresh token is invalid.",
            )
        })?;
        let body = serde_json::to_vec(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_text,
            "client_id": CLIENT_ID,
            "scope": SCOPES
        }))
        .map_err(|_| ProviderError::new(ErrorCategory::Other, "Claude refresh could not start."))?;
        let response = self
            .http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url: Url::parse(REFRESH_URL).expect("Claude refresh URL is static and valid"),
                    headers: vec![(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                    body: Some(body),
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "Claude token refresh failed.")
            })?;
        if response.status == StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN {
            return Err(ProviderError::new(
                ErrorCategory::AuthExpired,
                "Claude session expired. Run `claude` to log in again.",
            ));
        }
        if !response.status.is_success() {
            return Err(ProviderError::http(response.status));
        }
        let body: serde_json::Value = serde_json::from_slice(&response.body).map_err(|_| {
            ProviderError::new(
                ErrorCategory::Decoding,
                "Claude refresh response is invalid.",
            )
        })?;
        let access_token = body
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                ProviderError::new(
                    ErrorCategory::Decoding,
                    "Claude refresh response is invalid.",
                )
            })?;
        let expires_in_seconds = body
            .get("expires_in")
            .and_then(serde_json::Value::as_f64)
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
            .ok_or_else(|| {
                ProviderError::new(
                    ErrorCategory::Decoding,
                    "Claude refresh response is invalid.",
                )
            })?;
        Ok(RefreshedClaudeTokens {
            access_token: SecretBytes::new(access_token.as_bytes().to_vec()),
            refresh_token: body
                .get("refresh_token")
                .and_then(serde_json::Value::as_str)
                .map(|token| SecretBytes::new(token.as_bytes().to_vec())),
            expires_in_seconds,
        })
    }
}
