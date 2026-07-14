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
    ProviderError, authorization_header, json_object, number, provider_failure, snapshot, text,
    timestamp,
};

const PROVIDER_ID: &str = "zai";
const DISPLAY_NAME: &str = "Z.ai";
const QUOTA_URL: &str = "https://api.z.ai/api/monitor/usage/quota/limit";
const SUBSCRIPTION_URL: &str = "https://api.z.ai/api/biz/subscription/list";
const MONTH_MS: u64 = 30 * 24 * 60 * 60 * 1000;

pub struct ZaiProvider {
    auth: ProtectedApiKeyStore,
    http: Arc<dyn HttpTransport>,
}

impl ZaiProvider {
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
            &["ZAI_API_KEY", "GLM_API_KEY"],
            vec![
                paths.user_profile.join(".config/openusage/zai.json"),
                paths.user_profile.join(".config/zai/key.json"),
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
            ProviderError::new(ErrorCategory::NotLoggedIn, "No Z.ai API key is configured.")
        })?;
        let quota = self.get(QUOTA_URL, &key, cancellation.child_token());
        let subscription = self.get(SUBSCRIPTION_URL, &key, cancellation.child_token());
        let (quota, subscription) = tokio::join!(quota, subscription);
        let quota = quota?;
        if quota.status == StatusCode::UNAUTHORIZED || quota.status == StatusCode::FORBIDDEN {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "Z.ai API key is invalid.",
            ));
        }
        if !quota.status.is_success() {
            return Err(ProviderError::http(quota.status));
        }
        let quota_body = json_object(&quota.body)?;
        if quota_body
            .get("success")
            .and_then(serde_json::Value::as_bool)
            == Some(false)
            && text(quota_body.get("msg"))
                .is_some_and(|message| message.to_ascii_lowercase().contains("coding plan"))
        {
            return Err(ProviderError::new(
                ErrorCategory::NotAvailable,
                "No active GLM Coding Plan.",
            ));
        }
        let lines = map_quota(&quota_body)?;
        let plan = subscription
            .ok()
            .and_then(|response| {
                response
                    .status
                    .is_success()
                    .then(|| json_object(&response.body).ok())
                    .flatten()
            })
            .and_then(|body| {
                body.get("data")?
                    .as_array()?
                    .first()
                    .and_then(|entry| text(entry.get("productName")))
                    .map(str::to_owned)
            });
        Ok(snapshot(PROVIDER_ID, DISPLAY_NAME, plan, lines, None, None))
    }

    async fn get(
        &self,
        url: &str,
        key: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url: Url::parse(url).expect("static Z.ai URL"),
                    headers: vec![authorization_header(key)?],
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "Z.ai could not be reached."))
    }
}

fn map_quota(root: &serde_json::Value) -> Result<Vec<MetricLine>, ProviderError> {
    let container = root.get("data").unwrap_or(root);
    let limits = container
        .get("limits")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?;
    if limits.is_empty() {
        return Ok(vec![MetricLine::Badge {
            label: "Status".to_owned(),
            text: "No usage data".to_owned(),
            color_hex: None,
            subtitle: None,
        }]);
    }
    let mut lines = Vec::new();
    for limit in limits {
        match text(limit.get("type")).or_else(|| text(limit.get("name"))) {
            Some("TOKENS_LIMIT") => lines.push(token_line(limit)?),
            Some("TIME_LIMIT") => lines.push(web_line(limit)?),
            _ => {}
        }
    }
    if lines.is_empty() {
        return Err(invalid());
    }
    Ok(lines)
}

fn token_line(value: &serde_json::Value) -> Result<MetricLine, ProviderError> {
    let unit = number(value.get("unit")).ok_or_else(invalid)?;
    let count = number(value.get("number"))
        .filter(|count| *count > 0.0)
        .ok_or_else(invalid)?;
    let unit_ms = match unit as u64 {
        3 => 60 * 60 * 1000,
        4 => 24 * 60 * 60 * 1000,
        5 => MONTH_MS,
        6 => 7 * 24 * 60 * 60 * 1000,
        _ => return Err(invalid()),
    };
    let period = (unit_ms as f64 * count).round() as u64;
    let label = if period < 24 * 60 * 60 * 1000 {
        "Session"
    } else {
        "Weekly"
    };
    let used = number(value.get("percentage"))
        .ok_or_else(invalid)?
        .clamp(0.0, 100.0);
    Ok(MetricLine::Progress {
        label: label.to_owned(),
        used,
        limit: 100.0,
        format: MetricFormat::Percent,
        resets_at: timestamp(value.get("nextResetTime")),
        period_duration_ms: Some(period),
        color_hex: None,
    })
}

fn web_line(value: &serde_json::Value) -> Result<MetricLine, ProviderError> {
    let used = number(value.get("currentValue"))
        .filter(|used| *used >= 0.0)
        .ok_or_else(invalid)?;
    let limit = number(value.get("usage"))
        .filter(|limit| *limit >= 0.0)
        .ok_or_else(invalid)?;
    if limit > 0.0 {
        return Ok(MetricLine::Progress {
            label: "Web Searches".to_owned(),
            used,
            limit,
            format: MetricFormat::Count {
                suffix: "searches".to_owned(),
            },
            resets_at: timestamp(value.get("nextResetTime")),
            period_duration_ms: Some(MONTH_MS),
            color_hex: None,
        });
    }
    Ok(MetricLine::Values {
        label: "Web Searches".to_owned(),
        values: vec![MetricValue {
            number: used,
            kind: MetricKind::Count,
            label: Some("searches".to_owned()),
            estimated: false,
        }],
        color_hex: None,
        expiries_at: Vec::new(),
        unknown_models: Vec::new(),
    })
}

fn invalid() -> ProviderError {
    ProviderError::new(ErrorCategory::Decoding, "Z.ai response is invalid.")
}

#[async_trait]
impl ProviderRuntime for ZaiProvider {
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
