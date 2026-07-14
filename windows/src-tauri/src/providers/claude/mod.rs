mod auth;
mod client;
mod history;
mod mapper;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;

use crate::contracts::ProviderSnapshot;
use crate::platform::http::HttpTransport;
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

pub use auth::{ClaudeAuthStore, ClaudeCredentials};

use self::client::ClaudeClient;
use self::history::ClaudeHistoryScanner;
use self::mapper::{ClaudeMappedUsage, is_auth_rejection, map_usage, rate_limited};
use super::support::{PricingCatalog, ProviderError, history_metrics, snapshot};

const PROVIDER_ID: &str = "claude";
const DISPLAY_NAME: &str = "Claude";
const MISSING_PROFILE_WARNING: &str =
    "Re-login for live usage. Run `claude` and sign in again to restore session and weekly limits.";

pub struct ClaudeProvider {
    auth: ClaudeAuthStore,
    client: ClaudeClient,
    history: ClaudeHistoryScanner,
    pricing: Arc<dyn PricingCatalog>,
}

impl ClaudeProvider {
    pub fn new(
        auth: ClaudeAuthStore,
        http: Arc<dyn HttpTransport>,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Self {
        Self {
            auth,
            client: ClaudeClient::new(http),
            history: ClaudeHistoryScanner::default(),
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
        let credentials = self.auth.load().await?;
        let projects_root = self.auth.projects_root();
        let pricing = self.pricing.clone();
        let history = self.history.clone();
        let history_task = async move { history.scan(projects_root, pricing).await };
        let live_task = self.refresh_live(credentials, cancellation);
        let (live, local_history) = tokio::join!(live_task, history_task);
        let mut mapped = live?;

        let mut usage_history = None;
        match local_history {
            Ok(daily) if !daily.is_empty() => {
                let (history_lines, normalized_history) = history_metrics(
                    &daily,
                    chrono::DateTime::<Utc>::from(std::time::SystemTime::now()),
                    "From your local Claude Code history.",
                );
                mapped.lines.extend(history_lines);
                usage_history = Some(normalized_history);
            }
            Ok(_) => {}
            Err(_) => {
                mapped.warning = Some(match mapped.warning {
                    Some(warning) => format!("{warning} Local history is temporarily unavailable."),
                    None => "Local history is temporarily unavailable.".to_owned(),
                });
            }
        }
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            mapped.plan,
            mapped.lines,
            usage_history,
            mapped.warning,
        ))
    }

    async fn refresh_live(
        &self,
        mut credentials: ClaudeCredentials,
        cancellation: CancellationToken,
    ) -> Result<ClaudeMappedUsage, ProviderError> {
        if !credentials.has_profile_scope() {
            return Ok(ClaudeMappedUsage {
                plan: credentials.subscription_type.clone(),
                lines: Vec::new(),
                warning: Some(MISSING_PROFILE_WARNING.to_owned()),
            });
        }
        let now_ms = now_utc().timestamp_millis() as f64;
        if credentials.needs_refresh(now_ms) {
            self.rotate(&credentials, cancellation.child_token())
                .await?;
            credentials = self.auth.load().await?;
        }

        let mut response = self
            .client
            .usage(&credentials.access_token, cancellation.child_token())
            .await?;
        if is_auth_rejection(response.status) {
            self.rotate(&credentials, cancellation.child_token())
                .await?;
            credentials = self.auth.load().await?;
            response = self
                .client
                .usage(&credentials.access_token, cancellation.child_token())
                .await?;
        }
        if response.status == StatusCode::TOO_MANY_REQUESTS {
            return Ok(rate_limited(&response, &credentials));
        }
        map_usage(&response, &credentials)
    }

    async fn rotate(
        &self,
        credentials: &ClaudeCredentials,
        cancellation: CancellationToken,
    ) -> Result<(), ProviderError> {
        let refresh_token = credentials.refresh_token.as_ref().ok_or_else(|| {
            ProviderError::new(
                crate::contracts::ErrorCategory::AuthExpired,
                "Claude session expired. Run `claude` to log in again.",
            )
        })?;
        let refreshed = self.client.refresh(refresh_token, cancellation).await?;
        let expires_at =
            now_utc().timestamp_millis() as f64 + refreshed.expires_in_seconds * 1000.0;
        self.auth
            .save_refreshed(
                credentials,
                &refreshed.access_token,
                refreshed.refresh_token.as_ref(),
                expires_at,
            )
            .await
    }
}

fn now_utc() -> chrono::DateTime<Utc> {
    chrono::DateTime::<Utc>::from(std::time::SystemTime::now())
}

#[async_trait]
impl ProviderRuntime for ClaudeProvider {
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
