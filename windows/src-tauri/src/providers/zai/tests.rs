use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::header::HeaderMap;

use crate::platform::environment::FakeEnvironment;
use crate::platform::http::{HttpError, HttpRequest, HttpTransport};

use super::*;

struct FakeHttp(Mutex<HashMap<String, VecDeque<HttpResponse>>>);

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(
        &self,
        request: HttpRequest,
        _: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        let endpoint = if request.url.path().contains("quota") {
            "quota"
        } else {
            "subscription"
        };
        self.0
            .lock()
            .unwrap()
            .get_mut(endpoint)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| HttpError::Request("missing fixture response".to_owned()))
    }
}

fn response(status: StatusCode, body: &'static [u8]) -> HttpResponse {
    HttpResponse {
        status,
        headers: HeaderMap::new(),
        body: body.to_vec(),
    }
}

fn paths(directory: &tempfile::TempDir) -> WindowsPaths {
    WindowsPaths::new(
        directory.path().join("profile"),
        directory.path().join("roaming"),
        directory.path().join("local"),
    )
    .unwrap()
}

#[tokio::test]
async fn maps_session_weekly_and_web_search_fixtures() {
    let directory = tempfile::tempdir().unwrap();
    let mut environment = FakeEnvironment::default();
    environment.insert("ZAI_API_KEY", b"fixture-key".to_vec());
    let provider = ZaiProvider::with_environment(
        &paths(&directory),
        Arc::new(FakeHttp(Mutex::new(HashMap::from([
            (
                "quota".to_owned(),
                VecDeque::from([response(
                    StatusCode::OK,
                    include_bytes!(
                        "../../../../../Tests/Fixtures/ProviderParity/v1/providers/zai/quota.json"
                    ),
                )]),
            ),
            (
                "subscription".to_owned(),
                VecDeque::from([response(
                    StatusCode::OK,
                    include_bytes!(
                        "../../../../../Tests/Fixtures/ProviderParity/v1/providers/zai/subscription.json"
                    ),
                )]),
            ),
        ])))),
        Arc::new(environment),
    );

    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("GLM Coding Max"));
    assert_eq!(
        snapshot
            .lines
            .iter()
            .map(MetricLine::label)
            .collect::<Vec<_>>(),
        ["Session", "Weekly", "Web Searches"]
    );
}

#[tokio::test]
async fn no_coding_plan_is_distinct_from_invalid_key() {
    let directory = tempfile::tempdir().unwrap();
    let mut environment = FakeEnvironment::default();
    environment.insert("ZAI_API_KEY", b"fixture-key".to_vec());
    let provider = ZaiProvider::with_environment(
        &paths(&directory),
        Arc::new(FakeHttp(Mutex::new(HashMap::from([
            (
                "quota".to_owned(),
                VecDeque::from([response(
                    StatusCode::OK,
                    br#"{"success":false,"code":500,"msg":"No coding plan"}"#,
                )]),
            ),
            (
                "subscription".to_owned(),
                VecDeque::from([response(StatusCode::OK, b"{}")]),
            ),
        ])))),
        Arc::new(environment),
    );

    let error = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.category, ErrorCategory::NotAvailable);

    let mut invalid_environment = FakeEnvironment::default();
    invalid_environment.insert("ZAI_API_KEY", b"fixture-key".to_vec());
    let invalid_provider = ZaiProvider::with_environment(
        &paths(&directory),
        Arc::new(FakeHttp(Mutex::new(HashMap::from([
            (
                "quota".to_owned(),
                VecDeque::from([response(StatusCode::UNAUTHORIZED, b"{}")]),
            ),
            (
                "subscription".to_owned(),
                VecDeque::from([response(StatusCode::OK, b"{}")]),
            ),
        ])))),
        Arc::new(invalid_environment),
    );
    let invalid_error = invalid_provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(invalid_error.category, ErrorCategory::AuthInvalid);
}
