use std::collections::HashMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue};
use serde_json::Value;
use zeroize::Zeroize;

use crate::contracts::{
    ErrorCategory, MetricChartPoint, MetricKind, MetricLine, MetricValue, ProviderSnapshot,
    UsageDay, UsageHistory,
};
use crate::platform::secret::SecretBytes;
use crate::runtime::refresh::ProviderFailure;

pub const SESSION_PERIOD_MS: u64 = 5 * 60 * 60 * 1000;
pub const WEEK_PERIOD_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, thiserror::Error, Eq, PartialEq)]
#[error("{message}")]
pub struct ProviderError {
    pub category: ErrorCategory,
    pub message: String,
}

impl ProviderError {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    pub fn not_logged_in(command: &str) -> Self {
        Self::new(
            ErrorCategory::NotLoggedIn,
            format!("Not logged in. Run `{command}` to authenticate."),
        )
    }

    pub fn http(status: reqwest::StatusCode) -> Self {
        let category = match status.as_u16() {
            429 => ErrorCategory::RateLimited,
            400..=499 => ErrorCategory::Http4xx,
            500..=599 => ErrorCategory::Http5xx,
            _ => ErrorCategory::Other,
        };
        Self::new(category, format!("Provider request failed ({status})."))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input_per_million: f64,
    pub cached_input_per_million: f64,
    pub output_per_million: f64,
}

pub trait PricingCatalog: Send + Sync {
    fn price(&self, model: &str) -> Option<ModelPrice>;
}

#[derive(Debug, Clone, Default)]
pub struct FixedPricing {
    prices: HashMap<String, ModelPrice>,
}

impl FixedPricing {
    pub fn new(prices: HashMap<String, ModelPrice>) -> Self {
        Self {
            prices: prices
                .into_iter()
                .map(|(model, price)| (model.to_ascii_lowercase(), price))
                .collect(),
        }
    }
}

impl PricingCatalog for FixedPricing {
    fn price(&self, model: &str) -> Option<ModelPrice> {
        self.prices.get(&model.to_ascii_lowercase()).copied()
    }
}

pub fn authorization_header(
    secret: &SecretBytes,
) -> Result<(HeaderName, HeaderValue), ProviderError> {
    let mut value = Vec::with_capacity(secret.expose().len() + 7);
    value.extend_from_slice(b"Bearer ");
    value.extend_from_slice(secret.expose());
    let header = HeaderValue::from_bytes(&value).map_err(|_| {
        ProviderError::new(
            ErrorCategory::AuthInvalid,
            "Stored access token is invalid.",
        )
    });
    value.zeroize();
    header.map(|header| (AUTHORIZATION, header))
}

pub fn json_object(bytes: &[u8]) -> Result<Value, ProviderError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        ProviderError::new(
            ErrorCategory::Decoding,
            "Provider returned an invalid response.",
        )
    })?;
    if !value.is_object() {
        return Err(ProviderError::new(
            ErrorCategory::Decoding,
            "Provider returned an invalid response.",
        ));
    }
    Ok(value)
}

pub fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64().filter(|number| number.is_finite()),
        Value::String(text) => text.parse().ok().filter(|number: &f64| number.is_finite()),
        _ => None,
    }
}

pub fn text(value: Option<&Value>) -> Option<&str> {
    value?
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

pub fn timestamp(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|date| date.to_rfc3339()),
        Value::Number(number) => {
            let raw = number.as_f64()?;
            let seconds = if raw.abs() >= 10_000_000_000.0 {
                raw / 1000.0
            } else {
                raw
            };
            DateTime::<Utc>::from_timestamp_millis((seconds * 1000.0) as i64)
                .map(|date| date.to_rfc3339())
        }
        _ => None,
    }
}

