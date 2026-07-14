use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use reqwest::header::HeaderMap;

use crate::platform::http::{HttpError, HttpRequest, HttpResponse, HttpTransport};

use super::*;

struct FakeHttp(Mutex<VecDeque<HttpResponse>>);

#[async_trait]
impl HttpTransport for FakeHttp {
    async fn send(
        &self,
        request: HttpRequest,
        _: CancellationToken,
    ) -> Result<HttpResponse, HttpError> {
        assert!(
            request
                .url
                .as_str()
                .ends_with("/exa.seat_management_pb.SeatManagementService/GetUserStatus")
        );
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| HttpError::Request("missing fixture".to_owned()))
    }
}

#[tokio::test]
async fn native_credentials_and_fixture_map_daily_weekly_and_balance() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("credentials.toml");
    std::fs::write(
        &path,
        "windsurf_api_key = 'fixture-key'\napi_server_url = 'https://server.codeium.com/'\n",
    )
    .unwrap();
    let provider = DevinProvider::new(
        DevinAuthStore::new(path, Vec::new()),
        Arc::new(FakeHttp(Mutex::new(VecDeque::from([HttpResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: include_bytes!(
                "../../../../../Tests/Fixtures/ProviderParity/v1/providers/devin/user-status.json"
            )
            .to_vec(),
        }])))),
    );

    let snapshot = provider
        .refresh_provider(CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(snapshot.plan.as_deref(), Some("Core"));
    assert_eq!(
        snapshot
            .lines
            .iter()
            .map(MetricLine::label)
            .collect::<Vec<_>>(),
        ["Daily quota", "Weekly quota", "Extra usage balance"]
    );
}

#[tokio::test]
async fn insecure_server_and_missing_sources_fail_distinctly() {
    assert_eq!(
        safe_server("http://example.com").unwrap_err().category,
        ErrorCategory::AuthInvalid
    );
    let directory = tempfile::tempdir().unwrap();
    let error = match DevinAuthStore::new(directory.path().join("missing"), Vec::new())
        .load_all()
        .await
    {
        Ok(_) => panic!("missing sources must not load"),
        Err(error) => error,
    };
    assert_eq!(error.category, ErrorCategory::NotLoggedIn);
    assert_eq!(
        map_usage(b"{}").unwrap_err().category,
        ErrorCategory::Decoding
    );
}
