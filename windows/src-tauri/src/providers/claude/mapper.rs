use reqwest::StatusCode;
use serde_json::Value;

use crate::contracts::{MetricFormat, MetricKind, MetricLine, MetricValue};
use crate::platform::http::HttpResponse;

use super::super::support::{
    ProviderError, SESSION_PERIOD_MS, WEEK_PERIOD_MS, json_object, number, text, timestamp,
};
use super::auth::ClaudeCredentials;

pub struct ClaudeMappedUsage {
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
    pub warning: Option<String>,
}

pub fn map_usage(
    response: &HttpResponse,
    credentials: &ClaudeCredentials,
) -> Result<ClaudeMappedUsage, ProviderError> {
    if !response.status.is_success() {
        return Err(ProviderError::http(response.status));
    }
    let body = json_object(&response.body)?;
    let mut lines = Vec::new();
    append_window(&body, "five_hour", "Session", SESSION_PERIOD_MS, &mut lines);
    append_window(&body, "seven_day", "Weekly", WEEK_PERIOD_MS, &mut lines);
    append_window(
        &body,
        "seven_day_sonnet",
        "Sonnet",
        WEEK_PERIOD_MS,
        &mut lines,
    );
    append_scoped_weekly(&body, "Fable", &mut lines);
    append_extra_usage(&body, &mut lines);
    Ok(ClaudeMappedUsage {
        plan: plan(credentials),
        lines,
        warning: None,
    })
}

pub fn rate_limited(response: &HttpResponse, credentials: &ClaudeCredentials) -> ClaudeMappedUsage {
    let retry = response
        .header("retry-after")
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| format!("{}m", seconds.div_ceil(60)));
    let status = retry
        .as_ref()
        .map(|retry| format!("Rate limited, retry in ~{retry}"))
        .unwrap_or_else(|| "Rate limited, try again later".to_owned());
    let warning = retry
        .map(|retry| format!("Updates blocked by Anthropic. Retrying in ~{retry}."))
        .unwrap_or_else(|| "Updates blocked by Anthropic. Try again later.".to_owned());
    ClaudeMappedUsage {
        plan: plan(credentials),
        lines: vec![MetricLine::Badge {
            label: "Status".to_owned(),
            text: status,
            color_hex: Some("#F59E0B".to_owned()),
            subtitle: None,
        }],
        warning: Some(warning),
    }
}

pub fn is_auth_rejection(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
}

fn append_window(
    body: &Value,
    key: &str,
    label: &str,
    period_duration_ms: u64,
    lines: &mut Vec<MetricLine>,
) {
    let Some(window) = body.get(key).and_then(Value::as_object) else {
        return;
    };
    let Some(used) = number(window.get("utilization")) else {
        return;
    };
    if !(0.0..=100.0).contains(&used) {
        return;
    }
    lines.push(MetricLine::Progress {
        label: label.to_owned(),
        used,
        limit: 100.0,
        format: MetricFormat::Percent,
        resets_at: timestamp(window.get("resets_at")),
        period_duration_ms: Some(period_duration_ms),
        color_hex: None,
    });
}

fn append_scoped_weekly(body: &Value, model_name: &str, lines: &mut Vec<MetricLine>) {
    let Some(limits) = body.get("limits").and_then(Value::as_array) else {
        return;
    };
    let matching = limits.iter().find(|entry| {
        text(entry.get("kind")) == Some("weekly_scoped")
            && text(
                entry
                    .get("scope")
                    .and_then(|scope| scope.get("model"))
                    .and_then(|model| model.get("display_name")),
            ) == Some(model_name)
    });
    let Some(limit) = matching else { return };
    let Some(used) = number(limit.get("percent")).filter(|used| (0.0..=100.0).contains(used))
    else {
        return;
    };
    lines.push(MetricLine::Progress {
        label: model_name.to_owned(),
        used,
        limit: 100.0,
        format: MetricFormat::Percent,
        resets_at: timestamp(limit.get("resets_at")),
        period_duration_ms: Some(WEEK_PERIOD_MS),
        color_hex: None,
    });
}

fn append_extra_usage(body: &Value, lines: &mut Vec<MetricLine>) {
    let Some(extra) = body.get("extra_usage").and_then(Value::as_object) else {
        return;
    };
    if extra.get("is_enabled").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let Some(used) = number(extra.get("used_credits")).map(|cents| cents / 100.0) else {
        return;
    };
    let limit = number(extra.get("monthly_limit")).map(|cents| cents / 100.0);
    if limit.is_some_and(|limit| limit > 0.0 && used <= limit) {
        lines.push(MetricLine::Progress {
            label: "Extra usage spent".to_owned(),
            used,
            limit: limit.expect("positive limit checked above"),
            format: MetricFormat::Dollars,
            resets_at: None,
            period_duration_ms: None,
            color_hex: None,
        });
    } else {
        lines.push(MetricLine::Values {
            label: "Extra usage spent".to_owned(),
            values: vec![MetricValue {
                number: used,
                kind: MetricKind::Dollars,
                label: None,
                estimated: false,
            }],
            color_hex: None,
            expiries_at: Vec::new(),
            unknown_models: Vec::new(),
        });
    }
}

fn plan(credentials: &ClaudeCredentials) -> Option<String> {
    let subscription = credentials.subscription_type.as_deref()?.trim();
    if subscription.is_empty() {
        return None;
    }
    let mut plan = title_case(subscription);
    if let Some(tier) = &credentials.rate_limit_tier
        && let Some(multiplier) = tier
            .split(|character: char| !character.is_ascii_alphanumeric())
            .find(|part| {
                part.ends_with('x')
                    && part[..part.len() - 1]
                        .bytes()
                        .all(|byte| byte.is_ascii_digit())
            })
    {
        plan.push(' ');
        plan.push_str(multiplier);
    }
    Some(plan)
}

fn title_case(value: &str) -> String {
    value
        .split([' ', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
