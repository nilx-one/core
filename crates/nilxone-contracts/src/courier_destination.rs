// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Where a courier such as xPing goes: the nearest of the `BusinessPoint`s a
//! host offers, chosen the same way by every runtime.
//!
//! The host hands in where the trip starts and the points eligible for it;
//! Core answers which one. A point is referenced by its stable id, opaque
//! here: how a `BusinessPoint` id is encoded belongs to the `BusinessPoint`
//! contract, not to this selector. Choosing a point owns no tier, flight
//! time, estimated arrival or route, and never proves a subscription, a
//! payment, an arrival or presence (0x1 `product-direction/xping-courier.md`).

use serde::{Deserialize, Serialize};

use crate::GEO_COORDINATE_E7_SCALE;
use crate::GeoCoordinate;
use crate::spoken_line::{CENTIMETERS_PER_DEGREE, cosine_millionths};

/// Version of the selection rules below; any change to them raises it.
pub const COURIER_DESTINATION_VERSION: u32 = 1;
/// The most candidates one request may offer: one owner's full set of
/// `UInt8`-numbered points.
pub const MAX_COURIER_CANDIDATES: usize = 256;
/// The longest point id, in Unicode scalar values.
pub const MAX_COURIER_POINT_ID_SCALARS: usize = 128;

const E7: i128 = GEO_COORDINATE_E7_SCALE as i128;
const COSINE_SCALE: i128 = 1_000_000;

/// One point a courier may go to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourierCandidate {
    /// The point's stable id.
    pub point_id: String,
    /// Where the point stands.
    pub coordinate: GeoCoordinate,
}

/// Where a trip starts and the points it may go to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CourierDestinationRequest {
    /// Where the trip starts, captured once by the host.
    pub origin: GeoCoordinate,
    /// The points eligible in the host's context.
    pub candidates: Vec<CourierCandidate>,
}

/// Stable failure classification for destination selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CourierDestinationError {
    /// No candidate was offered.
    NoCandidates,
    /// More than [`MAX_COURIER_CANDIDATES`] were offered.
    TooManyCandidates,
    /// A point id is empty, too long, or holds whitespace or a control.
    PointId,
    /// Two candidates share a point id.
    DuplicatePointId,
}

impl CourierDestinationError {
    /// The wire code for this failure.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NoCandidates => "no_candidates",
            Self::TooManyCandidates => "too_many_candidates",
            Self::PointId => "point_id",
            Self::DuplicatePointId => "duplicate_point_id",
        }
    }
}

impl core::fmt::Display for CourierDestinationError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::NoCandidates => "no courier destination candidates",
            Self::TooManyCandidates => "too many courier destination candidates",
            Self::PointId => "not a valid point id",
            Self::DuplicatePointId => "duplicate point id",
        })
    }
}

impl std::error::Error for CourierDestinationError {}

