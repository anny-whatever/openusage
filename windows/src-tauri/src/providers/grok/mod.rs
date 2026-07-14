mod auth;
mod history;

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Method, StatusCode, Url};
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;

use crate::contracts::{ErrorCategory, MetricFormat, MetricLine, ProviderSnapshot};
use crate::platform::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::platform::secret::SecretBytes;
use crate::runtime::refresh::{ProviderFailure, ProviderRuntime};

pub use auth::GrokAuthStore;

use self::auth::GrokCredential;
use self::history::GrokHistoryScanner;
use super::support::{
    PricingCatalog, ProviderError, authorization_header, history_metrics, json_object, number,
    provider_failure, snapshot, text,
};

const PROVIDER_ID: &str = "grok";
const DISPLAY_NAME: &str = "Grok";
const CREDITS_URL: &str = "https://cli-chat-proxy.grok.com/v1/billing?format=credits";
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const REFRESH_URL: &str = "https://auth.x.ai/oauth2/token";

pub struct GrokProvider {
    auth: GrokAuthStore,
    http: Arc<dyn HttpTransport>,
    history: GrokHistoryScanner,
    pricing: Arc<dyn PricingCatalog>,
}

impl GrokProvider {
    pub fn new(
        auth: GrokAuthStore,
        http: Arc<dyn HttpTransport>,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Self {
        Self {
            auth,
            http,
            history: GrokHistoryScanner::default(),
            pricing,
        }
    }

    pub async fn has_local_credentials(&self) -> bool {
        self.auth.load_all().await.is_ok()
    }

    async fn refresh_provider(
        &self,
        cancellation: CancellationToken,
    ) -> Result<ProviderSnapshot, ProviderError> {
        let candidates = self.auth.load_all().await?;
        let history = self
            .history
            .scan(self.auth.log_path(), self.pricing.clone());
        let live = self.refresh_candidates(candidates, cancellation);
        let (live, history) = tokio::join!(live, history);
        let (plan, mut lines) = live?;
        let mut usage_history = None;
        let warning = match history {
            Ok(daily) if !daily.is_empty() => {
                let (history_lines, normalized) = history_metrics(
                    &daily,
                    chrono::DateTime::from(std::time::SystemTime::now()),
                    "From your Grok logs (estimated).",
                );
                lines.extend(history_lines);
                usage_history = Some(normalized);
                None
            }
            Ok(_) => None,
            Err(_) => Some("Local Grok history is temporarily unavailable.".to_owned()),
        };
        Ok(snapshot(
            PROVIDER_ID,
            DISPLAY_NAME,
            plan,
            lines,
            usage_history,
            warning,
        ))
    }

    async fn refresh_candidates(
        &self,
        candidates: Vec<GrokCredential>,
        cancellation: CancellationToken,
    ) -> Result<(Option<String>, Vec<MetricLine>), ProviderError> {
        let mut last_error = None;
        for credential in candidates {
            match self
                .refresh_candidate(credential, cancellation.child_token())
                .await
            {
                Ok(result) => return Ok(result),
                Err(error)
                    if matches!(
                        error.category,
                        ErrorCategory::AuthExpired | ErrorCategory::AuthInvalid
                    ) =>
                {
                    last_error = Some(error)
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or_else(|| ProviderError::not_logged_in("grok login")))
    }

    async fn refresh_candidate(
        &self,
        credential: GrokCredential,
        cancellation: CancellationToken,
    ) -> Result<(Option<String>, Vec<MetricLine>), ProviderError> {
        let mut access =
            if credential.needs_refresh(chrono::DateTime::from(std::time::SystemTime::now())) {
                self.rotate(&credential, cancellation.child_token()).await?
            } else {
                SecretBytes::new(credential.access.expose().to_vec())
            };
        let mut credits = self
            .get(CREDITS_URL, &access, cancellation.child_token())
            .await?;
        if auth_rejection(credits.status) {
            access = self.rotate(&credential, cancellation.child_token()).await?;
            credits = self
                .get(CREDITS_URL, &access, cancellation.child_token())
                .await?;
        }
        if !credits.status.is_success() {
            return Err(ProviderError::http(credits.status));
        }
        let lines = map_credits(&credits.body)?;
        let plan = self
            .get(SETTINGS_URL, &access, cancellation.child_token())
            .await
            .ok()
            .filter(|response| response.status.is_success())
            .and_then(|response| json_object(&response.body).ok())
            .and_then(|body| text(body.get("subscription_tier_display")).map(str::to_owned));
        Ok((plan, lines))
    }

    async fn get(
        &self,
        url: &str,
        access: &SecretBytes,
        cancellation: CancellationToken,
    ) -> Result<HttpResponse, ProviderError> {
        self.http
            .send(
                HttpRequest {
                    method: Method::GET,
                    url: Url::parse(url).expect("static Grok URL"),
                    headers: vec![
                        authorization_header(access)?,
                        (
                            reqwest::header::HeaderName::from_static("x-xai-token-auth"),
                            reqwest::header::HeaderValue::from_static("xai-grok-cli"),
                        ),
                    ],
                    body: None,
                },
                cancellation,
            )
            .await
            .map_err(|_| ProviderError::new(ErrorCategory::Network, "Grok could not be reached."))
    }

    async fn rotate(
        &self,
        credential: &GrokCredential,
        cancellation: CancellationToken,
    ) -> Result<SecretBytes, ProviderError> {
        let refresh = credential
            .refresh
            .as_ref()
            .ok_or_else(|| ProviderError::new(ErrorCategory::AuthExpired, "Grok login expired."))?;
        let refresh = std::str::from_utf8(refresh.expose()).map_err(|_| {
            ProviderError::new(ErrorCategory::AuthInvalid, "Grok refresh token is invalid.")
        })?;
        let body = form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "refresh_token")
            .append_pair("client_id", &credential.client_id)
            .append_pair("refresh_token", refresh)
            .finish()
            .into_bytes();
        let response = self
            .http
            .send(
                HttpRequest {
                    method: Method::POST,
                    url: Url::parse(REFRESH_URL).expect("static Grok URL"),
                    headers: vec![(
                        reqwest::header::CONTENT_TYPE,
                        reqwest::header::HeaderValue::from_static(
                            "application/x-www-form-urlencoded",
                        ),
                    )],
                    body: Some(body),
                },
                cancellation,
            )
            .await
            .map_err(|_| {
                ProviderError::new(ErrorCategory::Network, "Grok token refresh failed.")
            })?;
        if !response.status.is_success() {
            return Err(ProviderError::new(
                ErrorCategory::AuthExpired,
                "Grok login expired.",
            ));
        }
        let body = json_object(&response.body)?;
        let access = text(body.get("access_token")).ok_or_else(|| {
            ProviderError::new(ErrorCategory::Decoding, "Grok refresh response is invalid.")
        })?;
        let refreshed = SecretBytes::new(access.as_bytes().to_vec());
        let refresh = text(body.get("refresh_token"))
            .map(|value| SecretBytes::new(value.as_bytes().to_vec()));
        let expires_at = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
            + chrono::Duration::seconds(
                number(body.get("expires_in")).unwrap_or(3600.0).max(1.0) as i64
            );
        self.auth
            .save_refreshed(credential, &refreshed, refresh.as_ref(), expires_at)
            .await?;
        Ok(refreshed)
    }
}

fn map_credits(contents: &[u8]) -> Result<Vec<MetricLine>, ProviderError> {
    let body = json_object(contents)?;
    let config = body.get("config").ok_or_else(invalid_response)?;
    let period = config.get("currentPeriod").ok_or_else(invalid_response)?;
    let period_type = text(period.get("type")).ok_or_else(invalid_response)?;
    let start = text(period.get("start"))
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .ok_or_else(invalid_response)?;
    let end = text(period.get("end"))
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .ok_or_else(invalid_response)?;
    if end <= start {
        return Err(invalid_response());
    }
    let mut lines = Vec::new();
    if period_type == "USAGE_PERIOD_TYPE_WEEKLY" {
        lines.push(MetricLine::Progress {
            label: "Weekly limit".to_owned(),
            used: number(config.get("creditUsagePercent"))
                .unwrap_or(0.0)
                .clamp(0.0, 100.0),
            limit: 100.0,
            format: MetricFormat::Percent,
            resets_at: Some(end.to_rfc3339()),
            period_duration_ms: Some((end.timestamp_millis() - start.timestamp_millis()) as u64),
            color_hex: None,
        });
    }
    let cap = number(config.get("onDemandCap").and_then(|value| value.get("val"))).unwrap_or(0.0);
    lines.push(MetricLine::Badge {
        label: "Pay as you go".to_owned(),
        text: if cap > 0.0 {
            format!("{} cap", format_number(cap))
        } else {
            "Disabled".to_owned()
        },
        color_hex: Some(if cap > 0.0 { "#22C55E" } else { "#A3A3A3" }.to_owned()),
        subtitle: None,
    });
    Ok(lines)
}

fn auth_rejection(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
}
fn invalid_response() -> ProviderError {
    ProviderError::new(ErrorCategory::Decoding, "Grok billing response is invalid.")
}
fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[async_trait]
impl ProviderRuntime for GrokProvider {
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
