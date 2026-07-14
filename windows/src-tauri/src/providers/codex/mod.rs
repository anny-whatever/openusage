mod auth;
mod client;
mod history;
mod mapper;
mod reset;

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::StatusCode;
use tokio_util::sync::CancellationToken;

use crate::contracts::{MetricLine, ProviderSnapshot};
use crate::platform::http::{HttpResponse, HttpTransport};
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

pub use auth::{CodexAuthStore, CodexCredentials};
pub use client::CodexClient;
pub use reset::{
    CodexResetClaimService, PostClaimRefresh, ResetClaimConfirmation, ResetClaimOutcome,
};

use self::history::CodexHistoryScanner;
use self::mapper::{CodexMappedUsage, map_usage};
use super::support::{PricingCatalog, ProviderError, history_metrics, snapshot};

const PROVIDER_ID: &str = "codex";
const DISPLAY_NAME: &str = "Codex";

pub struct CodexProvider {
    auth: CodexAuthStore,
    client: CodexClient,
    history: CodexHistoryScanner,
    pricing: Arc<dyn PricingCatalog>,
}

impl CodexProvider {
    pub fn new(
        auth: CodexAuthStore,
        http: Arc<dyn HttpTransport>,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Self {
        Self {
            auth,
            client: CodexClient::new(http),
            history: CodexHistoryScanner::default(),
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
        let roots = self.auth.history_roots();
        let pricing = self.pricing.clone();
        let history = self.history.clone();
        let history_task = async move { history.scan(roots, pricing).await };
        let live_task = self.refresh_candidates(cancellation);
        let (live, local_history) = tokio::join!(live_task, history_task);
        let mut mapped = live?;
        let mut usage_history = None;
        let mut warning = None;
        match local_history {
            Ok(daily) if !daily.is_empty() => {
                let (lines, history) = history_metrics(
                    &daily,
                    chrono::DateTime::from(std::time::SystemTime::now()),
                    "From your local Codex history.",
                );
                mapped.lines.extend(lines);
                usage_history = Some(history);
            }
            Ok(_) => {}
            Err(_) => {
                warning = Some("Local history is temporarily unavailable.".to_owned());
            }
        }
        if mapped.lines.is_empty() {
            mapped.lines.push(MetricLine::Badge {
                label: "Status".to_owned(),
                text: "No usage data".to_owned(),
                color_hex: Some("#A3A3A3".to_owned()),
                subtitle: None,
            });
        }
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            mapped.plan,
            mapped.lines,
            usage_history,
            warning,
        ))
    }

    async fn refresh_candidates(
        &self,
        cancellation: CancellationToken,
    ) -> Result<CodexMappedUsage, ProviderError> {
        let candidates = self.auth.load_all().await?;
        let mut last_auth_error = None;
        for credentials in candidates {
            match self
                .refresh_live(credentials, cancellation.child_token())
                .await
            {
                Ok(mapped) => return Ok(mapped),
                Err(error)
                    if matches!(
                        error.category,
                        crate::contracts::ErrorCategory::AuthExpired
                            | crate::contracts::ErrorCategory::AuthInvalid
                    ) =>
                {
                    last_auth_error = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_auth_error.unwrap_or_else(|| ProviderError::not_logged_in("codex")))
    }

    async fn refresh_live(
        &self,
        mut credentials: CodexCredentials,
        cancellation: CancellationToken,
    ) -> Result<CodexMappedUsage, ProviderError> {
        let now = chrono::DateTime::from(std::time::SystemTime::now());
        if credentials.needs_refresh(now) {
            self.rotate(&credentials, cancellation.child_token())
                .await?;
            credentials = self.auth.reload(&credentials).await?;
        }
        let mut usage = self
            .client
            .usage(
                &credentials.access_token,
                credentials.account_id.as_ref(),
                cancellation.child_token(),
            )
            .await?;
        if is_auth_rejection(&usage) {
            self.rotate(&credentials, cancellation.child_token())
                .await?;
            credentials = self.auth.reload(&credentials).await?;
            usage = self
                .client
                .usage(
                    &credentials.access_token,
                    credentials.account_id.as_ref(),
                    cancellation.child_token(),
                )
                .await?;
        }
        let reset_credits = self
            .client
            .reset_credits(
                &credentials.access_token,
                credentials.account_id.as_ref(),
                cancellation.child_token(),
            )
            .await
            .ok();
        map_usage(&usage, reset_credits.as_ref())
    }

    async fn rotate(
        &self,
        credentials: &CodexCredentials,
        cancellation: CancellationToken,
    ) -> Result<(), ProviderError> {
        let refresh_token = credentials.refresh_token.as_ref().ok_or_else(|| {
            ProviderError::new(
                crate::contracts::ErrorCategory::AuthExpired,
                "Codex session expired. Run `codex` to log in again.",
            )
        })?;
        let refreshed = self.client.refresh(refresh_token, cancellation).await?;
        self.auth
            .save_refreshed(
                credentials,
                &refreshed.access_token,
                refreshed.refresh_token.as_ref(),
                refreshed.id_token.as_ref(),
            )
            .await
    }
}

#[async_trait]
impl ProviderRuntime for CodexProvider {
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

fn is_auth_rejection(response: &HttpResponse) -> bool {
    response.status == StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN
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
