use std::sync::Arc;

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderName, HeaderValue, USER_AGENT};
use reqwest::{Method, StatusCode, Url};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::contracts::ErrorCategory;
use crate::platform::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, authorization_header, text};

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REFRESH_URL: &str = "https://auth.openai.com/oauth/token";
const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const CONSUME_RESET_URL: &str =
    "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits/consume";

pub struct RefreshedCodexTokens {
    pub access_token: SecretBytes,
    pub refresh_token: Option<SecretBytes>,
    pub id_token: Option<SecretBytes>,
}

#[derive(Clone)]
pub struct CodexClient {
    http: Arc<dyn HttpTransport>,
}

impl CodexClient {
    pub fn new(http: Arc<dyn HttpTransport>) -> Self {
        Self { http }
    }

    pub async fn refresh(
        &self,
        refresh_token: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<RefreshedCodexTokens, ProviderError> {
        let refresh_token = std::str::from_utf8(refresh_token.expose()).map_err(|_| {
            ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Codex refresh token is invalid.",
            )
        })?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "refresh_token")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("refresh_token", refresh_token)
            .finish()
            .into_bytes();
        let response = self
            .http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url: static_url(REFRESH_URL),
                    headers: vec![(
                        CONTENT_TYPE,
                        HeaderValue::from_static("application/x-www-form-urlencoded"),
                    )],
                    body: Some(body),
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "Codex token refresh failed.")
            })?;
        if response.status == StatusCode::BAD_REQUEST || response.status == StatusCode::UNAUTHORIZED
        {
            return Err(refresh_rejection(&response.body));
        }
        if !response.status.is_success() {
            return Err(ProviderError::http(response.status));
        }
        let body: Value = serde_json::from_slice(&response.body).map_err(|_| {
            ProviderError::new(
                ErrorCategory::Decoding,
                "Codex refresh response is invalid.",
            )
        })?;
        let access_token = text(body.get("access_token")).ok_or_else(|| {
            ProviderError::new(
                ErrorCategory::AuthExpired,
                "Codex session expired. Log in again.",
            )
        })?;
        Ok(RefreshedCodexTokens {
            access_token: SecretBytes::new(access_token.as_bytes().to_vec()),
            refresh_token: text(body.get("refresh_token"))
                .map(|token| SecretBytes::new(token.as_bytes().to_vec())),
            id_token: text(body.get("id_token"))
                .map(|token| SecretBytes::new(token.as_bytes().to_vec())),
        })
    }

    pub async fn usage(
        &self,
        access_token: &SecretBytes,
        account_id: Option<&SecretBytes>,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.get(USAGE_URL, access_token, account_id, false, cancellation)
            .await
    }

    pub async fn reset_credits(
        &self,
        access_token: &SecretBytes,
        account_id: Option<&SecretBytes>,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.get(
            RESET_CREDITS_URL,
            access_token,
            account_id,
            true,
            cancellation,
        )
        .await
    }

    pub async fn consume_reset(
        &self,
        access_token: &SecretBytes,
        account_id: Option<&SecretBytes>,
        credit_id: &str,
        idempotency_key: &str,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        validate_claim_value(credit_id)?;
        validate_claim_value(idempotency_key)?;
        let mut headers = request_headers(access_token, account_id, true)?;
        headers.push((CONTENT_TYPE, HeaderValue::from_static("application/json")));
        let body = serde_json::to_vec(&json!({
            "credit_id": credit_id,
            "redeem_request_id": idempotency_key
        }))
        .map_err(|_| ProviderError::new(ErrorCategory::Other, "Reset claim could not start."))?;
        self.http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url: static_url(CONSUME_RESET_URL),
                    headers,
                    body: Some(body),
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "Reset claim request failed."))
    }

    async fn get(
        &self,
        url: &str,
        access_token: &SecretBytes,
        account_id: Option<&SecretBytes>,
        codex_headers: bool,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url: static_url(url),
                    headers: request_headers(access_token, account_id, codex_headers)?,
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "Codex request failed."))
    }
}

fn request_headers(
    access_token: &SecretBytes,
    account_id: Option<&SecretBytes>,
    codex_headers: bool,
) -> Result<Vec<(HeaderName, HeaderValue)>, ProviderError> {
    let mut headers = vec![
        authorization_header(access_token)?,
        (ACCEPT, HeaderValue::from_static("application/json")),
        (USER_AGENT, HeaderValue::from_static("OpenUsage")),
    ];
    if let Some(account_id) = account_id {
        headers.push((
            HeaderName::from_static("chatgpt-account-id"),
            HeaderValue::from_bytes(account_id.expose()).map_err(|_| {
                ProviderError::new(ErrorCategory::AuthInvalid, "Codex account is invalid.")
            })?,
        ));
    }
    if codex_headers {
        headers.push((
            HeaderName::from_static("openai-beta"),
            HeaderValue::from_static("codex-1"),
        ));
        headers.push((
            HeaderName::from_static("originator"),
            HeaderValue::from_static("Codex Desktop"),
        ));
    }
    Ok(headers)
}

fn refresh_rejection(body: &[u8]) -> ProviderError {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let code = text(value.get("code")).or_else(|| {
        value
            .get("error")
            .and_then(|error| text(error.get("code")).or_else(|| error.as_str()))
    });
    let message = match code {
        Some("refresh_token_reused") => "Codex token conflict. Run `codex` to log in again.",
        Some("refresh_token_invalidated") => "Codex token revoked. Run `codex` to log in again.",
        _ => "Codex session expired. Run `codex` to log in again.",
    };
    ProviderError::new(ErrorCategory::AuthExpired, message)
}

fn validate_claim_value(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ProviderError::new(
            ErrorCategory::AuthInvalid,
            "Reset claim identifier is invalid.",
        ));
    }
    Ok(())
}

fn static_url(value: &str) -> Url {
    Url::parse(value).expect("Codex endpoint is static and valid")
}
