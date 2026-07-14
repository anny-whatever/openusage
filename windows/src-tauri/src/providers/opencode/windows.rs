use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};

use crate::contracts::{MetricFormat, MetricLine};

use super::scanner::OpenCodeRow;

const SESSION_MS: i64 = 5 * 60 * 60 * 1000;
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const SESSION_CAP: f64 = 12.0;
const WEEKLY_CAP: f64 = 30.0;
const MONTHLY_CAP: f64 = 60.0;

pub fn meter_lines(
    rows: &[OpenCodeRow],
    anchor_ms: Option<i64>,
    now: DateTime<Utc>,
) -> Vec<MetricLine> {
    let now_ms = now.timestamp_millis();
    let session_start = now_ms - SESSION_MS;
    let session_rows = rows
        .iter()
        .filter(|row| row.timestamp_ms >= session_start && row.timestamp_ms < now_ms);
    let session_spend = session_rows.clone().map(|row| row.cost).sum();
    let session_reset = session_rows
        .map(|row| row.timestamp_ms)
        .min()
        .unwrap_or(now_ms)
        + SESSION_MS;

    let week_start_date =
        now.date_naive() - Duration::days(now.weekday().num_days_from_monday() as i64);
    let week_start = Utc
        .from_utc_datetime(&week_start_date.and_hms_opt(0, 0, 0).expect("midnight"))
        .timestamp_millis();
    let week_end = week_start + WEEK_MS;
    let weekly_spend = sum(rows, week_start, week_end);

    let (month_start, month_end) = month_bounds(now, anchor_ms);
    let monthly_spend = sum(
        rows,
        month_start.timestamp_millis(),
        month_end.timestamp_millis(),
    );

    vec![
        line(
            "Session",
            session_spend,
            SESSION_CAP,
            session_reset,
            SESSION_MS as u64,
        ),
        line("Weekly", weekly_spend, WEEKLY_CAP, week_end, WEEK_MS as u64),
        line(
            "Monthly",
            monthly_spend,
            MONTHLY_CAP,
            month_end.timestamp_millis(),
            (month_end.timestamp_millis() - month_start.timestamp_millis()) as u64,
        ),
    ]
}

fn month_bounds(now: DateTime<Utc>, anchor_ms: Option<i64>) -> (DateTime<Utc>, DateTime<Utc>) {
    let Some(anchor) = anchor_ms.and_then(DateTime::<Utc>::from_timestamp_millis) else {
        let start = date_time(now.year(), now.month(), 1, 0, 0, 0, 0);
        let (year, month) = shift_month(now.year(), now.month(), 1);
        return (start, date_time(year, month, 1, 0, 0, 0, 0));
    };
    let mut year = now.year();
    let mut month = now.month();
    let mut start = anchored_start(year, month, anchor);
    if start > now {
        (year, month) = shift_month(year, month, -1);
        start = anchored_start(year, month, anchor);
    }
    let (next_year, next_month) = shift_month(year, month, 1);
    (start, anchored_start(next_year, next_month, anchor))
}

fn anchored_start(year: i32, month: u32, anchor: DateTime<Utc>) -> DateTime<Utc> {
    date_time(
        year,
        month,
        anchor.day().min(days_in_month(year, month)),
        anchor.hour(),
        anchor.minute(),
        anchor.second(),
        anchor.nanosecond(),
    )
}

fn date_time(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nanosecond: u32,
) -> DateTime<Utc> {
    Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(year, month, day)
            .expect("validated date")
            .and_hms_nano_opt(hour, minute, second, nanosecond)
            .expect("validated time"),
    )
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (next_year, next_month) = shift_month(year, month, 1);
    (NaiveDate::from_ymd_opt(next_year, next_month, 1).unwrap() - Duration::days(1)).day()
}

fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let total = year * 12 + month as i32 - 1 + delta;
    (total.div_euclid(12), total.rem_euclid(12) as u32 + 1)
}

fn sum(rows: &[OpenCodeRow], start: i64, end: i64) -> f64 {
    let total: f64 = rows
        .iter()
        .filter(|row| row.timestamp_ms >= start && row.timestamp_ms < end)
        .map(|row| row.cost)
        .sum();
    (total * 10_000.0).round() / 10_000.0
}

fn line(label: &str, used: f64, limit: f64, reset_ms: i64, period_ms: u64) -> MetricLine {
    MetricLine::Progress {
        label: label.to_owned(),
        used,
        limit,
        format: MetricFormat::Dollars,
        resets_at: DateTime::<Utc>::from_timestamp_millis(reset_ms).map(|date| date.to_rfc3339()),
        period_duration_ms: Some(period_ms),
        color_hex: None,
    }
}
