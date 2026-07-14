use std::collections::HashMap;
use std::sync::Arc;

use crate::providers::{FixedPricing, ModelPrice};

use super::*;

#[test]
fn credits_fixture_maps_weekly_and_pay_as_you_go() {
    let lines = map_credits(include_bytes!(
        "../../../../../Tests/Fixtures/ProviderParity/v1/providers/grok/credits.json"
    ))
    .unwrap();
    assert_eq!(
        lines.iter().map(MetricLine::label).collect::<Vec<_>>(),
        ["Weekly limit", "Pay as you go"]
    );
}

#[tokio::test]
async fn incremental_log_history_prices_only_attributed_rows() {
    let directory = tempfile::tempdir().unwrap();
    let log = directory.path().join("unified.jsonl");
    let day = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339();
    std::fs::write(&log, format!(
        "{{\"pid\":7,\"msg\":\"model changed\",\"ctx\":{{\"model\":\"grok-test\"}}}}\n{{\"pid\":7,\"ts\":\"{day}\",\"msg\":\"shell.turn.inference_done\",\"ctx\":{{\"prompt_tokens\":100,\"completion_tokens\":20}}}}\n"
    )).unwrap();
    let pricing = FixedPricing::new(HashMap::from([(
        "GROK-TEST".to_owned(),
        ModelPrice {
            input_per_million: 1.0,
            cached_input_per_million: 0.1,
            output_per_million: 2.0,
        },
    )]));

    let daily = GrokHistoryScanner::default()
        .scan(log, Arc::new(pricing))
        .await
        .unwrap();

    assert_eq!(daily.values().next().unwrap().tokens, 120.0);
    assert!(daily.values().next().unwrap().has_cost);
}

#[tokio::test]
async fn malformed_and_missing_auth_are_distinct() {
    let directory = tempfile::tempdir().unwrap();
    let missing = GrokAuthStore::new(directory.path().join("missing.json"));
    let missing_error = match missing.load_all().await {
        Ok(_) => panic!("missing auth must fail"),
        Err(error) => error,
    };
    assert_eq!(missing_error.category, ErrorCategory::NotLoggedIn);
    let malformed_path = directory.path().join("auth.json");
    std::fs::write(&malformed_path, "not-json").unwrap();
    let malformed = GrokAuthStore::new(malformed_path);
    let malformed_error = match malformed.load_all().await {
        Ok(_) => panic!("malformed auth must fail"),
        Err(error) => error,
    };
    assert_eq!(malformed_error.category, ErrorCategory::AuthInvalid);
}
