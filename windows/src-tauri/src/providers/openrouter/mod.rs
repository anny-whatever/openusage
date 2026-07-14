use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url};
use tokio_util::sync::CancellationToken;

use crate::contracts::{
    ErrorCategory, MetricFormat, MetricKind, MetricLine, MetricValue, ProviderSnapshot,
};
use crate::platform::environment::{EnvironmentReader, SystemEnvironment};
use crate::platform::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::platform::paths::WindowsPaths;
use crate::platform::secret::{DpapiSecretStore, SecretBytes};
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

use super::api_key::{ApiKeyStatus, ProtectedApiKeyStore};
use super::support::{
    ProviderError, authorization_header, json_object, number, provider_failure, snapshot,
};

const PROVIDER_ID: &str = "openrouter";
const DISPLAY_NAME: &str = "OpenRouter";
const CREDITS_URL: &str = "https://openrouter.ai/api/v1/credits";
const KEY_URL: &str = "https://openrouter.ai/api/v1/key";

pub struct OpenRouterProvider {
    auth: ProtectedApiKeyStore,
    http: Arc<dyn HttpTransport>,
}

impl OpenRouterProvider {
    pub fn from_windows(paths: &WindowsPaths, http: Arc<dyn HttpTransport>) -> Self {
        Self::with_environment(paths, http, Arc::new(SystemEnvironment))
    }

    pub fn with_environment(
        paths: &WindowsPaths,
        http: Arc<dyn HttpTransport>,
        environment: Arc<dyn EnvironmentReader>,
    ) -> Self {
        let auth = ProtectedApiKeyStore::new(
            PROVIDER_ID,
            &["OPENROUTER_API_KEY", "OPENROUTER_KEY"],
            vec![
                paths.user_profile.join(".config/openusage/openrouter.json"),
                paths.user_profile.join(".config/openrouter/key.json"),
            ],
            environment,
            DpapiSecretStore::new(paths.openusage_data().join("secrets")),
        );
        Self { auth, http }
    }

    pub async fn key_status(&self) -> Result<ApiKeyStatus, ProviderError> {
        self.auth.status().await
    }

    pub async fn save_key(&self, key: SecretBytes) -> Result<ApiKeyStatus, ProviderError> {
        self.auth.save(key).await
    }

    pub async fn delete_key(&self) -> Result<ApiKeyStatus, ProviderError> {
        self.auth.delete().await
    }

    async fn refresh_provider(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderError> {
        let key = self.auth.load().await?.ok_or_else(|| {
            ProviderError::new(
                ErrorCategory::NotLoggedIn,
                "No OpenRouter API key is configured.",
            )
        })?;
        let credits = self.get(CREDITS_URL, &key, cancellation.child_token());
        let metadata = self.get(KEY_URL, &key, cancellation.child_token());
        let (credits, metadata) = tokio::join!(credits, metadata);
        let mut lines = Vec::new();
        let mut plan = None;
        let mut failures = Vec::new();
        match credits {
            Ok(response) => match map_credits(&response) {
                Ok(mapped) => lines.extend(mapped),
                Err(error) => failures.push(error),
            },
            Err(error) => failures.push(error),
        }
        match metadata {
            Ok(response) => match map_key(&response) {
                Ok((mapped_plan, mapped)) => {
                    plan = mapped_plan;
                    lines.extend(mapped);
                }
                Err(error) => failures.push(error),
            },
            Err(error) => failures.push(error),
        }
        if lines.is_empty() {
            if failures.len() == 2
                && failures
                    .iter()
                    .all(|error| error.category == ErrorCategory::AuthInvalid)
            {
                return Err(ProviderError::new(
                    ErrorCategory::AuthInvalid,
                    "OpenRouter API key is invalid.",
                ));
            }
            return Err(failures.into_iter().next().unwrap_or_else(|| {
                ProviderError::new(
                    ErrorCategory::Decoding,
                    "OpenRouter returned no usage data.",
                )
            }));
        }
        let warning = (!failures.is_empty())
            .then(|| "Some OpenRouter usage is temporarily unavailable.".to_owned());
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            plan,
            lines,
            None,
            warning,
        ))
    }

    async fn get(
        &self,
        url: &str,
        key: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        let authorization = authorization_header(key)?;
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url: Url::parse(url).expect("static OpenRouter URL"),
                    headers: vec![authorization],
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "OpenRouter could not be reached.")
            })
    }
}

fn response_data(response: &HttpResponse) -> Result<serde_json::Value, ProviderError> {
    if response.status == StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN {
        return Err(ProviderError::new(
            ErrorCategory::AuthInvalid,
            "OpenRouter API key is invalid.",
        ));
    }
    if !response.status.is_success() {
        return Err(ProviderError::http(response.status));
    }
    json_object(&response.body)?
        .get("data")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| {
            ProviderError::new(ErrorCategory::Decoding, "OpenRouter response is invalid.")
        })
}

fn map_credits(response: &HttpResponse) -> Result<Vec<MetricLine>, ProviderError> {
    let data = response_data(response)?;
    let used = number(data.get("total_usage"))
        .ok_or_else(|| {
            ProviderError::new(ErrorCategory::Decoding, "OpenRouter response is invalid.")
        })?
        .max(0.0);
    let total = number(data.get("total_credits")).unwrap_or(0.0).max(0.0);
    let mut lines = Vec::new();
    if total > 0.0 {
        lines.push(progress("Credits", used, total, MetricFormat::Dollars));
    }
    lines.push(value_line("Balance", (total - used).max(0.0)));
    Ok(lines)
}

fn map_key(response: &HttpResponse) -> Result<(Option<String>, Vec<MetricLine>), ProviderError> {
    let data = response_data(response)?;
    let mut lines = Vec::new();
    for (field, label) in [
        ("usage_daily", "Today"),
        ("usage_weekly", "This Week"),
        ("usage_monthly", "This Month"),
    ] {
        if let Some(amount) = number(data.get(field)) {
            lines.push(value_line(label, amount.max(0.0)));
        }
    }
    if let Some(limit) = number(data.get("limit")).filter(|limit| *limit > 0.0) {
        lines.push(progress(
            "Key Limit",
            number(data.get("usage")).unwrap_or(0.0).max(0.0),
            limit,
            MetricFormat::Dollars,
        ));
    }
    let plan = data
        .get("is_free_tier")
        .and_then(serde_json::Value::as_bool)
        .map(|free| if free { "Free Tier" } else { "Pay As You Go" }.to_owned());
    Ok((plan, lines))
}

fn progress(label: &str, used: f64, limit: f64, format: MetricFormat) -> MetricLine {
    MetricLine::Progress {
        label: label.to_owned(),
        used,
        limit,
        format,
        resets_at: None,
        period_duration_ms: None,
        color_hex: None,
    }
}

fn value_line(label: &str, number: f64) -> MetricLine {
    MetricLine::Values {
        label: label.to_owned(),
        values: vec![MetricValue {
            number,
            kind: MetricKind::Dollars,
            label: None,
            estimated: false,
        }],
        color_hex: None,
        expiries_at: Vec::new(),
        unknown_models: Vec::new(),
    }
}

#[async_trait]
impl ProviderRuntime for OpenRouterProvider {
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

#[cfg(test)]
mod tests;
