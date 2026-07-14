use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;

use crate::platform::http::{HttpError, HttpRequest, HttpResponse, HttpTransport};
use crate::providers::{FixedPricing, ModelPrice};

use super::*;

struct FakeHttp {
    responses: Mutex<VecDeque<HttpResponse>>,
    urls: Mutex<Vec<String>>,
}

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(
        &self,
        request: HttpRequest,
        _cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        self.urls.lock().unwrap().push(request.url.to_string());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| HttpError::Request("no fixture response".to_owned()))
    }
}

fn response(status: StatusCode, body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: HeaderMap::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn write_auth(directory: &std::path::Path, access: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(directory).unwrap();
    let path = directory.join("auth.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"tokens":{{"access_token":"{access}","refresh_token":"refresh","account_id":"account"}}}}"#
        ),
    )
    .unwrap();
    path
}

fn usage_body() -> &'static str {
    include_str!("../../../../../Tests/Fixtures/ProviderParity/v1/providers/codex/usage.json")
}

fn token_line(timestamp: &str) -> String {
    format!(
        r#"{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","model":"gpt-test","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":50}}}}}}}}"#
    )
}

#[tokio::test]
async fn maps_usage_and_deduplicates_active_history_over_archive() {
    let directory = tempfile::tempdir().unwrap();
    let home = directory.path().join(".codex");
    let auth_path = write_auth(&home, "access");
    let active = home.join("sessions/day");
    let archived = home.join("archived_sessions/day");
    std::fs::create_dir_all(&active).unwrap();
    std::fs::create_dir_all(&archived).unwrap();
    let timestamp = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now())
        .format("%Y-%m-%dT12:00:00Z")
        .to_string();
    std::fs::write(active.join("rollout.jsonl"), token_line(&timestamp)).unwrap();
    std::fs::write(archived.join("rollout.jsonl"), token_line(&timestamp)).unwrap();
    let http = Arc::new(FakeHttp {
        responses: Mutex::new(VecDeque::from([
            response(StatusCode::OK, usage_body()),
            response(
                StatusCode::OK,
                include_str!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/codex/reset-credits.json"
                ),
            ),
        ])),
        urls: Mutex::new(Vec::new()),
    });
    let pricing = FixedPricing::new(HashMap::from([(
        "gpt-test".to_owned(),
        ModelPrice {
            input_per_million: 1.0,
            cached_input_per_million: 0.1,
            output_per_million: 2.0,
        },
    )]));
    let provider = CodexProvider::new(
        CodexAuthStore::new(vec![auth_path]),
        http,
        Arc::new(pricing),
    );

    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert!(snapshot.lines.iter().any(|line| line.label() == "Session"));
    assert!(
        snapshot
            .lines
            .iter()
            .any(|line| line.label() == "Rate Limit Resets")
    );
    let today = snapshot
        .lines
        .iter()
        .find(|line| line.label() == "Today")
        .expect("today history line");
    match today {
        MetricLine::Values { values, .. } => {
            assert_eq!(values.last().unwrap().number, 150.0);
        }
        _ => panic!("today must be numeric values"),
    }
}

#[tokio::test]
async fn api_key_only_auth_is_explicitly_unsupported() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.json");
    std::fs::write(&path, r#"{"OPENAI_API_KEY":"secret"}"#).unwrap();

    let error = match CodexAuthStore::new(vec![path]).load().await {
        Ok(_) => panic!("API-key auth must not load for subscription usage"),
        Err(error) => error,
    };

    assert_eq!(
        error.category,
        crate::contracts::ErrorCategory::NotAvailable
    );
    assert!(!error.message.contains("secret"));
}

#[tokio::test]
async fn ordered_auth_sources_skip_invalid_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("invalid/auth.json");
    std::fs::create_dir_all(invalid.parent().unwrap()).unwrap();
    std::fs::write(&invalid, "not-json").unwrap();
    let valid = write_auth(&directory.path().join("valid"), "second-access");
    let store = CodexAuthStore::new(vec![invalid, valid]);

    let credentials = store.load_all().await.unwrap();

    assert_eq!(credentials.len(), 1);
    assert_eq!(credentials[0].access_token.expose(), b"second-access");
}

struct RefreshCounter(AtomicUsize);

#[async_trait]
impl PostClaimRefresh for RefreshCounter {
    async fn refresh_codex(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn explicit_reset_claim_replays_one_idempotent_credit_and_refreshes() {
    let directory = tempfile::tempdir().unwrap();
    let auth = CodexAuthStore::new(vec![write_auth(directory.path(), "access")]);
    let expiry = "2026-07-20T12:00:00Z"
        .parse::<chrono::DateTime<chrono::Utc>>()
        .unwrap();
    let http = Arc::new(FakeHttp {
        responses: Mutex::new(VecDeque::from([
            response(
                StatusCode::OK,
                r#"{"credits":[{"id":"credit-1","status":"available","expires_at":"2026-07-20T12:00:00Z"}]}"#,
            ),
            response(StatusCode::OK, r#"{"code":"reset"}"#),
            response(StatusCode::OK, r#"{"code":"already_redeemed"}"#),
        ])),
        urls: Mutex::new(Vec::new()),
    });
    let refresh = Arc::new(RefreshCounter(AtomicUsize::new(0)));
    let service =
        CodexResetClaimService::new(auth, CodexClient::new(http.clone()), refresh.clone());

    let first = service
        .claim(
            expiry,
            "request-1".to_owned(),
            ResetClaimConfirmation::confirmed(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let replay = service
        .claim(
            expiry,
            "request-1".to_owned(),
            ResetClaimConfirmation::confirmed(),
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(first, ResetClaimOutcome::Success);
    assert_eq!(replay, ResetClaimOutcome::Success);
    assert_eq!(refresh.0.load(Ordering::SeqCst), 2);
    assert_eq!(
        http.urls
            .lock()
            .unwrap()
            .iter()
            .filter(|url| url.ends_with("/consume"))
            .count(),
        2
    );
}