fn valid_point_id(id: &str) -> bool {
    let scalars = id.chars().count();
    (1..=MAX_COURIER_POINT_ID_SCALARS).contains(&scalars)
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The squared ground distance in square centimetres: the equirectangular
/// integer approximation of `distance_meters`, without rounding to metres,
/// so points a few centimetres apart still order. Longitude wraps across the
/// dateline.
fn squared_cm(from: GeoCoordinate, to: GeoCoordinate) -> u128 {
    let full_turn = 360 * i128::from(GEO_COORDINATE_E7_SCALE);
    let mut east = i128::from(to.longitude_e7()) - i128::from(from.longitude_e7());
    if east > full_turn / 2 {
        east -= full_turn;
    } else if east < -full_turn / 2 {
        east += full_turn;
    }
    let north = i128::from(to.latitude_e7()) - i128::from(from.latitude_e7());
    let middle = (i64::from(to.latitude_e7()) + i64::from(from.latitude_e7())).abs() / 2;
    let north_cm = north * CENTIMETERS_PER_DEGREE / E7;
    let east_cm = east * CENTIMETERS_PER_DEGREE * cosine_millionths(middle) / (E7 * COSINE_SCALE);
    (north_cm * north_cm + east_cm * east_cm).unsigned_abs()
}

/// The nearest candidate to the origin; of equally near ones, the smallest
/// point id by code point. The answer never depends on the candidates' order.
///
/// # Errors
///
/// Rejects an empty or oversized list, an invalid point id and a duplicate
/// point id, so a malformed projection never silently picks a point.
pub fn courier_destination(
    request: &CourierDestinationRequest,
) -> Result<&CourierCandidate, CourierDestinationError> {
    if request.candidates.len() > MAX_COURIER_CANDIDATES {
        return Err(CourierDestinationError::TooManyCandidates);
    }
    let mut ids = std::collections::BTreeSet::new();
    for candidate in &request.candidates {
        if !valid_point_id(&candidate.point_id) {
            return Err(CourierDestinationError::PointId);
        }
        if !ids.insert(candidate.point_id.as_str()) {
            return Err(CourierDestinationError::DuplicatePointId);
        }
    }
    request
        .candidates
        .iter()
        .min_by(|a, b| {
            squared_cm(request.origin, a.coordinate)
                .cmp(&squared_cm(request.origin, b.coordinate))
                .then_with(|| a.point_id.cmp(&b.point_id))
        })
        .ok_or(CourierDestinationError::NoCandidates)
}

/// [`courier_destination`] as JSON for Wasm and `UniFFI`:
/// `{"ok":true,"point_id":…}`, `{"ok":false,"error":"invalid"}` for input
/// that does not parse, or `{"ok":false,"error":<code>}` for a rejected one.
#[must_use]
pub fn courier_destination_wire(request: &str) -> String {
    let answer = match serde_json::from_str::<CourierDestinationRequest>(request) {
        Ok(request) => match courier_destination(&request) {
            Ok(chosen) => serde_json::json!({ "ok": true, "point_id": chosen.point_id }),
            Err(error) => serde_json::json!({ "ok": false, "error": error.code() }),
        },
        Err(_) => serde_json::json!({ "ok": false, "error": "invalid" }),
    };
    answer.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(longitude_e7: i32, latitude_e7: i32) -> GeoCoordinate {
        GeoCoordinate::new(longitude_e7, latitude_e7).expect("coordinate")
    }

    fn candidate(id: &str, longitude_e7: i32, latitude_e7: i32) -> CourierCandidate {
        CourierCandidate {
            point_id: id.to_owned(),
            coordinate: point(longitude_e7, latitude_e7),
        }
    }

    fn chosen(origin: GeoCoordinate, candidates: Vec<CourierCandidate>) -> String {
        courier_destination(&CourierDestinationRequest { origin, candidates })
            .expect("destination")
            .point_id
            .clone()
    }

    /// Golden Gate, Kyiv.
    fn kyiv() -> GeoCoordinate {
        point(305_130_000, 504_490_000)
    }

    #[test]
    fn the_nearest_point_is_chosen_whatever_the_order() {
        let near = candidate("near", 305_140_000, 504_495_000);
        let far = candidate("far", 306_000_000, 505_000_000);
        let middle = candidate("middle", 305_300_000, 504_600_000);
        let mut candidates = vec![far, near, middle];
        assert_eq!(chosen(kyiv(), candidates.clone()), "near");
        candidates.reverse();
        assert_eq!(chosen(kyiv(), candidates.clone()), "near");
        candidates.rotate_left(1);
        assert_eq!(chosen(kyiv(), candidates), "near");
    }

    #[test]
    fn equally_near_points_choose_the_smallest_id() {
        // The same spot twice, and one mirrored due north and due south.
        let candidates = vec![
            candidate("b", 305_130_000, 504_500_000),
            candidate("a", 305_130_000, 504_500_000),
            candidate("c", 305_130_000, 504_480_000),
        ];
        assert_eq!(chosen(kyiv(), candidates.clone()), "a");
        let mut reversed = candidates;
        reversed.reverse();
        assert_eq!(chosen(kyiv(), reversed), "a");
    }

    #[test]
    fn centimetres_order_points_that_whole_metres_would_tie() {
        // About 1.11 m and 1.22 m north: the same whole metre.
        let candidates = vec![
            candidate("a-farther", 305_130_000, 504_490_110),
            candidate("b-nearer", 305_130_000, 504_490_100),
        ];
        assert_eq!(chosen(kyiv(), candidates), "b-nearer");
    }

    #[test]
    fn longitude_wraps_across_the_dateline() {
        let origin = point(1_799_000_000, 0);
        let candidates = vec![
            candidate("across", -1_799_000_000, 0),
            candidate("same-side", 1_790_000_000, 0),
        ];
        assert_eq!(chosen(origin, candidates), "across");
    }

    #[test]
    fn malformed_projections_never_pick_a_point() {
        let request = |candidates| CourierDestinationRequest {
            origin: kyiv(),
            candidates,
        };
        assert_eq!(
            courier_destination(&request(Vec::new())),
            Err(CourierDestinationError::NoCandidates)
        );
        assert_eq!(
            courier_destination(&request(vec![candidate("a", 0, 0), candidate("a", 1, 1),])),
            Err(CourierDestinationError::DuplicatePointId)
        );
        for bad in ["", "two words", "tab\there", &"x".repeat(129)] {
            assert_eq!(
                courier_destination(&request(vec![candidate(bad, 0, 0)])),
                Err(CourierDestinationError::PointId),
                "{bad:?}"
            );
        }
        let many = (0..=MAX_COURIER_CANDIDATES)
            .map(|i| candidate(&format!("p{i}"), 0, 0))
            .collect();
        assert_eq!(
            courier_destination(&request(many)),
            Err(CourierDestinationError::TooManyCandidates)
        );
        let full = (0..MAX_COURIER_CANDIDATES)
            .map(|i| candidate(&format!("p{i:03}"), 0, 0))
            .collect();
        assert_eq!(
            courier_destination(&request(full)).map(|c| c.point_id.as_str()),
            Ok("p000")
        );
    }

    #[test]
    fn the_wire_answers_only_a_point_id() {
        let request = r#"{"origin":{"longitude_e7":"305130000","latitude_e7":"504490000"},
            "candidates":[
              {"point_id":"0xda-market-zoloti-vorota","coordinate":{"longitude_e7":"305131000","latitude_e7":"504491000"}},
              {"point_id":"0xda-market-lavina","coordinate":{"longitude_e7":"303740000","latitude_e7":"504940000"}}
            ]}"#;
        assert_eq!(
            courier_destination_wire(request),
            r#"{"ok":true,"point_id":"0xda-market-zoloti-vorota"}"#
        );
        assert_eq!(
            courier_destination_wire(
                r#"{"origin":{"longitude_e7":"0","latitude_e7":"0"},"candidates":[]}"#
            ),
            r#"{"error":"no_candidates","ok":false}"#
        );
        // Unknown fields, such as a tier or flight time, are not part of it.
        assert_eq!(
            courier_destination_wire(
                r#"{"origin":{"longitude_e7":"0","latitude_e7":"0"},"candidates":[],"tier":"exclusive"}"#
            ),
            r#"{"error":"invalid","ok":false}"#
        );
        assert_eq!(
            courier_destination_wire("{}"),
            r#"{"error":"invalid","ok":false}"#
        );
    }
}
