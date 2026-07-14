#![cfg(windows)]

use std::sync::Arc;

use openusage_windows_lib::contracts::Validate;
use openusage_windows_lib::platform::http::{BoundedHttpClient, HttpClientConfig, HttpTransport};
use openusage_windows_lib::platform::paths::WindowsPaths;
use openusage_windows_lib::providers::FixedPricing;
use openusage_windows_lib::providers::claude::{ClaudeAuthStore, ClaudeProvider};
use openusage_windows_lib::providers::codex::{CodexAuthStore, CodexProvider};
use openusage_windows_lib::providers::cursor::{CursorAuthStore, CursorProvider};
use openusage_windows_lib::runtime::refresh::ProviderRuntime;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn native_provider_probe_reports_only_safe_outcomes() {
    if std::env::var_os("OPENUSAGE_RUN_NATIVE_PROVIDER_PROBE").is_none() {
        return;
    }

    let paths = WindowsPaths::from_environment().expect("native Windows paths");
    let http: Arc<dyn HttpTransport> =
        Arc::new(BoundedHttpClient::new(HttpClientConfig::default()).expect("bounded HTTP client"));
    let pricing = Arc::new(FixedPricing::default());
    let providers: Vec<Arc<dyn ProviderRuntime>> = vec![
        Arc::new(ClaudeProvider::new(
            ClaudeAuthStore::from_windows(&paths).expect("Claude source configuration"),
            http.clone(),
            pricing.clone(),
        )),
        Arc::new(CodexProvider::new(
            CodexAuthStore::from_windows(&paths).expect("Codex source configuration"),
            http.clone(),
            pricing.clone(),
        )),
        Arc::new(CursorProvider::new(
            CursorAuthStore::from_windows(&paths),
            http,
            pricing,
        )),
    ];

    for provider in providers {
        let provider_id = provider.provider_id().to_owned();
        match provider.refresh(CancellationToken::new()).await {
            Ok(snapshot) => {
                snapshot.validate().expect("provider snapshot contract");
                println!(
                    "provider={provider_id} outcome=success metrics={} history={}",
                    snapshot.lines.len(),
                    snapshot.usage_history.is_some()
                );
            }
            Err(failure) => {
                println!(
                    "provider={provider_id} outcome=failure category={}",
                    failure.category
                );
            }
        }
    }
}
