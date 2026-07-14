use chrono::TimeZone;

use super::*;

#[test]
fn utc_go_windows_match_session_week_and_anchored_month() {
    let now = chrono::Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
    let rows = vec![
        scanner::OpenCodeRow {
            timestamp_ms: now.timestamp_millis() - 60 * 60 * 1000,
            cost: 2.0,
            tokens: 100.0,
            provider_id: "opencode-go".to_owned(),
        },
        scanner::OpenCodeRow {
            timestamp_ms: now.timestamp_millis() - 24 * 60 * 60 * 1000,
            cost: 3.0,
            tokens: 100.0,
            provider_id: "opencode-go".to_owned(),
        },
    ];
    let lines = windows::meter_lines(
        &rows,
        Some(
            chrono::Utc
                .with_ymd_and_hms(2026, 1, 10, 4, 0, 0)
                .unwrap()
                .timestamp_millis(),
        ),
        now,
    );
    assert_eq!(
        lines.iter().map(MetricLine::label).collect::<Vec<_>>(),
        ["Session", "Weekly", "Monthly"]
    );
    match &lines[0] {
        MetricLine::Progress { used, .. } => assert_eq!(*used, 2.0),
        _ => panic!("session meter"),
    }
    match &lines[1] {
        MetricLine::Progress { used, .. } => assert_eq!(*used, 5.0),
        _ => panic!("weekly meter"),
    }
}

#[tokio::test]
async fn locked_sqlite_is_read_only_and_hosted_usage_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("opencode.db");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    connection
        .execute("CREATE TABLE message(time_created INTEGER, data TEXT)", [])
        .unwrap();
    let now =
        chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp_millis();
    let data = serde_json::json!({ "role": "assistant", "providerID": "opencode-go", "modelID": "go-test", "cost": 1.5, "tokens": { "total": 120 } }).to_string();
    connection
        .execute(
            "INSERT INTO message VALUES (?1, ?2)",
            rusqlite::params![now, data],
        )
        .unwrap();
    let before = std::fs::metadata(&database).unwrap().modified().unwrap();
    let provider = OpenCodeProvider::new(OpenCodeAuthStore::new(directory.path().to_owned()));

    let snapshot = provider.refresh_provider().await.unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("Go"));
    assert!(snapshot.usage_history.is_some());
    assert_eq!(
        std::fs::metadata(&database).unwrap().modified().unwrap(),
        before
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM message", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn malformed_auth_without_database_fails_as_logged_out_without_secret_output() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("auth.json"), "not-json").unwrap();
    let provider = OpenCodeProvider::new(OpenCodeAuthStore::new(directory.path().to_owned()));
    let error = provider.refresh_provider().await.unwrap_err();
    assert_eq!(error.category, ErrorCategory::CredentialAccess);
    assert!(!error.message.contains("not-json"));
}
