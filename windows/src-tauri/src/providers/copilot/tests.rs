use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::header::HeaderMap;

use crate::platform::http::{HttpError, HttpRequest, HttpResponse, HttpTransport};

use super::*;

struct FakeHttp(Mutex<VecDeque<HttpResponse>>);

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(&self, _: HttpRequest, _: CancellationToken) -> Result<HttpResponse, HttpError> {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| HttpError::Request("missing fixture".to_owned()))
    }
}

fn response(status: StatusCode, body: &'static [u8]) -> HttpResponse {
    HttpResponse {
        status,
        headers: HeaderMap::new(),
        body: body.to_vec(),
    }
}

#[tokio::test]
async fn editor_auth_and_usage_fixture_map_credits_without_unlimited_placeholders() {
    let directory = tempfile::tempdir().unwrap();
    let auth_path = directory.path().join("apps.json");
    std::fs::write(
        &auth_path,
        r#"{"github.com:client":{"oauth_token":"fixture-token"}}"#,
    )
    .unwrap();
    let provider = CopilotProvider::new(
        CopilotAuthStore::new(vec![auth_path]),
        Arc::new(FakeHttp(Mutex::new(VecDeque::from([response(
            StatusCode::OK,
            include_bytes!(
                "../../../../../Tests/Fixtures/ProviderParity/v1/providers/copilot/usage.json"
            ),
        )])))),
    );

    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("Individual Pro"));
    assert_eq!(
        snapshot
            .lines
            .iter()
            .map(MetricLine::label)
            .collect::<Vec<_>>(),
        ["Credits", "Extra Usage"]
    );
}

#[tokio::test]
async fn missing_and_denied_credentials_are_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let missing = CopilotAuthStore::new(vec![directory.path().join("missing.json")]);
    assert_eq!(
        missing.load().await.unwrap_err().category,
        ErrorCategory::NotLoggedIn
    );

    let auth_path = directory.path().join("apps.json");
    std::fs::write(
        &auth_path,
        r#"{"github.com":{"oauth_token":"secret-token"}}"#,
    )
    .unwrap();
    let provider = CopilotProvider::new(
        CopilotAuthStore::new(vec![auth_path]),
        Arc::new(FakeHttp(Mutex::new(VecDeque::from([response(
            StatusCode::UNAUTHORIZED,
            b"{}",
        )])))),
    );
    let error = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error.category, ErrorCategory::AuthInvalid);
    assert!(!error.message.contains("secret-token"));
}

#[test]
fn organization_billing_counts_only_ai_credit_units() {
    let lines = map_org_billing(include_bytes!(
        "../../../../../Tests/Fixtures/ProviderParity/v1/providers/copilot/org-billing.json"
    ))
    .unwrap();

    assert_eq!(
        lines.iter().map(MetricLine::label).collect::<Vec<_>>(),
        ["Org Credits", "Org Spend"]
    );
    match &lines[0] {
        MetricLine::Values { values, .. } => assert_eq!(values[0].number, 125.0),
        _ => panic!("organization credits must be a value"),
    }
}

#[test]
fn malformed_usage_payload_fails_loudly() {
    let error = map_usage(&serde_json::json!({})).unwrap_err();
    assert_eq!(error.category, ErrorCategory::NotAvailable);
}
