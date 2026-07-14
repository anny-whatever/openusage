use openusage_windows_lib::contracts::{
    ErrorCategory, LimitEnvelope, ProviderSnapshotEnvelope, Validate,
};

const FIXTURE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../Tests/Fixtures/ProviderParity/v1"
);

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURE_ROOT}/{name}"))
        .unwrap_or_else(|error| panic!("failed to read {name}: {error}"))
}

#[test]
fn valid_snapshot_round_trips_without_inventing_missing_values() {
    let envelope: ProviderSnapshotEnvelope =
        serde_json::from_str(&read_fixture("snapshot-valid.json")).expect("fixture should decode");

    envelope.validate().expect("fixture should validate");
    let encoded = serde_json::to_value(envelope).expect("fixture should encode");
    let snapshot = &encoded["snapshot"];
    assert!(snapshot.get("errorCategory").is_none());
    assert_eq!(
        snapshot["lines"][1]["values"][0]["number"].as_f64(),
        Some(0.0)
    );
}

#[test]
fn valid_limits_preserve_zero_and_omit_absent_scalars() {
    let envelope: LimitEnvelope =
        serde_json::from_str(&read_fixture("limits-valid.json")).expect("fixture should decode");

    envelope.validate().expect("fixture should validate");
    let encoded = serde_json::to_value(envelope).expect("fixture should encode");
    assert_eq!(encoded["resources"][1]["used"].as_f64(), Some(0.0));
    assert!(encoded["resources"][1].get("limit").is_none());
}

#[test]
fn malformed_snapshot_boundaries_fail_loudly() {
    for name in [
        "snapshot-invalid-negative.json",
        "snapshot-invalid-date.json",
        "snapshot-invalid-duplicate.json",
    ] {
        let envelope: ProviderSnapshotEnvelope =
            serde_json::from_str(&read_fixture(name)).expect("fixture shape should decode");
        assert!(
            envelope.validate().is_err(),
            "{name} should fail validation"
        );
    }
}

#[test]
fn malformed_limits_shape_fails_loudly() {
    let error = serde_json::from_str::<LimitEnvelope>(&read_fixture("limits-invalid-missing.json"))
        .expect_err("missing label must fail decoding");

    assert!(error.to_string().contains("label"));
}

#[test]
fn limits_expiry_and_error_category_wire_values_are_stable() {
    let mut value: serde_json::Value =
        serde_json::from_str(&read_fixture("limits-valid.json")).unwrap();
    value["expiresAt"] = value["fetchedAt"].clone();
    let envelope: LimitEnvelope = serde_json::from_value(value).unwrap();

    assert!(envelope.validate().is_err());
    assert_eq!(
        serde_json::to_string(&ErrorCategory::Http4xx).unwrap(),
        "\"http_4xx\""
    );
}
