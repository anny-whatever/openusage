use serde_json::Value;

use crate::contracts::{MetricFormat, MetricKind, MetricLine, MetricValue};
use crate::platform::http::HttpResponse;

use super::super::support::{ProviderError, json_object, number, text, timestamp};

const BILLING_PERIOD_MS: u64 = 30 * 24 * 60 * 60 * 1000;

pub struct CursorMappedUsage {
    pub plan: Option<String>,
    pub lines: Vec<MetricLine>,
}

pub fn map_usage(
    usage_response: &HttpResponse,
    plan_response: Option<&HttpResponse>,
    credits_response: Option<&HttpResponse>,
) -> Result<CursorMappedUsage, ProviderError> {
    if !usage_response.status.is_success() {
        return Err(ProviderError::http(usage_response.status));
    }
    let usage = json_object(&usage_response.body)?;
    if usage.get("enabled").and_then(Value::as_bool) == Some(false) {
        return Err(ProviderError::new(
            crate::contracts::ErrorCategory::NotAvailable,
            "No active Cursor subscription.",
        ));
    }
    let plan_usage = usage
        .get("planUsage")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ProviderError::new(
                crate::contracts::ErrorCategory::NotAvailable,
                "Cursor usage limits are unavailable.",
            )
        })?;
    let plan = plan_name(plan_response);
    let plan_limit = number(plan_usage.get("limit"));
    let reported_percent = number(plan_usage.get("totalPercentUsed"));
    if plan_limit.is_none() && reported_percent.is_none() {
        return Err(ProviderError::new(
            crate::contracts::ErrorCategory::Decoding,
            "Cursor total usage limit is missing.",
        ));
    }
    let cycle = billing_cycle(&usage);
    let plan_spent_cents = number(plan_usage.get("totalSpend")).unwrap_or_else(|| {
        let limit = plan_limit.unwrap_or(0.0);
        let remaining = number(plan_usage.get("remaining")).unwrap_or(0.0);
        (limit - remaining).max(0.0)
    });
    let computed_percent = plan_limit
        .filter(|limit| *limit > 0.0)
        .map(|limit| plan_spent_cents / limit * 100.0)
        .unwrap_or(0.0);
    let mut lines = Vec::new();
    append_credits(credits_response, &mut lines);

    let team = plan
        .as_deref()
        .is_some_and(|plan| plan.eq_ignore_ascii_case("team"))
        || usage
            .get("spendLimitUsage")
            .and_then(|spend| text(spend.get("limitType")))
            .is_some_and(|kind| kind.eq_ignore_ascii_case("team"));
    if team {
        let limit = plan_limit.filter(|limit| *limit > 0.0).ok_or_else(|| {
            ProviderError::new(
                crate::contracts::ErrorCategory::NotAvailable,
                "Cursor team usage is unavailable.",
            )
        })?;
        lines.push(progress(
            "Total usage",
            plan_spent_cents / 100.0,
            limit / 100.0,
            MetricFormat::Dollars,
            cycle.clone(),
        ));
    } else {
        lines.push(progress(
            "Total usage",
            reported_percent.unwrap_or(computed_percent),
            100.0,
            MetricFormat::Percent,
            cycle.clone(),
        ));
    }
    for (key, label) in [
        ("autoPercentUsed", "Auto usage"),
        ("apiPercentUsed", "API usage"),
    ] {
        if let Some(used) = number(plan_usage.get(key)) {
            lines.push(progress(
                label,
                used,
                100.0,
                MetricFormat::Percent,
                cycle.clone(),
            ));
        }
    }
    append_on_demand(&usage, &mut lines);
    Ok(CursorMappedUsage { plan, lines })
}

fn progress(
    label: &str,
    used: f64,
    limit: f64,
    format: MetricFormat,
    cycle: (Option<String>, u64),
) -> MetricLine {
    MetricLine::Progress {
        label: label.to_owned(),
        used,
        limit,
        format,
        resets_at: cycle.0,
        period_duration_ms: Some(cycle.1),
        color_hex: None,
    }
}

fn billing_cycle(usage: &Value) -> (Option<String>, u64) {
    let start = number(usage.get("billingCycleStart"));
    let end = number(usage.get("billingCycleEnd"));
    let duration = start
        .zip(end)
        .filter(|(start, end)| end > start)
        .map(|(start, end)| (end - start) as u64)
        .unwrap_or(BILLING_PERIOD_MS);
    (timestamp(usage.get("billingCycleEnd")), duration)
}

fn plan_name(response: Option<&HttpResponse>) -> Option<String> {
    let body = response
        .filter(|response| response.status.is_success())
        .and_then(|response| serde_json::from_slice::<Value>(&response.body).ok())?;
    let name = text(body.get("planInfo").and_then(|plan| plan.get("planName")))?;
    Some(title_case(name))
}

fn append_credits(response: Option<&HttpResponse>, lines: &mut Vec<MetricLine>) {
    let Some(body) = response
        .filter(|response| response.status.is_success())
        .and_then(|response| serde_json::from_slice::<Value>(&response.body).ok())
    else {
        return;
    };
    if body.get("hasCreditGrants").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let total = number(body.get("totalCents")).unwrap_or(0.0);
    let used = number(body.get("usedCents")).unwrap_or(0.0);
    if total <= 0.0 {
        return;
    }
    lines.push(MetricLine::Values {
        label: "Credits".to_owned(),
        values: vec![MetricValue {
            number: (total - used).max(0.0) / 100.0,
            kind: MetricKind::Dollars,
            label: None,
            estimated: false,
        }],
        color_hex: None,
        expiries_at: Vec::new(),
        unknown_models: Vec::new(),
    });
}

fn append_on_demand(usage: &Value, lines: &mut Vec<MetricLine>) {
    let Some(spend) = usage.get("spendLimitUsage") else {
        return;
    };
    let limit = number(spend.get("individualLimit"))
        .or_else(|| number(spend.get("pooledLimit")))
        .unwrap_or(0.0);
    let remaining = number(spend.get("individualRemaining"))
        .or_else(|| number(spend.get("pooledRemaining")))
        .unwrap_or(0.0);
    let spent = number(spend.get("individualUsed"))
        .or_else(|| number(spend.get("pooledUsed")))
        .or_else(|| number(spend.get("totalSpend")))
        .filter(|spent| *spent > 0.0)
        .unwrap_or_else(|| (limit - remaining).max(0.0));
    if limit > 0.0 {
        lines.push(progress(
            "On-demand",
            spent / 100.0,
            limit / 100.0,
            MetricFormat::Dollars,
            (None, BILLING_PERIOD_MS),
        ));
    } else if spent > 0.0 {
        lines.push(MetricLine::Values {
            label: "On-demand".to_owned(),
            values: vec![MetricValue {
                number: spent / 100.0,
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

fn title_case(value: &str) -> String {
    value
        .split_whitespace()
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}
