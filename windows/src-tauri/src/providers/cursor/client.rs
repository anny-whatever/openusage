use std::sync::Arc;

use reqwest::header::{ACCEPT, CONTENT_TYPE, COOKIE, HeaderName, HeaderValue};
use reqwest::{Method, Url};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::contracts::ErrorCategory;
use crate::platform::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::platform::secret::SecretBytes;

use super::super::support::{ProviderError, authorization_header, text};

const USAGE_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCurrentPeriodUsage";
const PLAN_URL: &str = "https://api2.cursor.sh/aiserver.v1.DashboardService/GetPlanInfo";
const CREDITS_URL: &str =
    "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCreditGrantsBalance";
const REFRESH_URL: &str = "https://api2.cursor.sh/oauth/token";
const CSV_URL: &str = "https://cursor.com/api/dashboard/export-usage-events-csv";
const CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";

pub struct RefreshedCursorToken {
    pub access_token: SecretBytes,
    pub refresh_token: Option<SecretBytes>,
}

#[derive(Clone)]
pub struct CursorClient {
    http: Arc<dyn HttpTransport>,
}

impl CursorClient {
    pub fn new(http: Arc<dyn HttpTransport>) -> Self {
        Self { http }
    }

    pub async fn refresh(
        &self,
        refresh_token: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<RefreshedCursorToken, ProviderError> {
        let refresh = std::str::from_utf8(refresh_token.expose()).map_err(|_| {
            ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Cursor refresh token is invalid.",
            )
        })?;
        let body = serde_json::to_vec(&json!({
            "grant_type": "refresh_token",
            "client_id": CLIENT_ID,
            "refresh_token": refresh
        }))
        .map_err(|_| ProviderError::new(ErrorCategory::Other, "Cursor refresh could not start."))?;
        let response = self
            .http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url: static_url(REFRESH_URL),
                    headers: vec![(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                    body: Some(body),
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "Cursor token refresh failed.")
            })?;
        if !response.status.is_success() {
            return Err(ProviderError::new(
                if response.status.is_client_error() {
                    ErrorCategory::AuthExpired
                } else {
                    ErrorCategory::Http5xx
                },
                "Cursor session expired. Sign in again.",
            ));
        }
        let body: Value = serde_json::from_slice(&response.body).map_err(|_| {
            ProviderError::new(
                ErrorCategory::Decoding,
                "Cursor refresh response is invalid.",
            )
        })?;
        let access = text(body.get("access_token")).ok_or_else(|| {
            ProviderError::new(
                ErrorCategory::Decoding,
                "Cursor refresh response is invalid.",
            )
        })?;
        Ok(RefreshedCursorToken {
            access_token: SecretBytes::new(access.as_bytes().to_vec()),
            refresh_token: text(body.get("refresh_token"))
                .map(|token| SecretBytes::new(token.as_bytes().to_vec())),
        })
    }

    pub async fn usage(
        &self,
        access: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.connect_post(USAGE_URL, access, cancellation).await
    }

    pub async fn plan(
        &self,
        access: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.connect_post(PLAN_URL, access, cancellation).await
    }

    pub async fn credits(
        &self,
        access: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.connect_post(CREDITS_URL, access, cancellation).await
    }

    pub async fn csv(
        &self,
        access: &SecretBytes,
        subject: &str,
        start_ms: i64,
        end_ms: i64,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        let user_id = subject.split('|').nth(1).unwrap_or(subject);
        if user_id.is_empty() {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Cursor session is invalid.",
            ));
        }
        let mut url = static_url(CSV_URL);
        url.query_pairs_mut()
            .append_pair("startDate", &start_ms.to_string())
            .append_pair("endDate", &end_ms.to_string())
            .append_pair("strategy", "tokens");
        let mut cookie = Vec::new();
        cookie.extend_from_slice(b"WorkosCursorSessionToken=");
        cookie.extend_from_slice(user_id.as_bytes());
        cookie.extend_from_slice(b"%3A%3A");
        cookie.extend_from_slice(access.expose());
        let cookie_header = HeaderValue::from_bytes(&cookie).map_err(|_| {
            ProviderError::new(ErrorCategory::AuthInvalid, "Cursor session is invalid.")
        });
        cookie.zeroize();
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url,
                    headers: vec![
                        (COOKIE, cookie_header?),
                        (ACCEPT, HeaderValue::from_static("text/csv")),
                    ],
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "Cursor history request failed.")
            })
    }

    async fn connect_post(
        &self,
        url: &str,
        access: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        let headers = vec![
            authorization_header(access)?,
            (CONTENT_TYPE, HeaderValue::from_static("application/json")),
            (
                HeaderName::from_static("connect-protocol-version"),
                HeaderValue::from_static("1"),
            ),
        ];
        self.http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url: static_url(url),
                    headers,
                    body: Some(b"{}".to_vec()),
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "Cursor request failed."))
    }
}

fn static_url(value: &str) -> Url {
    Url::parse(value).expect("Cursor endpoint is static and valid")
}
