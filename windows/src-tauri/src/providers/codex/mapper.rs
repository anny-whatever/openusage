use serde_json::Value;

use crate::contracts::{MetricFormat, MetricKind, MetricLine, MetricValue};
use crate::platform::http::HttpResponse;

use super::super::support::{
    ProviderError, SESSION_PERIOD_MS, WEEK_PERIOD_MS, json_object, number, text, timestamp,
};

const CREDIT_USD_RATE: f64 = 0.04;

pub struct CodexMappedUsage {
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
}

pub fn map_usage(
    response: &HttpResponse,
    reset_credits: Option<&HttpResponse>,
) -> Result<CodexMappedUsage, ProviderError> {
    if !response.status.is_success() {
        return Err(ProviderError::http(response.status));
    }
    let body = json_object(&response.body)?;
    let mut lines = Vec::new();
    if let Some(rate_limit) = body.get("rate_limit") {
        append_windows(rate_limit, "Session", "Weekly", &mut lines);
    }
    append_spark(&body, &mut lines);
    if let Some((count, expiries)) = read_reset_credits(&body, reset_credits) {
        lines.push(MetricLine::Values {
            label: "Rate Limit Resets".to_owned(),
            values: vec![MetricValue {
                number: count as f64,
                kind: MetricKind::Count,
                label: Some("available".to_owned()),
                estimated: false,
            }],
            color_hex: None,
            expiries_at: expiries,
            unknown_models: Vec::new(),
        });
    }
    if let Some(remaining) = read_credits(response, &body) {
        let credits = remaining.max(0.0).floor();
        lines.push(MetricLine::Values {
            label: "Credits".to_owned(),
            values: vec![
                MetricValue {
                    number: credits * CREDIT_USD_RATE,
                    kind: MetricKind::Dollars,
                    label: None,
                    estimated: false,
                },
                MetricValue {
                    number: credits,
                    kind: MetricKind::Count,
                    label: Some("credits".to_owned()),
                    estimated: false,
                },
            ],
            color_hex: None,
            expiries_at: Vec::new(),
            unknown_models: Vec::new(),
        });
    }
    Ok(CodexMappedUsage {
        plan: format_plan(text(body.get("plan_type"))),
        lines,
    })
}

fn append_windows(rate_limit: &Value, session: &str, weekly: &str, lines: &mut Vec<MetricLine>) {
    let primary = rate_limit.get("primary_window");
    let secondary = rate_limit.get("secondary_window");
    let candidates = [
        (primary, session, SESSION_PERIOD_MS),
        (secondary, weekly, WEEK_PERIOD_MS),
    ];
    for (window, fallback_label, fallback_period) in candidates {
        let Some(window) = window else { continue };
        let Some(used) =
            number(window.get("used_percent")).filter(|used| (0.0..=100.0).contains(used))
        else {
            continue;
        };
        let period = number(window.get("limit_window_seconds"))
            .map(|seconds| (seconds * 1000.0) as u64)
            .unwrap_or(fallback_period);
        let label = if period == WEEK_PERIOD_MS {
            weekly
        } else {
            fallback_label
        };
        lines.push(MetricLine::Progress {
            label: label.to_owned(),
            used,
            limit: 100.0,
            format: MetricFormat::Percent,
            resets_at: reset_timestamp(window),
            period_duration_ms: Some(period),
            color_hex: None,
        });
    }
}

fn append_spark(body: &Value, lines: &mut Vec<MetricLine>) {
    let Some(entries) = body.get("additional_rate_limits").and_then(Value::as_array) else {
        return;
    };
    let spark = entries.iter().find(|entry| {
        [
            text(entry.get("limit_name")),
            text(entry.get("metered_feature")),
        ]
        .into_iter()
        .flatten()
        .any(|name| name.to_ascii_lowercase().contains("spark"))
    });
    if let Some(rate_limit) = spark.and_then(|entry| entry.get("rate_limit")) {
        append_windows(rate_limit, "Spark", "Spark Weekly", lines);
    }
}

fn reset_timestamp(window: &Value) -> Option<String> {
    timestamp(window.get("reset_at")).or_else(|| {
        let seconds = number(window.get("reset_after_seconds"))?;
        let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
        Some((now + chrono::Duration::milliseconds((seconds * 1000.0) as i64)).to_rfc3339())
    })
}

fn read_reset_credits(
    usage: &Value,
    dedicated: Option<&HttpResponse>,
) -> Option<(u64, Vec<String>)> {
    let dedicated_body = dedicated
        .filter(|response| response.status.is_success())
        .and_then(|response| serde_json::from_slice::<Value>(&response.body).ok())
        .filter(|body| number(body.get("available_count")).is_some());
    let source = dedicated_body
        .as_ref()
        .or_else(|| usage.get("rate_limit_reset_credits"))?;
    let count = number(source.get("available_count"))?.max(0.0).floor() as u64;
    let mut expiries = source
        .get("credits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|credit| text(credit.get("status")).is_none_or(|status| status == "available"))
        .filter_map(|credit| timestamp(credit.get("expires_at")))
        .collect::<Vec<_>>();
    expiries.sort();
    expiries.truncate(64);
    Some((count, expiries))
}

fn read_credits(response: &HttpResponse, body: &Value) -> Option<f64> {
    let credits = body.get("credits");
    number(credits.and_then(|credits| credits.get("balance")))
        .or_else(|| {
            (credits
                .and_then(|credits| credits.get("has_credits"))
                .and_then(Value::as_bool)
                == Some(false))
            .then_some(0.0)
        })
        .or_else(|| {
            response
                .header("x-codex-credits-balance")
                .and_then(|value| value.parse().ok())
        })
}

fn format_plan(value: Option<&str>) -> Option<String> {
    match value?.to_ascii_lowercase().as_str() {
        "prolite" => Some("Pro 5x".to_owned()),
        "pro" => Some("Pro 20x".to_owned()),
        _ => Some(
            value?
                .split('_')
                .map(|part| {
                    let mut characters = part.chars();
                    characters
                        .next()
                        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
    }
}
