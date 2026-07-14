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
        _cancellation: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        let endpoint = if request.url.path().ends_with("/credits") {
            "credits"
        } else {
            "key"
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
async fn protected_key_lifecycle_and_fixture_mapping_never_read_back_the_key() {
    let directory = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp(Mutex::new(HashMap::from([
        (
            "credits".to_owned(),
            VecDeque::from([response(
                StatusCode::OK,
                include_bytes!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/openrouter/credits.json"
                ),
            )]),
        ),
        (
            "key".to_owned(),
            VecDeque::from([response(
                StatusCode::OK,
                include_bytes!(
                    "../../../../../Tests/Fixtures/ProviderParity/v1/providers/openrouter/key.json"
                ),
            )]),
        ),
    ]))));
    let provider = OpenRouterProvider::with_environment(
        &paths(&directory),
        http,
        Arc::new(FakeEnvironment::default()),
    );

    assert_eq!(provider.key_status().await.unwrap(), ApiKeyStatus::Missing);
    assert_eq!(
        provider
            .save_key(SecretBytes::new(b"fixture-key".to_vec()))
            .await
            .unwrap(),
        ApiKeyStatus::Stored
    );
    assert_eq!(
        provider
            .save_key(SecretBytes::new(b"replacement-key".to_vec()))
            .await
            .unwrap(),
        ApiKeyStatus::Stored
    );
    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("Pay As You Go"));
    assert!(snapshot.lines.iter().any(|line| line.label() == "Balance"));
    assert_eq!(provider.delete_key().await.unwrap(), ApiKeyStatus::Missing);
}

#[tokio::test]
async fn both_auth_rejections_report_invalid_key() {
    let directory = tempfile::tempdir().unwrap();
    let mut environment = FakeEnvironment::default();
    environment.insert("OPENROUTER_API_KEY", b"secret-fixture-material".to_vec());
    let denied = || response(StatusCode::UNAUTHORIZED, b"{}");
    let provider = OpenRouterProvider::with_environment(
        &paths(&directory),
        Arc::new(FakeHttp(Mutex::new(HashMap::from([
            ("credits".to_owned(), VecDeque::from([denied()])),
            ("key".to_owned(), VecDeque::from([denied()])),
        ])))),
        Arc::new(environment),
    );

    assert_eq!(
        provider.key_status().await.unwrap(),
        ApiKeyStatus::FromEnvironment
    );
    assert_eq!(
        provider
            .save_key(SecretBytes::new(b"stored-override".to_vec()))
            .await
            .unwrap(),
        ApiKeyStatus::OverrideActive
    );
    assert_eq!(
        provider.delete_key().await.unwrap(),
        ApiKeyStatus::FromEnvironment
    );

    let error = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap_err();

    assert_eq!(error.category, ErrorCategory::AuthInvalid);
    assert!(!error.message.contains("secret-fixture-material"));

    let malformed = response(StatusCode::OK, b"{}");
    assert_eq!(
        response_data(&malformed).unwrap_err().category,
        ErrorCategory::Decoding
    );
}
