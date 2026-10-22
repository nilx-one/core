// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Deterministic map projection of curated landmarks; never a new place or
//! presence record. Input geography is host-supplied and must be verified
//! against the same published archive as a rated cell spill.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::landmark_rarity::{REGISTRIES, Registry, rarity_in, registry_in};
use crate::{CellLandmark, GeoCoordinate, RarityV3, is_landmark_id};

/// Bounds one projection request: hosts must page or clip archive queries.
pub const MAX_RARITY_MAP_LANDMARKS: usize = 4096;

/// Which archive features may be projected; no host-assigned rarity is accepted.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LandmarkMapRequest {
    registry: u32,
    landmarks: Vec<CellLandmark>,
}

#[derive(Debug, Serialize)]
struct LandmarkMapItem {
    id: String,
    at: GeoCoordinate,
    rarity: &'static str,
}

/// Filters every archive-verified landmark to Rare, Special or Exclusive,
/// retaining its exact archive identifier and coordinate. Input order cannot
/// influence output, and duplicate identities may not move a pin.
///
/// This is a presentation-only *projection*, not an authenticity check:
/// a host still must verify its input ids, coordinates, archive revision and
/// published registry before exposing a trusted map overlay.
fn map_items_in(
    registries: &[Registry],
    request: LandmarkMapRequest,
) -> Option<Vec<LandmarkMapItem>> {
    registry_in(registries, request.registry)?;
    if request.landmarks.len() > MAX_RARITY_MAP_LANDMARKS {
        return None;
    }
    let mut landmarks = request.landmarks;
    landmarks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut result: Vec<LandmarkMapItem> = Vec::new();
    let mut last: Option<(&str, GeoCoordinate)> = None;
    for landmark in &landmarks {
        if !is_landmark_id(&landmark.id) {
            return None;
        }
        if let Some((id, at)) = last
            && id == landmark.id
        {
            if at != landmark.at {
                return None;
            }
            continue;
        }
        last = Some((landmark.id.as_str(), landmark.at));
        let rarity = rarity_in(registries, request.registry, &landmark.id)?;
        if rarity >= RarityV3::Rare {
            result.push(LandmarkMapItem {
                id: landmark.id.clone(),
                at: landmark.at,
                rarity: rarity.code(),
            });
        }
    }
    Some(result)
}

/// Returns a sorted, untruncated projection containing *all* verified Rare,
/// Special and Exclusive candidates the host provided. No synthetic place,
/// computed rarity or device location is persisted.
///
/// # Wire
///
/// Input: `{"registry":1,"landmarks":[{"id":"n123","at":{"longitude_e7":"304...","latitude_e7":"504..."}}]}`.
/// Output: `{"ok":true,"registry":1,"landmarks":[{"id":"...","at":{...},"rarity":"rare"}]}`.
/// Failure: `{"ok":false,"error":"invalid"}`. Invalid data never becomes
/// empty-success, except a genuinely empty qualifying registry or query.
#[must_use]
pub fn landmark_map_view_wire(input: &str) -> String {
    let Ok(request) = serde_json::from_str::<LandmarkMapRequest>(input) else {
        return r#"{"error":"invalid","ok":false}"#.to_owned();
    };
    let registry = request.registry;
    let Some(landmarks) = map_items_in(&REGISTRIES, request) else {
        return r#"{"error":"invalid","ok":false}"#.to_owned();
    };
    json!({"ok": true, "registry": registry, "landmarks": landmarks}).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point() -> GeoCoordinate {
        GeoCoordinate::new(304_461_000, 504_650_000).expect("valid coordinate")
    }

    fn landmark(id: &str, at: GeoCoordinate) -> CellLandmark {
        CellLandmark {
            id: id.to_owned(),
            at,
        }
    }

    fn rated() -> [Registry; 2] {
        [
            REGISTRIES[0],
            Registry {
                landmarks: &[
                    ("one", RarityV3::Rare),
                    ("two", RarityV3::Special),
                    ("three", RarityV3::Exclusive),
                    ("low", RarityV3::Uncommon),
                ],
                experience: REGISTRIES[0].experience,
                roll: REGISTRIES[0].roll,
                roll_domain: REGISTRIES[0].roll_domain,
            },
        ]
    }

    #[test]
    fn every_qualified_feature_survives_in_id_order_without_a_display_cap() {
        let at = point();
        let request = LandmarkMapRequest {
            registry: 2,
            landmarks: vec![
                landmark("two", at),
                landmark("one", at),
                landmark("three", at),
                landmark("low", at),
                landmark("unlisted", at),
                landmark("one", at),
            ],
        };
        let view = map_items_in(&rated(), request).expect("valid");
        let ids: Vec<_> = view.iter().map(|landmark| landmark.id.as_str()).collect();
        assert_eq!(ids, ["one", "three", "two"]);
        let rarities: Vec<_> = view.iter().map(|landmark| landmark.rarity).collect();
        assert_eq!(rarities, ["rare", "exclusive", "special"]);
    }

    #[test]
    fn conflicting_coordinates_and_invalid_ids_fail_closed() {
        let at = point();
        let elsewhere = GeoCoordinate::new(304_462_000, 504_650_000).expect("valid");
        let conflicts = LandmarkMapRequest {
            registry: 2,
            landmarks: vec![landmark("one", at), landmark("one", elsewhere)],
        };
        assert!(map_items_in(&rated(), conflicts).is_none());
        let invalid = LandmarkMapRequest {
            registry: 2,
            landmarks: vec![landmark("bad:id", at)],
        };
        assert!(map_items_in(&rated(), invalid).is_none());
        assert!(
            map_items_in(
                &rated(),
                LandmarkMapRequest {
                    registry: 99,
                    landmarks: vec![landmark("one", at)],
                }
            )
            .is_none()
        );
    }

    #[test]
    fn production_registry_one_remains_empty_and_legacy_unchanged() {
        let request = r#"{"registry":1,"landmarks":[{"id":"one","at":{"longitude_e7":"304461000","latitude_e7":"504650000"}}]}"#;
        let view: serde_json::Value =
            serde_json::from_str(&landmark_map_view_wire(request)).expect("JSON");
        assert_eq!(view["ok"], true);
        assert_eq!(view["landmarks"], json!([]));
        assert_eq!(
            landmark_map_view_wire(r#"{"registry":2,"landmarks":[]}"#),
            r#"{"error":"invalid","ok":false}"#
        );
        assert_eq!(
            landmark_map_view_wire(
                r#"{"registry":1,"landmarks":[{"id":"one","at":{"longitude_e7":"304461000","latitude_e7":"504650000"},"rarity":"exclusive"}]}"#
            ),
            r#"{"error":"invalid","ok":false}"#
        );
    }
}
