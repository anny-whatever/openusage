use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;

use super::super::history::{HistoryError, IncrementalJsonlCache, discover_jsonl};
use super::super::support::{DailyUsage, PricingCatalog, number, text};

#[derive(Debug, Clone)]
struct ClaudeHistoryEntry {
    date: NaiveDate,
    deduplication_key: Option<String>,
    model: Option<String>,
    input_tokens: f64,
    cached_input_tokens: f64,
    output_tokens: f64,
    cost_usd: Option<f64>,
}

#[derive(Clone, Default)]
pub struct ClaudeHistoryScanner {
    cache: IncrementalJsonlCache<ClaudeHistoryEntry>,
}

impl ClaudeHistoryScanner {
    pub async fn scan(
        &self,
        projects_root: PathBuf,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Result<HashMap<NaiveDate, DailyUsage>, HistoryError> {
        let files = tokio::task::spawn_blocking(move || discover_jsonl(&[projects_root]))
            .await
            .map_err(|_| HistoryError::TaskFailed)??;
        let entries = self.cache.scan(files, parse_file).await?;
        Ok(aggregate(entries, pricing.as_ref()))
    }
}

fn parse_file(contents: &[u8]) -> Vec<ClaudeHistoryEntry> {
    contents
        .split(|byte| *byte == b'\n')
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &[u8]) -> Option<ClaudeHistoryEntry> {
    let value: Value = serde_json::from_slice(line).ok()?;
    let date = DateTime::parse_from_rfc3339(text(value.get("timestamp"))?)
        .ok()?
        .with_timezone(&Utc)
        .date_naive();
    let message = value.get("message")?;
    let usage = message.get("usage")?;
    let input_tokens = number(usage.get("input_tokens"))?;
    let output_tokens = number(usage.get("output_tokens"))?;
    let cache_creation = number(usage.get("cache_creation_input_tokens")).unwrap_or(0.0);
    let cache_read = number(usage.get("cache_read_input_tokens")).unwrap_or(0.0);
    let message_id = text(message.get("id"));
    let request_id = text(value.get("requestId"));
    Some(ClaudeHistoryEntry {
        date,
        deduplication_key: message_id
            .zip(request_id)
            .map(|(message, request)| format!("{message}:{request}")),
        model: text(message.get("model"))
            .filter(|model| *model != "<synthetic>")
            .map(str::to_owned),
        input_tokens: input_tokens + cache_creation + cache_read,
        cached_input_tokens: cache_read,
        output_tokens,
        cost_usd: number(value.get("costUSD")),
    })
}

fn aggregate(
    entries: Vec<ClaudeHistoryEntry>,
    pricing: &dyn PricingCatalog,
) -> HashMap<NaiveDate, DailyUsage> {
    let mut seen = HashSet::new();
    let mut daily = HashMap::<NaiveDate, DailyUsage>::new();
    for entry in entries {
        if entry
            .deduplication_key
            .as_ref()
            .is_some_and(|key| !seen.insert(key.clone()))
        {
            continue;
        }
        let tokens = entry.input_tokens + entry.output_tokens;
        let cost = entry.cost_usd.or_else(|| {
            let price = pricing.price(entry.model.as_deref()?)?;
            let non_cached_input = (entry.input_tokens - entry.cached_input_tokens).max(0.0);
            Some(
                non_cached_input * price.input_per_million / 1_000_000.0
                    + entry.cached_input_tokens * price.cached_input_per_million / 1_000_000.0
                    + entry.output_tokens * price.output_per_million / 1_000_000.0,
            )
        });
        let day = daily.entry(entry.date).or_default();
        day.tokens += tokens;
        if let Some(cost) = cost {
            day.cost_usd += cost;
            day.has_cost = true;
        }
    }
    daily
}