pub fn snapshot(
    provider_id: &str,
    display_name: &str,
    plan: Option<String>,
    lines: Vec<MetricLine>,
    usage_history: Option<UsageHistory>,
    warning: Option<String>,
) -> ProviderSnapshot {
    ProviderSnapshot {
        provider_id: provider_id.to_owned(),
        display_name: display_name.to_owned(),
        plan,
        lines,
        refreshed_at: DateTime::<Utc>::from(std::time::SystemTime::now()).to_rfc3339(),
        usage_history,
        warning,
        error_category: None,
    }
}

pub fn provider_failure(error: ProviderError) -> ProviderFailure {
    ProviderFailure {
        category: serde_json::to_value(error.category)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "other".to_owned()),
        message: error.message,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DailyUsage {
    pub tokens: f64,
    pub cost_usd: f64,
    pub has_cost: bool,
}

pub fn history_metrics(
    daily: &HashMap<NaiveDate, DailyUsage>,
    now: DateTime<Utc>,
    note: &str,
) -> (Vec<MetricLine>, UsageHistory) {
    let today = now.date_naive();
    let yesterday = today - Duration::days(1);
    let start = today - Duration::days(30);
    let today_usage = daily.get(&today).cloned().unwrap_or_default();
    let yesterday_usage = daily.get(&yesterday).cloned().unwrap_or_default();
    let last_30 = daily
        .iter()
        .filter(|(date, _)| **date >= start && **date <= today)
        .fold(DailyUsage::default(), |mut total, (_, usage)| {
            total.tokens += usage.tokens;
            total.cost_usd += usage.cost_usd;
            total.has_cost |= usage.has_cost;
            total
        });
    let mut lines = Vec::new();
    append_usage_line(&mut lines, "Today", &today_usage);
    append_usage_line(&mut lines, "Yesterday", &yesterday_usage);
    append_usage_line(&mut lines, "Last 30 Days", &last_30);

    let mut days = (0..=30)
        .rev()
        .map(|offset| today - Duration::days(offset))
        .map(|date| {
            let usage = daily.get(&date).cloned().unwrap_or_default();
            (date, usage)
        })
        .collect::<Vec<_>>();
    let points = days
        .iter()
        .map(|(date, usage)| MetricChartPoint {
            value: usage.tokens,
            label: format!("{} {}", date.format("%b"), date.day()),
            value_label: Some(format!("{} tokens", usage.tokens.round() as u64)),
        })
        .collect();
    lines.push(MetricLine::Chart {
        label: "Usage Trend".to_owned(),
        points,
        note: Some(note.to_owned()),
    });
    let history = UsageHistory {
        days: days
            .drain(..)
            .map(|(date, usage)| UsageDay {
                date: date.format("%Y-%m-%d").to_string(),
                value: usage.tokens,
            })
            .collect(),
    };
    (lines, history)
}

fn append_usage_line(lines: &mut Vec<MetricLine>, label: &str, usage: &DailyUsage) {
    let mut values = Vec::with_capacity(2);
    if usage.has_cost {
        values.push(MetricValue {
            number: usage.cost_usd,
            kind: MetricKind::Dollars,
            label: None,
            estimated: true,
        });
    }
    values.push(MetricValue {
        number: usage.tokens,
        kind: MetricKind::Count,
        label: Some("tokens".to_owned()),
        estimated: false,
    });
    lines.push(MetricLine::Values {
        label: label.to_owned(),
        values,
        color_hex: None,
        expiries_at: Vec::new(),
        unknown_models: Vec::new(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_pricing_normalizes_model_keys_once() {
        let expected = ModelPrice {
            input_per_million: 1.0,
            cached_input_per_million: 0.1,
            output_per_million: 2.0,
        };
        let pricing = FixedPricing::new(HashMap::from([("Mixed-Case".to_owned(), expected)]));

        let actual = pricing.price("mixed-case").expect("normalized price");

        assert_eq!(actual.input_per_million, expected.input_per_million);
        assert_eq!(actual.output_per_million, expected.output_per_million);
    }
}
