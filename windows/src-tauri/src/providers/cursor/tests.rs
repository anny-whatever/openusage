use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::Engine;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;
use rusqlite::params;

use crate::contracts::MetricLine;
use crate::platform::http::{HttpError, HttpRequest, HttpResponse, HttpTransport};
use crate::providers::{FixedPricing, ModelPrice};

use super::*;

struct FakeHttp {
    responses: Mutex<HashMap<String, VecDeque<HttpResponse>>>,
}

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(
        &self,
        request: HttpRequest,
        _cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        let key = self
            .responses
            .lock()
            .unwrap()
            .keys()
            .find(|key| request.url.as_str().contains(key.as_str()))
            .cloned()
            .ok_or_else(|| HttpError::Request("missing fixture endpoint".to_owned()))?;
        self.responses
            .lock()
            .unwrap()
            .get_mut(&key)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| HttpError::Request("missing fixture response".to_owned()))
    }
}

fn response(body: impl Into<Vec<u8>>) -> HttpResponse {
    HttpResponse {
        status: StatusCode::OK,
        headers: HeaderMap::new(),
        body: body.into(),
    }
}

fn jwt(subject: &str) -> String {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "sub": subject,
            "exp": 4_102_444_800_u64
        }))
        .unwrap(),
    );
    format!("{header}.{payload}.")
}

fn create_locked_wal_database(
    directory: &tempfile::TempDir,
) -> (std::path::PathBuf, rusqlite::Connection) {
    let path = directory.path().join("state.vscdb");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    connection
        .execute(
            "CREATE TABLE ItemTable(key TEXT PRIMARY KEY, value TEXT)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO ItemTable(key, value) VALUES (?1, ?2), (?3, ?4), (?5, ?6)",
            params![
                "cursorAuth/accessToken",
                jwt("auth0|user-1"),
                "cursorAuth/refreshToken",
                "refresh",
                "cursorAuth/stripeMembershipType",
                "pro"
            ],
        )
        .unwrap();
    (path, connection)
}

#[tokio::test]
async fn locked_wal_database_stays_read_only_and_account_wide_csv_is_counted_once() {
    let directory = tempfile::tempdir().unwrap();
    let (database, connection) = create_locked_wal_database(&directory);
    let before = std::fs::metadata(&database).unwrap().modified().unwrap();
    let http = Arc::new(FakeHttp {
        responses: Mutex::new(HashMap::from([
            (
                "GetCurrentPeriodUsage".to_owned(),
                VecDeque::from([response(include_bytes!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/cursor/usage.json"
                ))]),
            ),
            (
                "GetPlanInfo".to_owned(),
                VecDeque::from([response(include_bytes!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/cursor/plan.json"
                ))]),
            ),
            (
                "GetCreditGrantsBalance".to_owned(),
                VecDeque::from([response(include_bytes!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/cursor/credits.json"
                ))]),
            ),
            (
                "export-usage-events-csv".to_owned(),
                VecDeque::from([response(include_bytes!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/cursor/history.csv"
                ))]),
            ),
        ])),
    });
    let pricing = FixedPricing::new(HashMap::from([(
        "cursor-model".to_owned(),
        ModelPrice {
            input_per_million: 1.0,
            cached_input_per_million: 0.1,
            output_per_million: 2.0,
        },
    )]));
    let provider = CursorProvider::new(
        CursorAuthStore::new(database.clone()),
        http,
        Arc::new(pricing),
    );

    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    let today = snapshot
        .lines
        .iter()
        .find(|line| line.label() == "Today")
        .expect("today line");
    match today {
        MetricLine::Values { values, .. } => assert_eq!(values.last().unwrap().number, 300.0),
        _ => panic!("today must be numeric values"),
    }
    assert_eq!(snapshot.plan.as_deref(), Some("Pro Plan"));
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM ItemTable", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        std::fs::metadata(&database).unwrap().modified().unwrap(),
        before
    );
}

#[tokio::test]
async fn malformed_or_missing_cursor_database_fails_loudly() {
    let directory = tempfile::tempdir().unwrap();
    let missing = CursorAuthStore::new(directory.path().join("missing.db"));
    let missing_error = match missing.load().await {
        Ok(_) => panic!("missing DB must fail"),
        Err(error) => error,
    };
    assert_eq!(
        missing_error.category,
        crate::contracts::ErrorCategory::NotLoggedIn
    );

    let malformed_path = directory.path().join("malformed.db");
    rusqlite::Connection::open(&malformed_path).unwrap();
    let malformed_error = match CursorAuthStore::new(malformed_path).load().await {
        Ok(_) => panic!("malformed DB must fail"),
        Err(error) => error,
    };
    assert_eq!(
        malformed_error.category,
        crate::contracts::ErrorCategory::CredentialAccess
    );
}
