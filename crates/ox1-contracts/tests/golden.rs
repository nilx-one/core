// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use ox1_contracts::{CoreError, OperationId, canonical_json};

#[test]
fn unsupported_version_failure_matches_golden_bytes() {
    let operation_id: OperationId = "op_00000000000000000000000000000001"
        .parse()
        .expect("fixture operation id");
    let error = CoreError::unsupported_contract_version(
        Some(operation_id),
        Some("0.2.0".to_owned()),
        "0.1.0".to_owned(),
    );
    let actual = canonical_json(&error).expect("canonical failure bytes");
    let expected = include_str!("fixtures/unsupported-contract-version.json")
        .trim_end()
        .as_bytes();
    assert_eq!(actual, expected);

    let round_trip: CoreError = serde_json::from_slice(&actual).expect("failure must round-trip");
    assert_eq!(round_trip, error);
}
