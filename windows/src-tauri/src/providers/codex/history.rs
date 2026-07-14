use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;

use super::super::history::{HistoryError, IncrementalJsonlCache, discover_jsonl};
use super::super::support::{DailyUsage, PricingCatalog, number, text};

#[derive(Debug, Clone)]
struct CodexHistoryEntry {
    date: NaiveDate,
    model: String,
    input_tokens: f64,
    cached_input_tokens: f64,
    output_tokens: f64,
    fast: bool,
}

#[derive(Clone, Default)]
pub struct CodexHistoryScanner {
    cache: IncrementalJsonlCache<CodexHistoryEntry>,
}

impl CodexHistoryScanner {
    pub async fn scan(
        &self,
        roots: Vec<PathBuf>,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Result<HashMap<NaiveDate, DailyUsage>, HistoryError> {
        let files = tokio::task::spawn_blocking(move || discover_codex_files(&roots))
            .await
            .map_err(|_| HistoryError::TaskFailed)??;
        let entries = self.cache.scan(files, parse_file).await?;
        Ok(aggregate(entries, pricing.as_ref()))
    }
}

fn discover_codex_files(roots: &[PathBuf]) -> Result<Vec<PathBuf>, HistoryError> {
    let mut files = Vec::new();
    for root in roots {
        let active_root = root.join("sessions");
        let active = discover_jsonl(std::slice::from_ref(&active_root))?;
        let active_relative = active
            .iter()
            .filter_map(|path| path.strip_prefix(&active_root).ok().map(Path::to_owned))
            .collect::<std::collections::HashSet<_>>();
        files.extend(active);
        let archived_root = root.join("archived_sessions");
        files.extend(
            discover_jsonl(std::slice::from_ref(&archived_root))?
                .into_iter()
                .filter(|path| {
                    path.strip_prefix(&archived_root)
                        .ok()
                        .is_none_or(|relative| !active_relative.contains(relative))
                }),
        );
    }
    files.sort();
    Ok(files)
}

fn parse_file(contents: &[u8]) -> Vec<CodexHistoryEntry> {
    let mut entries = Vec::new();
    let mut current_model = "gpt-5".to_owned();
    let mut fast = false;
    let mut previous_totals: Option<RawUsage> = None;
    for line in contents.split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let payload = value.get("payload");
        if text(value.get("type")) == Some("turn_context") {
            if let Some(model) = model_name(payload) {
                current_model = model.to_owned();
            }
            continue;
        }
        if text(payload.and_then(|payload| payload.get("type"))) == Some("thread_settings_applied")
        {
            let tier = text(
                payload
                    .and_then(|payload| payload.get("thread_settings"))
                    .and_then(|settings| settings.get("service_tier")),
            );
            fast = matches!(tier, Some("fast" | "priority"));
            continue;
        }
        if text(value.get("type")) != Some("event_msg")
            || text(payload.and_then(|payload| payload.get("type"))) != Some("token_count")
        {
            continue;
        }
        let Some(date) = text(value.get("timestamp"))
            .and_then(|timestamp| DateTime::parse_from_rfc3339(timestamp).ok())
            .map(|date| date.with_timezone(&Utc).date_naive())
        else {
            continue;
        };
        let info = payload.and_then(|payload| payload.get("info"));
        let totals = info
            .and_then(|info| info.get("total_token_usage"))
            .map(RawUsage::from_value);
        let usage = info
            .and_then(|info| info.get("last_token_usage"))
            .map(RawUsage::from_value)
            .or_else(|| totals.map(|totals| totals.subtract(previous_totals)))
            .unwrap_or_default();
        if let Some(totals) = totals {
            previous_totals = Some(totals);
        }
        if usage.total() <= 0.0 {
            continue;
        }
        if let Some(model) = model_name(payload).or_else(|| model_name(info)) {
            current_model = model.to_owned();
        }
        entries.push(CodexHistoryEntry {
            date,
            model: current_model.clone(),
            input_tokens: usage.input,
            cached_input_tokens: usage.cached.min(usage.input),
            output_tokens: usage.output + usage.reasoning,
            fast,
        });
    }
    entries
}

#[derive(Debug, Clone, Copy, Default)]
struct RawUsage {
    input: f64,
    cached: f64,
    output: f64,
    reasoning: f64,
}

impl RawUsage {
    fn from_value(value: &Value) -> Self {
        Self {
            input: number(
                value
                    .get("input_tokens")
                    .or_else(|| value.get("prompt_tokens")),
            )
            .unwrap_or(0.0),
            cached: number(
                value
                    .get("cached_input_tokens")
                    .or_else(|| value.get("cache_read_input_tokens")),
            )
            .unwrap_or(0.0),
            output: number(
                value
                    .get("output_tokens")
                    .or_else(|| value.get("completion_tokens")),
            )
            .unwrap_or(0.0),
            reasoning: number(value.get("reasoning_output_tokens")).unwrap_or(0.0),
        }
    }

    fn subtract(self, previous: Option<Self>) -> Self {
        let previous = previous.unwrap_or_default();
        Self {
            input: (self.input - previous.input).max(0.0),
            cached: (self.cached - previous.cached).max(0.0),
            output: (self.output - previous.output).max(0.0),
            reasoning: (self.reasoning - previous.reasoning).max(0.0),
        }
    }

    fn total(self) -> f64 {
        self.input + self.output + self.reasoning
    }
}

fn model_name(value: Option<&Value>) -> Option<&str> {
    text(value.and_then(|value| value.get("model")))
}

fn aggregate(
    entries: Vec<CodexHistoryEntry>,
    pricing: &dyn PricingCatalog,
) -> HashMap<NaiveDate, DailyUsage> {
    let mut daily = HashMap::<NaiveDate, DailyUsage>::new();
    for entry in entries {
        let tokens = entry.input_tokens + entry.output_tokens;
        let price = pricing.price(&entry.model);
        let cost = price.map(|price| {
            let non_cached = (entry.input_tokens - entry.cached_input_tokens).max(0.0);
            let base = non_cached * price.input_per_million / 1_000_000.0
                + entry.cached_input_tokens * price.cached_input_per_million / 1_000_000.0
                + entry.output_tokens * price.output_per_million / 1_000_000.0;
            if entry.fast { base * 2.0 } else { base }
        });
        let usage = daily.entry(entry.date).or_default();
        usage.tokens += tokens;
        if let Some(cost) = cost {
            usage.cost_usd += cost;
            usage.has_cost = true;
        }
    }
    daily
}
