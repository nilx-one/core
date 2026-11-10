// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;
use crate::error::ErrorEnvelope;

/// Parses a fixture and writes it back: the contract's types must reproduce the
/// pinned JSON exactly, so a client in another language can rely on it.
fn pinned<T: Serialize + DeserializeOwned>(fixture: &str) -> T {
    let expected: Value = serde_json::from_str(fixture).expect("fixture is JSON");
    let parsed: T = serde_json::from_str(fixture).expect("fixture parses");
    assert_eq!(serde_json::to_value(&parsed).expect("serializes"), expected);
    parsed
}

fn rejected<T: DeserializeOwned>(fixture: &str) {
    assert!(
        serde_json::from_str::<T>(fixture).is_err(),
        "accepted: {fixture}"
    );
}

#[test]
fn attest_fixtures_round_trip() {
    let request: AttestRequest = pinned(include_str!("../../fixtures/finds/attest_request.json"));
    assert_eq!(request.validate().expect("valid").longitude_e7, 305_234_000);
    let attested: AttestAnswer = pinned(include_str!(
        "../../fixtures/finds/attest_answer_attested.json"
    ));
    assert_eq!(attested.outcome, AttestOutcome::Attested);
    assert_eq!(attested.seeds_credited, Seeds(25));
    let again: AttestAnswer = pinned(include_str!(
        "../../fixtures/finds/attest_answer_already_yours.json"
    ));
    assert_eq!(again.outcome, AttestOutcome::AlreadyYours);
    assert_eq!(again.seeds_credited, Seeds(0));
    let error: ErrorEnvelope = pinned(include_str!("../../fixtures/finds/attest_error_taken.json"));
    assert_eq!(
        AttestRefusal::from_code(&error.error.code),
        Some(AttestRefusal::Taken)
    );
}

#[test]
fn sell_fixtures_round_trip() {
    let request: SellRequest = pinned(include_str!("../../fixtures/finds/sell_request.json"));
    assert_eq!(request.validate(), Ok(()));
    let sold: SellAnswer = pinned(include_str!("../../fixtures/finds/sell_answer_sold.json"));
    assert_eq!(
        (sold.outcome, sold.seeds_credited),
        (SaleOutcome::Sold, Seeds(40))
    );
    let again: SellAnswer = pinned(include_str!(
        "../../fixtures/finds/sell_answer_already_sold.json"
    ));
    assert_eq!(again.outcome, SaleOutcome::AlreadySold);
    let error: ErrorEnvelope = pinned(include_str!("../../fixtures/finds/sell_error_used_up.json"));
    assert_eq!(
        SaleRefusal::from_code(&error.error.code),
        Some(SaleRefusal::UsedUp)
    );
}

#[test]
fn craft_fixtures_round_trip() {
    let request: CraftRequest = pinned(include_str!("../../fixtures/finds/craft_request.json"));
    assert_eq!(request.validate(), Ok(()));
    assert_eq!(request.inputs.len(), 2);
    let started: CraftAnswer = pinned(include_str!(
        "../../fixtures/finds/craft_answer_started.json"
    ));
    assert_eq!(
        (started.outcome, started.seeds_debited),
        (CraftStart::Started, Seeds(120))
    );
    let again: CraftAnswer = pinned(include_str!(
        "../../fixtures/finds/craft_answer_already_started.json"
    ));
    assert_eq!(again.outcome, CraftStart::AlreadyStarted);
    let error: ErrorEnvelope = pinned(include_str!(
        "../../fixtures/finds/craft_error_not_enough_seeds.json"
    ));
    assert_eq!(
        CraftRefusal::from_code(&error.error.code),
        Some(CraftRefusal::NotEnoughSeeds)
    );
}

#[test]
fn malformed_bodies_are_rejected() {
    rejected::<AttestRequest>(include_str!(
        "../../fixtures/finds/rejected/attest_request_unknown_field.json"
    ));
    rejected::<AttestAnswer>(include_str!(
        "../../fixtures/finds/rejected/attest_answer_leading_zero.json"
    ));
    rejected::<AttestAnswer>(include_str!(
        "../../fixtures/finds/rejected/attest_answer_bad_uuid.json"
    ));
    rejected::<SellAnswer>(include_str!(
        "../../fixtures/finds/rejected/sell_answer_number_amount.json"
    ));
    rejected::<CraftAnswer>(include_str!(
        "../../fixtures/finds/rejected/craft_answer_overflow.json"
    ));
}

