use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

use super::super::support::{DailyUsage, PricingCatalog, ProviderError};
use crate::contracts::ErrorCategory;

const REQUIRED_COLUMNS: [&str; 6] = [
    "Date",
    "Model",
    "Input (w/ Cache Write)",
    "Input (w/o Cache Write)",
    "Cache Read",
    "Output Tokens",
];
const MAX_CSV_ROWS: usize = 100_000;
const MAX_CSV_COLUMNS: usize = 128;

pub fn parse_csv(
    contents: &[u8],
    pricing: &dyn PricingCatalog,
) -> Result<HashMap<NaiveDate, DailyUsage>, ProviderError> {
    let text = std::str::from_utf8(contents).map_err(|_| invalid_csv())?;
    let rows = parse_rows(text)?;
    let header = rows.first().ok_or_else(invalid_csv)?;
    if header.len() > MAX_CSV_COLUMNS || header.iter().collect::<HashSet<_>>().len() != header.len()
    {
        return Err(invalid_csv());
    }
    let indexes = REQUIRED_COLUMNS
        .iter()
        .map(|required| {
            header
                .iter()
                .position(|column| column.trim_matches('\u{feff}').trim() == *required)
                .ok_or_else(invalid_csv)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut daily = HashMap::<NaiveDate, DailyUsage>::new();
    for row in rows.iter().skip(1) {
        if row.len() != header.len() {
            continue;
        }
        let Some(date) = parse_date(&row[indexes[0]]) else {
            continue;
        };
        let model = row[indexes[1]].trim();
        if model.is_empty() {
            continue;
        }
        let Some(cache_write) = parse_tokens(&row[indexes[2]]) else {
            continue;
        };
        let Some(input) = parse_tokens(&row[indexes[3]]) else {
            continue;
        };
        let Some(cache_read) = parse_tokens(&row[indexes[4]]) else {
            continue;
        };
        let Some(output) = parse_tokens(&row[indexes[5]]) else {
            continue;
        };
        let Some(total) = cache_write
            .checked_add(input)
            .and_then(|total| total.checked_add(cache_read))
            .and_then(|total| total.checked_add(output))
        else {
            continue;
        };
        let price = pricing.price(model);
        let cost = price.map(|price| {
            (cache_write + input) as f64 * price.input_per_million / 1_000_000.0
                + cache_read as f64 * price.cached_input_per_million / 1_000_000.0
                + output as f64 * price.output_per_million / 1_000_000.0
        });
        let usage = daily.entry(date).or_default();
        usage.tokens += total as f64;
        if let Some(cost) = cost {
            usage.cost_usd += cost;
            usage.has_cost = true;
        }
    }
    Ok(daily)
}

fn parse_rows(text: &str) -> Result<Vec<Vec<String>>, ProviderError> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut characters = text.chars().peekable();
    let mut quoted = false;
    let mut quote_closed = false;
    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' {
                if characters.peek() == Some(&'"') {
                    characters.next();
                    field.push('"');
                } else {
                    quoted = false;
                    quote_closed = true;
                }
            } else {
                field.push(character);
            }
            continue;
        }
        if quote_closed && !matches!(character, ',' | '\r' | '\n') {
            return Err(invalid_csv());
        }
        match character {
            '"' if field.is_empty() && !quote_closed => quoted = true,
            '"' => return Err(invalid_csv()),
            ',' => {
                row.push(std::mem::take(&mut field));
                quote_closed = false;
            }
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                finish_row(&mut rows, &mut row, &mut field)?;
                quote_closed = false;
            }
            '\n' => {
                finish_row(&mut rows, &mut row, &mut field)?;
                quote_closed = false;
            }
            _ => field.push(character),
        }
    }
    if quoted {
        return Err(invalid_csv());
    }
    if !field.is_empty() || !row.is_empty() || quote_closed {
        finish_row(&mut rows, &mut row, &mut field)?;
    }
    Ok(rows)
}

fn finish_row(
    rows: &mut Vec<Vec<String>>,
    row: &mut Vec<String>,
    field: &mut String,
) -> Result<(), ProviderError> {
    row.push(std::mem::take(field));
    if row.iter().any(|value| !value.is_empty()) {
        if rows.len() >= MAX_CSV_ROWS || row.len() > MAX_CSV_COLUMNS {
            return Err(ProviderError::new(
                ErrorCategory::Decoding,
                "Cursor usage export exceeds its supported bound.",
            ));
        }
        rows.push(std::mem::take(row));
    } else {
        row.clear();
    }
    Ok(())
}

fn parse_tokens(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() {
        return Some(0);
    }
    let groups = value.split(',').collect::<Vec<_>>();
    if groups.len() > 1
        && (!(1..=3).contains(&groups[0].len())
            || !groups[0].bytes().all(|byte| byte.is_ascii_digit())
            || groups[1..]
                .iter()
                .any(|group| group.len() != 3 || !group.bytes().all(|byte| byte.is_ascii_digit())))
    {
        return None;
    }
    if groups.len() == 1 && !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    groups.concat().parse().ok()
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    let value = value.trim();
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc).date_naive())
        .or_else(|| {
            NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
                .ok()
                .map(|date| date.date())
        })
}

fn invalid_csv() -> ProviderError {
    ProviderError::new(
        ErrorCategory::Decoding,
        "Cursor usage export is malformed or missing required columns.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::FixedPricing;

    #[test]
    fn quoted_fields_grouped_numbers_and_empty_zero_are_supported() {
        let csv = "Date,Model,Input (w/ Cache Write),Input (w/o Cache Write),Cache Read,Output Tokens\n\
                   2026-07-14 12:00:00,\"model,variant\",\"1,000\",2,,3\n";

        let daily = parse_csv(csv.as_bytes(), &FixedPricing::default()).unwrap();

        assert_eq!(daily.values().next().unwrap().tokens, 1005.0);
    }

    #[test]
    fn duplicate_headers_and_unclosed_quotes_fail_loudly() {
        assert!(parse_rows("Date,Date\n\"open").is_err());
    }
}
