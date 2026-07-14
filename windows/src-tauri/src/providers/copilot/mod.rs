mod auth;

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

use crate::contracts::{
    ErrorCategory, MetricFormat, MetricKind, MetricLine, MetricValue, ProviderSnapshot,
};
use crate::platform::http::{HttpRequest, HttpTransport};
use crate::platform::secret::SecretBytes;
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

pub use auth::CopilotAuthStore;

use super::support::{ProviderError, number, provider_failure, snapshot, text, timestamp};

const PROVIDER_ID: &str = "copilot";
const DISPLAY_NAME: &str = "Copilot";
const USAGE_URL: &str = "https://api.github.com/copilot_internal/user";
const MONTH_MS: u64 = 30 * 24 * 60 * 60 * 1000;

pub struct CopilotProvider {
    auth: CopilotAuthStore,
    http: Arc<dyn HttpTransport>,
}

impl CopilotProvider {
    pub fn new(auth: CopilotAuthStore, http: Arc<dyn HttpTransport>) -> Self {
        Self { auth, http }
    }
    pub async fn has_local_credentials(&self) -> bool {
        self.auth.has_usable_credentials().await
    }

    async fn refresh_provider(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderError> {
        let token = self.auth.load().await?;
        let response = self
            .github_get(
                Url::parse(USAGE_URL).expect("static Copilot URL"),
                &token,
                cancellation.child_token(),
            )
            .await?;
        if response.status == StatusCode::UNAUTHORIZED || response.status == StatusCode::FORBIDDEN {
            return Err(ProviderError::new(
                ErrorCategory::AuthInvalid,
                "GitHub token is invalid or expired.",
            ));
        }
        if !response.status.is_success() {
            return Err(ProviderError::http(response.status));
        }
        let body: serde_json::Value = serde_json::from_slice(&response.body).map_err(|_| {
            ProviderError::new(ErrorCategory::Decoding, "Copilot response is invalid.")
        })?;
        let (plan, mut lines, org_managed) = map_usage(&body)?;
        if org_managed {
            lines = self
                .org_billing_lines(&token, cancellation.child_token())
                .await;
        }
        let warning = (org_managed && lines.is_empty())
            .then(|| "Organization usage requires a GitHub billing role.".to_owned());
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            plan,
            lines,
            None,
            warning,
        ))
    }

    async fn org_billing_lines(
        &self,
        token: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Vec<MetricLine> {
        let orgs_url =
            Url::parse("https://api.github.com/user/orgs?per_page=100").expect("static GitHub URL");
        let Ok(response) = self
            .github_get(orgs_url, token, cancellation.child_token())
            .await
        else {
            return Vec::new();
        };
        if !response.status.is_success() {
            return Vec::new();
        }
        let Ok(orgs) = serde_json::from_slice::<Vec<serde_json::Value>>(&response.body) else {
            return Vec::new();
        };
        for org in orgs.into_iter().take(100) {
            let Some(login) = text(org.get("login")).filter(|login| {
                login.len() <= 100
                    && login
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }) else {
                continue;
            };
            let mut url = Url::parse("https://api.github.com").expect("static GitHub URL");
            url.path_segments_mut()
                .expect("GitHub URL supports path segments")
                .extend(["orgs", login, "settings", "billing", "usage", "summary"]);
            let Ok(response) = self
                .github_get(url, token, cancellation.child_token())
                .await
            else {
                continue;
            };
            if response.status == StatusCode::FORBIDDEN || response.status == StatusCode::NOT_FOUND
            {
                continue;
            }
            if response.status == StatusCode::TOO_MANY_REQUESTS || response.status.is_server_error()
            {
                return Vec::new();
            }
            if response.status.is_success()
                && let Some(lines) = map_org_billing(&response.body)
            {
                return lines;
            }
        }
        Vec::new()
    }

    async fn github_get(
        &self,
        url: Url,
        token: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<crate::platform::http::HttpResponse, ProviderError> {
        let mut header = Vec::with_capacity(token.expose().len() + 6);
        header.extend_from_slice(b"token ");
        header.extend_from_slice(token.expose());
        let authorization = reqwest::header::HeaderValue::from_bytes(&header).map_err(|_| {
            ProviderError::new(ErrorCategory::AuthInvalid, "Copilot token is invalid.")
        });
        header.zeroize();
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url,
                    headers: vec![
                        (reqwest::header::AUTHORIZATION, authorization?),
                        (
                            reqwest::header::ACCEPT,
                            reqwest::header::HeaderValue::from_static("application/json"),
                        ),
                    ],
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "GitHub could not be reached."))
    }
}

