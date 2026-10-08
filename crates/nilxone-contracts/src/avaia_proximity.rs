// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Distance-bound capability of one Avaia relative to its Bond.
//! Distance is supplied by the host in whole metres; Core alone decides
//! whether a reveal is possible and how long it takes. This is not a
//! BondChain interaction or proof of a shared physical presence.

use serde::Serialize;

/// 100% of walking energy at one percent per 50 metres.
pub const RED_DISTANCE_METERS: u32 = 5_000;
/// New work is possible only strictly within 90% of the red boundary.
pub const REVEAL_DISTANCE_METERS: u32 = RED_DISTANCE_METERS * 9 / 10;
pub const NEAR_DISTANCE_METERS: u32 = 15;
const MIN_REVEAL_MS: u64 = 60_000;
const DISTANT_REVEAL_MS: u64 = 5 * 60_000;
const MAX_REVEAL_MS: u64 = 10 * 60_000;
const PER_ARTIFACT_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AvaiaProximity {
    pub distance_m: u32,
    pub red_m: u32,
    pub restore_below_m: u32,
    pub level: &'static str,
    pub can_reveal: bool,
    /// Absent means there is no valid new reveal duration.
    pub duration_ms: Option<u64>,
}

/// Deterministic tier and duration. The far-away baseline is 5 minutes,
/// growing toward 10 as artifacts are added; within 15m it is one minute.
/// Interpolation has no floating-point or host-specific rounding.
#[must_use]
pub fn avaia_proximity(distance_m: u32, artifacts: u32) -> AvaiaProximity {
    let can_reveal = distance_m < REVEAL_DISTANCE_METERS;
    let level = if distance_m <= NEAR_DISTANCE_METERS {
        "near"
    } else if distance_m >= RED_DISTANCE_METERS {
        "red"
    } else if !can_reveal {
        "restricted"
    } else {
        "working"
    };
    let duration_ms = can_reveal.then(|| {
        let delta = u64::from(distance_m.saturating_sub(NEAR_DISTANCE_METERS));
        let span = u64::from(REVEAL_DISTANCE_METERS - NEAR_DISTANCE_METERS);
        let far_extra = DISTANT_REVEAL_MS - MIN_REVEAL_MS;
        let artifact_extra = u64::from(artifacts.min(5)) * PER_ARTIFACT_MS;
        (MIN_REVEAL_MS + delta * (far_extra + artifact_extra) / span).min(MAX_REVEAL_MS)
    });
    AvaiaProximity {
        distance_m,
        red_m: RED_DISTANCE_METERS,
        restore_below_m: REVEAL_DISTANCE_METERS,
        level,
        can_reveal,
        duration_ms,
    }
}

/// Same JSON projection for Wasm and UniFFI; never a persisted state transition.
#[must_use]
pub fn avaia_proximity_wire(distance_m: u32, artifacts: u32) -> String {
    serde_json::to_string(&avaia_proximity(distance_m, artifacts))
        .expect("AvaiaProximity consists only of fixed finite scalar values")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn near_is_one_minute_even_with_artifacts() {
        for distance in [0, 14, 15] {
            let result = avaia_proximity(distance, 999);
            assert_eq!(result.level, "near");
            assert_eq!(result.duration_ms, Some(60_000));
        }
    }

    #[test]
    fn reveal_is_forbidden_until_below_ninety_percent() {
        assert!(avaia_proximity(4_499, 0).can_reveal);
        for distance in [4_500, 4_999, 5_000, u32::MAX] {
            let result = avaia_proximity(distance, 10);
            assert!(!result.can_reveal);
            assert_eq!(result.duration_ms, None);
        }
        assert_eq!(avaia_proximity(4_999, 0).level, "restricted");
        assert_eq!(avaia_proximity(5_000, 0).level, "red");
    }

    #[test]
    fn work_takes_longer_with_distance_and_artifacts_and_is_capped() {
        let close = avaia_proximity(16, 3).duration_ms.unwrap();
        let distant = avaia_proximity(4_499, 3).duration_ms.unwrap();
        assert!(close > 60_000);
        assert!(distant > close);
        assert!(distant < MAX_REVEAL_MS);
        assert!(avaia_proximity(4_499, 99).duration_ms.unwrap() <= MAX_REVEAL_MS);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&avaia_proximity_wire(15, 7))
                .unwrap()["duration_ms"],
            60_000
        );
    }
}
