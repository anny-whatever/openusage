use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::params;

use crate::contracts::ErrorCategory;
use crate::platform::sqlite::private_snapshot;

use super::super::support::{DailyUsage, ProviderError, number, text};

const MAX_DATABASE_ROWS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct OpenCodeRow {
    pub timestamp_ms: i64,
    pub cost: f64,
    pub tokens: f64,
    pub provider_id: String,
}

pub struct OpenCodeScan {
    pub daily: HashMap<NaiveDate, DailyUsage>,
    pub go_rows: Vec<OpenCodeRow>,
    pub go_anchor_ms: Option<i64>,
}

pub async fn scan(
    databases: Vec<PathBuf>,
    now: DateTime<Utc>,
) -> Result<Option<OpenCodeScan>, ProviderError> {
    if databases.is_empty() {
        return Ok(None);
    }
    let cutoff = (now - chrono::Duration::days(33)).timestamp_millis();
    let mut rows = Vec::new();
    let mut anchor = None;
    let mut failures = 0;
    for database in databases {
        let snapshot = match private_snapshot(database).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                failures += 1;
                continue;
            }
        };
        let loaded = tokio::task::spawn_blocking(move || query_snapshot(snapshot, cutoff))
            .await
            .map_err(|_| provider_error())?;
        match loaded {
            Ok((mut loaded_rows, loaded_anchor)) => {
                if rows.len().saturating_add(loaded_rows.len()) > MAX_DATABASE_ROWS {
                    return Err(ProviderError::new(
                        ErrorCategory::Other,
                        "OpenCode usage exceeds the supported row bound.",
                    ));
                }
                rows.append(&mut loaded_rows);
                anchor = loaded_anchor.into_iter().chain(anchor).min();
            }
            Err(_) => failures += 1,
        }
    }
    if failures > 0 && rows.is_empty() {
        return Err(provider_error());
    }
    let tile_cutoff = (now - chrono::Duration::days(30)).timestamp_millis();
    let mut daily = HashMap::<NaiveDate, DailyUsage>::new();
    let mut go_rows = Vec::new();
    for row in rows {
        if row.provider_id == "opencode-go" {
            go_rows.push(row.clone());
        }
        if row.timestamp_ms < tile_cutoff {
            continue;
        }
        let Some(date) =
            DateTime::<Utc>::from_timestamp_millis(row.timestamp_ms).map(|date| date.date_naive())
        else {
            continue;
        };
        let day = daily.entry(date).or_default();
        day.tokens += row.tokens;
        day.cost_usd += row.cost;
        day.has_cost = true;
    }
    Ok(Some(OpenCodeScan {
        daily,
        go_rows,
        go_anchor_ms: anchor,
    }))
}

fn query_snapshot(
    snapshot: crate::platform::sqlite::PrivateSqliteSnapshot,
    cutoff: i64,
) -> Result<(Vec<OpenCodeRow>, Option<i64>), rusqlite::Error> {
    let connection = snapshot.open().map_err(|error| match error {
        crate::platform::sqlite::SqliteReadError::Sqlite(error) => error,
        _ => rusqlite::Error::InvalidQuery,
    })?;
    let mut statement = connection.prepare(
        "SELECT time_created, data FROM message WHERE time_created >= ?1 ORDER BY time_created LIMIT ?2",
    )?;
    let iterator = statement.query_map(params![cutoff, (MAX_DATABASE_ROWS + 1) as i64], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut rows = Vec::new();
    for row in iterator {
        let (timestamp_ms, data) = row?;
        if let Some(row) = parse_row(timestamp_ms, &data) {
            rows.push(row);
        }
    }
    if rows.len() > MAX_DATABASE_ROWS {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let anchor = connection.query_row(
        "SELECT MIN(time_created) FROM message WHERE json_valid(data) AND json_extract(data,'$.role') = 'assistant' AND json_extract(data,'$.providerID') = 'opencode-go'",
        [], |row| row.get::<_, Option<i64>>(0),
    ).unwrap_or(None);
    Ok((rows, anchor))
}

fn parse_row(timestamp_ms: i64, data: &str) -> Option<OpenCodeRow> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    if text(value.get("role")) != Some("assistant") {
        return None;
    }
    let provider_id = text(value.get("providerID"))?;
    if provider_id != "opencode-go" && provider_id != "opencode" {
        return None;
    }
    let cost = number(value.get("cost")).filter(|value| *value >= 0.0)?;
    let tokens = number(value.get("tokens").and_then(|value| value.get("total")))
        .unwrap_or(0.0)
        .clamp(0.0, 1e15);
    Some(OpenCodeRow {
        timestamp_ms,
        cost,
        tokens,
        provider_id: provider_id.to_owned(),
    })
}

fn provider_error() -> ProviderError {
    ProviderError::new(
        ErrorCategory::CredentialAccess,
        "OpenCode local database could not be read.",
    )
}