#[test]
fn refusal_codes_and_statuses_are_pinned() {
    let attest: Vec<_> = AttestRefusal::ALL
        .iter()
        .map(|r| (r.code(), r.http_status()))
        .collect();
    assert_eq!(
        attest,
        [
            ("taken", 409),
            ("unknown_find", 422),
            ("out_of_time", 422),
            ("too_far", 422),
            ("not_the_avaias", 422),
            ("invalid_pickup", 422),
        ]
    );
    let sale: Vec<_> = SaleRefusal::ALL
        .iter()
        .map(|r| (r.code(), r.http_status()))
        .collect();
    assert_eq!(
        sale,
        [
            ("unknown_attestation", 404),
            ("not_for_sale", 409),
            ("used_up", 409)
        ]
    );
    let craft: Vec<_> = CraftRefusal::ALL
        .iter()
        .map(|r| (r.code(), r.http_status()))
        .collect();
    assert_eq!(
        craft,
        [
            ("invalid_craft", 422),
            ("unknown_recipe", 404),
            ("not_enough_seeds", 409),
            ("wrong_inputs", 409),
            ("unverifiable_inputs", 409),
            ("conflicting_replay", 409),
        ]
    );
    for refusal in AttestRefusal::ALL {
        assert_eq!(AttestRefusal::from_code(refusal.code()), Some(*refusal));
    }
    assert_eq!(AttestRefusal::from_code("taken "), None);
    assert_eq!(CraftRefusal::from_code("inactive"), None);
}

#[test]
fn a_pickup_must_name_a_place_on_earth_and_a_known_finder() {
    let ok = AttestRequest::new("cell-1", Finder::Avaia, 5, -180.0, 90.0);
    assert_eq!(
        ok.validate()
            .map(|p| (p.finder, p.longitude_e7, p.latitude_e7)),
        Ok((Finder::Avaia, -1_800_000_000, 900_000_000))
    );
    for (longitude, latitude) in [
        (180.1, 0.0),
        (0.0, -90.1),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        let bad = AttestRequest::new("cell-1", Finder::Bond, 5, longitude, latitude);
        assert_eq!(bad.validate(), Err(AttestRefusal::InvalidPickup));
    }
    let mut stranger = AttestRequest::new("cell-1", Finder::Bond, 5, 0.0, 0.0);
    stranger.finder = "robot".to_owned();
    assert_eq!(stranger.validate(), Err(AttestRefusal::InvalidPickup));
    let long = AttestRequest::new(
        "x".repeat(ARTIFACT_ID_MAX_LEN + 1),
        Finder::Bond,
        5,
        0.0,
        0.0,
    );
    assert_eq!(long.validate(), Err(AttestRefusal::InvalidPickup));
    let longest = AttestRequest::new("x".repeat(ARTIFACT_ID_MAX_LEN), Finder::Bond, 5, 0.0, 0.0);
    assert!(longest.validate().is_ok());
}

#[test]
fn a_craft_names_a_sane_id_and_at_most_eight_inputs() {
    let id =
        AttestationId::try_from("3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b".to_owned()).expect("uuid");
    assert_eq!(
        CraftRequest::new("abcdefgh", "lantern", std::slice::from_ref(&id)).validate(),
        Ok(())
    );
    assert_eq!(
        CraftRequest::new("a".repeat(64), "lantern", &[]).validate(),
        Ok(())
    );
    for craft_id in ["short", &"a".repeat(65), "has space 1", "ünïcode-id"] {
        assert_eq!(
            CraftRequest::new(craft_id, "lantern", &[]).validate(),
            Err(CraftRefusal::InvalidCraft)
        );
    }
    let nine = vec![id; CRAFT_INPUTS_MAX + 1];
    assert_eq!(
        CraftRequest::new("abcdefgh", "lantern", &nine).validate(),
        Err(CraftRefusal::InvalidCraft)
    );
}

#[test]
fn a_sale_names_an_id_of_attestation_length() {
    let short = SellRequest {
        attestation_id: "3f2b".to_owned(),
    };
    assert_eq!(short.validate(), Err(SaleRefusal::UnknownAttestation));
}

#[test]
fn seeds_are_canonical_decimal_strings() {
    for ok in ["0", "7", "9999999999999999999"] {
        assert!(Seeds::try_from(ok.to_owned()).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "00",
        "01",
        "-1",
        "+1",
        "1.0",
        " 1",
        "1e3",
        "10000000000000000000",
        "٣",
    ] {
        assert!(Seeds::try_from(bad.to_owned()).is_err(), "{bad}");
    }
    assert_eq!(
        String::from(Seeds(18_446_744_073_709_551_615 / 2)),
        "9223372036854775807"
    );
}

#[test]
fn ids_are_shaped() {
    assert!(AttestationId::try_from("3f2b8c1e-9a4d-4e6f-8b7a-1c2d3e4f5a6b".to_owned()).is_ok());
    for bad in [
        "3F2B8C1E-9A4D-4E6F-8B7A-1C2D3E4F5A6B",
        "3f2b8c1e-9a4d-1e6f-8b7a-1c2d3e4f5a6b",
        "3f2b8c1e-9a4d-4e6f-cb7a-1c2d3e4f5a6b",
        "3f2b8c1e9a4d4e6f8b7a1c2d3e4f5a6b0000",
    ] {
        assert!(AttestationId::try_from(bad.to_owned()).is_err(), "{bad}");
    }
    assert!(ItemId::try_from("seed_pouch".to_owned()).is_ok());
    for bad in ["", "Seed", "seed-pouch", &"a".repeat(65)] {
        assert!(ItemId::try_from(bad.to_owned()).is_err(), "{bad}");
    }
}