fn map_usage(
    body: &serde_json::Value,
) -> Result<(Option<String>, Vec<MetricLine>, bool), ProviderError> {
    let plan = text(body.get("copilot_plan")).map(title_case);
    let reset = timestamp(body.get("quota_reset_date"))
        .or_else(|| day_timestamp(body.get("limited_user_reset_date")));
    let snapshots = body.get("quota_snapshots");
    let mut lines = Vec::new();
    if let Some(line) = snapshot_line(
        "Credits",
        snapshots.and_then(|value| value.get("premium_interactions")),
        reset.clone(),
    ) {
        lines.push(line);
        if let Some(extra) =
            overage_line(snapshots.and_then(|value| value.get("premium_interactions")))
        {
            lines.push(extra);
        }
    }
    for (field, label) in [("chat", "Chat"), ("completions", "Completions")] {
        if let Some(line) = snapshot_line(
            label,
            snapshots.and_then(|value| value.get(field)),
            reset.clone(),
        ) {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        for (field, label) in [("chat", "Chat"), ("completions", "Completions")] {
            if let Some(line) = limited_line(
                label,
                body.get("limited_user_quotas").and_then(|v| v.get(field)),
                body.get("monthly_quotas").and_then(|v| v.get(field)),
                reset.clone(),
            ) {
                lines.push(line);
            }
        }
    }
    let org_managed = body
        .get("token_based_billing")
        .and_then(serde_json::Value::as_bool)
        == Some(true);
    if lines.is_empty() && !org_managed {
        return Err(ProviderError::new(
            ErrorCategory::NotAvailable,
            "Copilot usage is unavailable for this account.",
        ));
    }
    Ok((plan, lines, org_managed))
}

fn map_org_billing(contents: &[u8]) -> Option<Vec<MetricLine>> {
    let body: serde_json::Value = serde_json::from_slice(contents).ok()?;
    let items = body.get("usageItems")?.as_array()?;
    let matching = items
        .iter()
        .filter(|item| {
            text(item.get("product")).is_some_and(|product| product.eq_ignore_ascii_case("copilot"))
                && text(item.get("unitType")).is_some_and(|unit| {
                    unit.eq_ignore_ascii_case("ai-units") || unit.eq_ignore_ascii_case("ai-credits")
                })
        })
        .collect::<Vec<_>>();
    if matching.is_empty() {
        return None;
    }
    let credits = matching
        .iter()
        .map(|item| number(item.get("grossQuantity")).unwrap_or(0.0).max(0.0))
        .sum();
    let spend = matching
        .iter()
        .map(|item| number(item.get("netAmount")).unwrap_or(0.0).max(0.0))
        .sum();
    Some(vec![
        MetricLine::Values {
            label: "Org Credits".to_owned(),
            values: vec![MetricValue {
                number: credits,
                kind: MetricKind::Count,
                label: Some("credits".to_owned()),
                estimated: false,
            }],
            color_hex: None,
            expiries_at: Vec::new(),
            unknown_models: Vec::new(),
        },
        MetricLine::Values {
            label: "Org Spend".to_owned(),
            values: vec![MetricValue {
                number: spend,
                kind: MetricKind::Dollars,
                label: None,
                estimated: false,
            }],
            color_hex: None,
            expiries_at: Vec::new(),
            unknown_models: Vec::new(),
        },
    ])
}

fn snapshot_line(
    label: &str,
    value: Option<&serde_json::Value>,
    resets_at: Option<String>,
) -> Option<MetricLine> {
    let value = value?;
    let entitlement = number(value.get("entitlement"));
    let remaining = number(value.get("remaining"));
    if value.get("unlimited").and_then(serde_json::Value::as_bool) == Some(true)
        || entitlement == Some(-1.0)
        || remaining == Some(-1.0)
        || entitlement == Some(0.0)
    {
        return None;
    }
    let used = if let Some(percent_remaining) = number(value.get("percent_remaining")) {
        (100.0 - percent_remaining).clamp(0.0, 100.0)
    } else {
        let total = entitlement.filter(|total| *total > 0.0)?;
        (100.0 - remaining? / total * 100.0).clamp(0.0, 100.0)
    };
    Some(percent_line(label, used, resets_at))
}

fn overage_line(value: Option<&serde_json::Value>) -> Option<MetricLine> {
    let value = value?;
    if value
        .get("overage_permitted")
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        return None;
    }
    Some(MetricLine::Values {
        label: "Extra Usage".to_owned(),
        values: vec![MetricValue {
            number: number(value.get("overage_count")).unwrap_or(0.0).max(0.0),
            kind: MetricKind::Count,
            label: None,
            estimated: false,
        }],
        color_hex: None,
        expiries_at: Vec::new(),
        unknown_models: Vec::new(),
    })
}

fn limited_line(
    label: &str,
    remaining: Option<&serde_json::Value>,
    total: Option<&serde_json::Value>,
    resets_at: Option<String>,
) -> Option<MetricLine> {
    let total = number(total).filter(|value| *value > 0.0)?;
    let used = ((total - number(remaining)?).max(0.0) / total * 100.0).clamp(0.0, 100.0);
    Some(percent_line(label, used, resets_at))
}

fn percent_line(label: &str, used: f64, resets_at: Option<String>) -> MetricLine {
    MetricLine::Progress {
        label: label.to_owned(),
        used,
        limit: 100.0,
        format: MetricFormat::Percent,
        resets_at,
        period_duration_ms: Some(MONTH_MS),
        color_hex: None,
    }
}

fn day_timestamp(value: Option<&serde_json::Value>) -> Option<String> {
    let date = text(value)?;
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)
        .map(|date| date.and_utc().to_rfc3339())
}

fn title_case(value: &str) -> String {
    value
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| {
                    first.to_uppercase().collect::<String>() + &chars.as_str().to_ascii_lowercase()
                })
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[async_trait]
impl ProviderRuntime for CopilotProvider {
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
