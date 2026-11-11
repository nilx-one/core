// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;
use crate::error::ErrorEnvelope;

const VISIT: &str = "V8sKp3_ZtQ1rLw9-XyB6cNd4Hf2Gj7MaUoEe0TiRqYn";

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
fn visit_fixtures_round_trip() {
    let request: VisitRequest = pinned(include_str!("../../fixtures/market/visit_request.json"));
    assert_eq!(request.validate(), Ok("/telegram/"));
    let pickup: VisitRequest = pinned(include_str!(
        "../../fixtures/market/visit_request_pickup.json"
    ));
    assert_eq!(pickup.validate(), Ok("/"));
    assert_eq!(
        pickup.pickup_order_id.as_deref(),
        Some("order:2026.11-0042")
    );
    // An older client's balance still parses, and nothing reads it.
    let legacy: VisitRequest = pinned(include_str!(
        "../../fixtures/market/visit_request_legacy_seeds.json"
    ));
    assert_eq!(legacy.validate(), Ok("/"));
    let answer: VisitAnswer = pinned(include_str!("../../fixtures/market/visit_answer.json"));
    assert_eq!(answer.visit_id.as_deref(), Some(VISIT));
    assert_eq!(answer.code.as_str().len(), HANDOFF_CODE_LEN);
    let browse: VisitAnswer = pinned(include_str!(
        "../../fixtures/market/visit_answer_browse_only.json"
    ));
    assert_eq!(browse.visit_id, None);
    let error: ErrorEnvelope = pinned(include_str!(
        "../../fixtures/market/visit_error_invalid_context.json"
    ));
    assert_eq!(
        VisitRefusal::from_code(&error.error.code),
        Some(VisitRefusal::InvalidMarketContext)
    );
}

#[test]
fn return_fixtures_round_trip() {
    let answer: ReturnAnswer = pinned(include_str!("../../fixtures/market/return_answer.json"));
    assert!(answer.confirmed && !answer.item_delivery);
    let query = ReturnQuery {
        visit_id: answer.visit_id,
        order_id: answer.order_id,
    };
    assert_eq!(query.validate(), Ok(()));
    let error: ErrorEnvelope = pinned(include_str!(
        "../../fixtures/market/return_error_not_confirmed.json"
    ));
    assert_eq!(
        ReturnRefusal::from_code(&error.error.code),
        Some(ReturnRefusal::PurchaseNotConfirmed)
    );
}

#[test]
fn malformed_bodies_are_rejected() {
    rejected::<VisitRequest>(include_str!(
        "../../fixtures/market/rejected/visit_request_price.json"
    ));
    rejected::<VisitAnswer>(include_str!(
        "../../fixtures/market/rejected/visit_answer_short_code.json"
    ));
    // An answer may gain a field: a client ignores what it does not know.
    let future: ReturnAnswer = serde_json::from_str(include_str!(
        "../../fixtures/market/return_answer_future_field.json"
    ))
    .expect("tolerant");
    assert!(future.confirmed);
}

#[test]
fn refusal_codes_and_statuses_are_pinned() {
    let visit: Vec<_> = VisitRefusal::ALL
        .iter()
        .map(|r| (r.code(), r.http_status()))
        .collect();
    assert_eq!(
        visit,
        [
            ("invalid_market_context", 422),
            ("item_not_available", 409),
            ("market_visit_limit", 429)
        ]
    );
    let back: Vec<_> = ReturnRefusal::ALL
        .iter()
        .map(|r| (r.code(), r.http_status()))
        .collect();
    assert_eq!(
        back,
        [
            ("purchase_not_confirmed", 404),
            ("market_return_limit", 429)
        ]
    );
}

#[test]
fn a_visit_names_a_point_a_place_and_a_host_page() {
    let here = Position {
        longitude: 30.5,
        latitude: 50.4,
    };
    assert_eq!(
        VisitRequest::new("zoloti_vorota-1", here).validate(),
        Ok("/")
    );
    assert_eq!(VisitRequest::new("золоті", here).validate(), Ok("/"));
    assert_eq!(
        VisitRequest::new("a".repeat(POINT_MAX_CHARS), here).validate(),
        Ok("/")
    );
    for point in ["", "has space", "a/b", &"a".repeat(POINT_MAX_CHARS + 1)] {
        assert_eq!(
            VisitRequest::new(point, here).validate(),
            Err(VisitRefusal::InvalidMarketContext),
            "{point}"
        );
    }
    for (longitude, latitude) in [
        (180.5, 0.0),
        (0.0, 90.5),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        let request = VisitRequest::new(
            "zoloti",
            Position {
                longitude,
                latitude,
            },
        );
        assert_eq!(request.validate(), Err(VisitRefusal::InvalidMarketContext));
    }
    for path in [
        "https://evil.example/",
        "//evil.example/",
        "/a/b/",
        "/Telegram/",
        "telegram/",
        "/telegram",
    ] {
        let mut request = VisitRequest::new("zoloti", here);
        request.return_path = Some(path.to_owned());
        assert_eq!(
            request.validate(),
            Err(VisitRefusal::InvalidMarketContext),
            "{path}"
        );
    }
}

#[test]
fn ids_and_return_pages_are_shaped() {
    assert!(is_market_visit_id(VISIT));
    assert!(!is_market_visit_id(&VISIT[1..]));
    assert!(!is_market_visit_id(&VISIT.replace('_', "+")));
    assert!(is_market_order_id("order:2026.11-0042"));
    assert!(!is_market_order_id(""));
    assert!(!is_market_order_id("order 1"));
    assert!(!is_market_order_id(&"a".repeat(ORDER_ID_MAX_LEN + 1)));
    assert!(is_market_return_path("/"));
    assert!(is_market_return_path("/discord-activity/"));
    assert!(!is_market_return_path(""));
    let malformed = ReturnQuery {
        visit_id: "short".to_owned(),
        order_id: "o1".to_owned(),
    };
    assert_eq!(
        malformed.validate(),
        Err(ReturnRefusal::PurchaseNotConfirmed)
    );
}
