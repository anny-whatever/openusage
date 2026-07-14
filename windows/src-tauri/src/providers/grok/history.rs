use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};

use super::super::history::{HistoryError, IncrementalJsonlCache};
use super::super::support::{DailyUsage, PricingCatalog, number, text};

#[derive(Debug, Clone)]
struct GrokHistoryEntry {
    date: NaiveDate,
    model: String,
    input: f64,
    cached: f64,
    output: f64,
}

#[derive(Clone, Default)]
pub struct GrokHistoryScanner {
    cache: IncrementalJsonlCache<GrokHistoryEntry>,
}

impl GrokHistoryScanner {
    pub async fn scan(
        &self,
        path: PathBuf,
        pricing: Arc<dyn PricingCatalog>,
    ) -> Result<HashMap<NaiveDate, DailyUsage>, HistoryError> {
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(HashMap::new());
        }
        let entries = self.cache.scan(vec![path], parse_file).await?;
        let mut daily = HashMap::<NaiveDate, DailyUsage>::new();
        for entry in entries {
            let Some(price) = pricing.price(&entry.model) else {
                continue;
            };
            let cost = (entry.input - entry.cached).max(0.0) * price.input_per_million
                / 1_000_000.0
                + entry.cached * price.cached_input_per_million / 1_000_000.0
                + entry.output * price.output_per_million / 1_000_000.0;
            let day = daily.entry(entry.date).or_default();
            day.tokens += entry.input + entry.output;
            day.cost_usd += cost;
            day.has_cost = true;
        }
        Ok(daily)
    }
}

fn parse_file(contents: &[u8]) -> Vec<GrokHistoryEntry> {
    let mut models = HashMap::<i64, String>::new();
    let mut entries = Vec::new();
    for line in contents.split(|byte| *byte == b'\n') {
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let Some(message) = text(value.get("msg")) else {
            continue;
        };
        let process = number(value.get("pid")).map(|value| value as i64);
        let context = value.get("ctx").unwrap_or(&serde_json::Value::Null);
        if let Some(model) = model_event(message, context) {
            if let Some(process) = process {
                models.insert(process, model.to_owned());
            }
            continue;
        }
        if message != "shell.turn.inference_done" {
            continue;
        }
        let Some(model) = process.and_then(|process| models.get(&process)).cloned() else {
            continue;
        };
        let Some(timestamp) =
            text(value.get("ts")).and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        else {
            continue;
        };
        let Some(prompt) = number(context.get("prompt_tokens")) else {
            continue;
        };
        let cached = number(context.get("cached_prompt_tokens"))
            .unwrap_or(0.0)
            .clamp(0.0, prompt.max(0.0));
        let completion = number(context.get("completion_tokens"))
            .unwrap_or(0.0)
            .max(0.0);
        let reasoning = number(context.get("reasoning_tokens"))
            .unwrap_or(0.0)
            .max(0.0);
        entries.push(GrokHistoryEntry {
            date: timestamp.with_timezone(&Utc).date_naive(),
            model,
            input: prompt.max(0.0),
            cached,
            output: completion + reasoning,
        });
    }
    entries
}

fn model_event<'a>(message: &str, context: &'a serde_json::Value) -> Option<&'a str> {
    let fields: &[&str] = match message {
        "model changed" => &["model"],
        "model catalog: notifying clients" => &["current_model_id"],
        "backend_search: model switch" | "subagent model resolved" => {
            &["model", "current_model_id", "model_id"]
        }
        _ => return None,
    };
    fields.iter().find_map(|field| text(context.get(field)))
}
