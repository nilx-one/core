// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Presentation-only pickup choices for an Identity-attested paid Special item.
//! The authority must supply `paid`; this projection is never proof of payment,
//! arrival, consent, a Market visit, a `BondChain` or subscription activation.
//! The host supplies the in-world Avaia and existing `BusinessPoint` coordinates
//! and a successfully planned pedestrian route. No GPS-derived physical visit.

use serde::{Deserialize, Serialize};

use crate::{GeoCoordinate, distance_meters};

/// Strictly *less* than 300 in-world metres is eligible to suggest Avaia.
pub const AVAIA_SPECIAL_PICKUP_WALK_RADIUS_METERS: u32 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpecialPickupRequest {
    /// Must originate in an authenticated Identity paid-item projection.
    pub paid: bool,
    /// The same `BusinessPoint` as Market's visit destination.
    pub point: GeoCoordinate,
    /// The Avaia's current in-world position, not device geolocation.
    pub avaia: Option<GeoCoordinate>,
    /// True only after the existing walk graph has found a route.
    pub walk_route_available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialPickupChoices {
    /// A choice to dispatch, not a dispatch itself.
    pub offer_xping: bool,
    /// An optional, user-confirmed walking suggestion.
    pub suggest_avaia_walk: bool,
    /// Whole in-world metres, or none if Avaia's position is unknown.
    pub avaia_distance_m: Option<u32>,
}

/// Deterministic UI choices. The game must separately confirm user action
/// and any collection through the existing Identity receipt/Market visit.
#[must_use]
pub fn special_pickup_choices(request: &SpecialPickupRequest) -> SpecialPickupChoices {
    let distance = request
        .avaia
        .map(|avaia| distance_meters(avaia, request.point));
    SpecialPickupChoices {
        offer_xping: request.paid,
        suggest_avaia_walk: request.paid
            && request.walk_route_available
            && distance.is_some_and(|d| d < AVAIA_SPECIAL_PICKUP_WALK_RADIUS_METERS),
        avaia_distance_m: distance,
    }
}

/// Wasm/UniFFI shared JSON projection. Coordinates are canonical E7 strings.
/// All distance values are decimal strings (Core wire contract v0).
/// Malformed input explicitly fails closed; no fallback grants a pickup.
#[must_use]
pub fn special_pickup_choices_wire(input: &str) -> String {
    let Ok(request) = serde_json::from_str::<SpecialPickupRequest>(input) else {
        return r#"{"ok":false,"error":"invalid"}"#.to_owned();
    };
    let choices = special_pickup_choices(&request);
    serde_json::json!({
        "ok": true,
        "offer_xping": choices.offer_xping,
        "suggest_avaia_walk": choices.suggest_avaia_walk,
        "avaia_distance_m": choices.avaia_distance_m.map(|distance| distance.to_string()),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(latitude: f64) -> GeoCoordinate {
        GeoCoordinate::from_degrees(30.5131, latitude).expect("valid Kyiv point")
    }

    fn request(latitude: Option<f64>, paid: bool, route: bool) -> SpecialPickupRequest {
        SpecialPickupRequest {
            paid,
            point: at(50.4489),
            avaia: latitude.map(at),
            walk_route_available: route,
        }
    }

    #[test]
    fn a_paid_item_suggests_avaia_only_inside_the_strict_radius() {
        let near = special_pickup_choices(&request(Some(50.45158), true, true));
        assert!(near.offer_xping);
        assert!(near.suggest_avaia_walk);
        assert!(near.avaia_distance_m.unwrap() < 300);
        let boundary = special_pickup_choices(&request(Some(50.451595), true, true));
        assert_eq!(boundary.avaia_distance_m, Some(300));
        assert!(!boundary.suggest_avaia_walk);
        let far = special_pickup_choices(&request(Some(50.45161), true, true));
        assert!(!far.suggest_avaia_walk);
        assert!(far.offer_xping);
    }

    #[test]
    fn no_paid_receipt_or_route_or_avaia_never_suggests_walking() {
        for test in [
            request(Some(50.44891), false, true),
            request(Some(50.44891), true, false),
            request(None, true, true),
        ] {
            let choices = special_pickup_choices(&test);
            assert!(!choices.suggest_avaia_walk);
            assert_eq!(choices.offer_xping, test.paid);
        }
    }

    #[test]
    fn malformed_or_untrusted_inputs_fail_closed() {
        assert_eq!(
            special_pickup_choices_wire("{}"),
            r#"{"ok":false,"error":"invalid"}"#
        );
        assert_eq!(
            special_pickup_choices_wire(
                r#"{"paid":true,"point":{"longitude_e7":305131000,"latitude_e7":"504489000"},"avaia":null,"walk_route_available":true}"#
            ),
            r#"{"ok":false,"error":"invalid"}"#
        );
        let valid = serde_json::to_string(&request(Some(50.45158), true, true)).unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&special_pickup_choices_wire(&valid)).unwrap();
        assert_eq!(result["offer_xping"], true);
        assert_eq!(result["suggest_avaia_walk"], true);
        assert!(result["avaia_distance_m"].is_string());
    }
}
