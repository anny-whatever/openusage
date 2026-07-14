mod auth;
mod client;
mod csv;
mod mapper;

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;

use crate::contracts::ProviderSnapshot;
use crate::platform::http::HttpTransport;
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

pub use auth::{CursorAuthStore, CursorCredentials};

use self::client::{CursorClient, RefreshedCursorToken};
use self::csv::parse_csv;
use self::mapper::map_usage;
use super::support::{PricingCatalog, ProviderError, history_metrics, snapshot};

const PROVIDER_ID: &str = "cursor";
const DISPLAY_NAME: &str = "Cursor";

pub struct CursorProvider {
    auth: CursorAuthStore,
    client: CursorClient,
    pricing: Arc<dyn PricingCatalog>,
}

impl CursorProvider {
    pub fn new(
        auth: CursorAuthStore,
        http: Arc<dyn HttpTransport>,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Self {
        Self {
            auth,
            client: CursorClient::new(http),
            pricing,
        }
    }

    pub async fn has_local_credentials(&self) -> bool {
        self.auth.has_usable_credentials().await
    }

    async fn refresh_provider(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderError> {
        let mut credentials = self.auth.load().await?;
        let original_subject = credentials.subject();
        let now = chrono::DateTime::from(std::time::SystemTime::now());
        let mut access = if credentials.needs_refresh(now) {
            let refreshed = self
                .rotate(&credentials, cancellation.child_token())
                .await?;
            if let Some(refresh_token) = refreshed.refresh_token {
                credentials.refresh_token = Some(refresh_token);
            }
            refreshed.access_token
        } else {
            credentials.access_token.take().ok_or_else(|| {
                ProviderError::new(
                    crate::contracts::ErrorCategory::AuthExpired,
                    "Cursor session expired. Sign in again.",
                )
            })?
        };
        let mut usage = self
            .client
            .usage(&access, cancellation.child_token())
            .await?;
        if is_auth_rejection(usage.status) {
            let refreshed = self
                .rotate(&credentials, cancellation.child_token())
                .await?;
            if let Some(refresh_token) = refreshed.refresh_token {
                credentials.refresh_token = Some(refresh_token);
            }
            access = refreshed.access_token;
            usage = self
                .client
                .usage(&access, cancellation.child_token())
                .await?;
        }

        let subject = token_subject(&access).or(original_subject);
        let plan_task = self.client.plan(&access, cancellation.child_token());
        let credits_task = self.client.credits(&access, cancellation.child_token());
        let csv_task = async {
            let subject = subject.ok_or_else(|| {
                ProviderError::new(
                    crate::contracts::ErrorCategory::AuthInvalid,
                    "Cursor session is invalid.",
                )
            })?;
            let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
            let start = now - chrono::Duration::days(30);
            self.client
                .csv(
                    &access,
                    &subject,
                    start.timestamp_millis(),
                    now.timestamp_millis(),
                    cancellation.child_token(),
                )
                .await
        };
        let (plan, credits, csv) = tokio::join!(plan_task, credits_task, csv_task);
        let mut mapped = map_usage(&usage, plan.as_ref().ok(), credits.as_ref().ok())?;
        let mut usage_history = None;
        let mut warning = None;
        match csv {
            Ok(response) if response.status.is_success() => {
                match parse_csv(&response.body, self.pricing.as_ref()) {
                    Ok(daily) if !daily.is_empty() => {
                        let (lines, history) = history_metrics(
                            &daily,
                            chrono::DateTime::from(std::time::SystemTime::now()),
                            "From your Cursor usage export.",
                        );
                        mapped.lines.extend(lines);
                        usage_history = Some(history);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        warning =
                            Some("Cursor usage history is temporarily unavailable.".to_owned())
                    }
                }
            }
            Ok(_) | Err(_) => {
                warning = Some("Cursor usage history is temporarily unavailable.".to_owned())
            }
        }
        // Refreshed tokens are deliberately session-only: the Windows port never mutates Cursor's DB.
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            mapped.plan,
            mapped.lines,
            usage_history,
            warning,
        ))
    }

    async fn rotate(
        &self,
        credentials: &CursorCredentials,
        cancellation: CancellationToken,
    ) -> Result<RefreshedCursorToken, ProviderError> {
        let refresh = credentials.refresh_token.as_ref().ok_or_else(|| {
            ProviderError::new(
                crate::contracts::ErrorCategory::AuthExpired,
                "Cursor session expired. Sign in again.",
            )
        })?;
        self.client.refresh(refresh, cancellation).await
    }
}

#[async_trait]
impl ProviderRuntime for CursorProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    async fn refresh(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderFailure> {
        self.refresh_provider(cancellation)
            .await
            .map_err(provider_failure)
    }
}

fn is_auth_rejection(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
}

fn token_subject(token: &crate::platform::secret::SecretBytes) -> Option<String> {
    use base64::Engine;

    let token = std::str::from_utf8(token.expose()).ok()?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.split('.').nth(1)?)
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    value
        .get("sub")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn provider_failure(error: ProviderError) -> ProviderFailure {
    ProviderFailure {
        category: serde_json::to_value(error.category)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "other".to_owned()),
        message: error.message,
    }
}

#[cfg(test)]
mod tests;
