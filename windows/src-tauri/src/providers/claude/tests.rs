use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::StatusCode;
use reqwest::header::HeaderMap;

use crate::platform::http::{HttpError, HttpRequest, HttpResponse, HttpTransport};
use crate::providers::{FixedPricing, ModelPrice};

use super::*;

struct FakeHttp {
    responses: Mutex<VecDeque<HttpResponse>>,
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(
        &self,
        request: HttpRequest,
        _cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        self.calls.lock().unwrap().push(request.url.to_string());
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

fn write_credentials(
    directory: &tempfile::TempDir,
    access: &str,
    refresh: &str,
) -> ClaudeAuthStore {
    let path = directory.path().join(".credentials.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"claudeAiOauth":{{"accessToken":"{access}","refreshToken":"{refresh}","expiresAt":4102444800000,"subscriptionType":"pro","rateLimitTier":"default_5x","scopes":["user:profile"]}}}}"#
        ),
    )
    .unwrap();
    ClaudeAuthStore::new(path)
}

#[tokio::test]
async fn maps_live_usage_and_incremental_local_history() {
    let directory = tempfile::tempdir().unwrap();
    let auth = write_credentials(&directory, "access", "refresh");
    let projects = directory.path().join("projects/project");
    std::fs::create_dir_all(&projects).unwrap();
    let today =
        chrono::DateTime::<Utc>::from(std::time::SystemTime::now()).format("%Y-%m-%dT12:00:00Z");
    std::fs::write(
        projects.join("session.jsonl"),
        format!(
            r#"{{"timestamp":"{today}","requestId":"request","message":{{"id":"message","model":"claude-test","usage":{{"input_tokens":100,"output_tokens":20}}}}}}"#
        ),
    )
    .unwrap();
    let http = Arc::new(FakeHttp {
        responses: Mutex::new(VecDeque::from([response(
            StatusCode::OK,
            include_str!(
                "../../../../../Tests/Fixtures/ProviderParity/v1/providers/claude/usage.json"
            ),
        )])),
        calls: Mutex::new(Vec::new()),
    });
    let pricing = FixedPricing::new(HashMap::from([(
        "claude-test".to_owned(),
        ModelPrice {
            input_per_million: 3.0,
            cached_input_per_million: 0.3,
            output_per_million: 15.0,
        },
    )]));
    let provider = ClaudeProvider::new(auth, http, Arc::new(pricing));

    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert!(snapshot.lines.iter().any(|line| line.label() == "Session"));
    assert!(snapshot.lines.iter().any(|line| line.label() == "Today"));
    assert!(snapshot.usage_history.is_some());
    assert_eq!(snapshot.plan.as_deref(), Some("Pro 5x"));
}

#[tokio::test]
async fn auth_rejection_refreshes_once_persists_and_retries() {
    let directory = tempfile::tempdir().unwrap();
    let auth = write_credentials(&directory, "old-access", "refresh");
    let http = Arc::new(FakeHttp {
        responses: Mutex::new(VecDeque::from([
            response(StatusCode::UNAUTHORIZED, "{}"),
            response(
                StatusCode::OK,
                r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600}"#,
            ),
            response(StatusCode::OK, r#"{"five_hour":{"utilization":10}}"#),
        ])),
        calls: Mutex::new(Vec::new()),
    });
    let provider = ClaudeProvider::new(
        auth.clone(),
        http.clone(),
        Arc::new(FixedPricing::default()),
    );

    provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(http.calls.lock().unwrap().len(), 3);
    assert_eq!(
        auth.load().await.unwrap().access_token.expose(),
        b"new-access"
    );
}

#[tokio::test]
async fn malformed_credentials_fail_without_network_or_secret_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(".credentials.json");
    std::fs::write(&path, b"not-json").unwrap();
    let error = match ClaudeAuthStore::new(path).load().await {
        Ok(_) => panic!("malformed credentials must fail"),
        Err(error) => error,
    };

    assert_eq!(error.category, crate::contracts::ErrorCategory::AuthInvalid);
    assert!(!error.message.contains("not-json"));
}

#[tokio::test]
async fn missing_credentials_report_logged_out() {
    let directory = tempfile::tempdir().unwrap();
    let error = match ClaudeAuthStore::new(directory.path().join("missing.json"))
        .load()
        .await
    {
        Ok(_) => panic!("missing credentials must not load"),
        Err(error) => error,
    };

    assert_eq!(error.category, crate::contracts::ErrorCategory::NotLoggedIn);
}
