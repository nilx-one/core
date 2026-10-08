// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Distance-bound capability of one Avaia relative to its Bond.
//! Distance is supplied by the host in whole metres; Core alone decides
//! whether a reveal is possible and how long it takes. This is not a
//! `BondChain` interaction or proof of a shared physical presence.
//!
//! Core keeps no state of its own: the host stores the previous answer's
//! `can_reveal` and hands it back as `previously_blocked`, so the transition
//! (block at red, restore below 90% of red) is a pure function of the
//! observation and that one bit.

use serde::Serialize;

/// 100% of walking energy at one percent per 50 metres.
pub const RED_DISTANCE_METERS: u32 = 5_000;
/// Blocked work is restored only strictly below 90% of the red boundary.
pub const REVEAL_DISTANCE_METERS: u32 = RED_DISTANCE_METERS * 9 / 10;
/// The visual `near` tier: strictly closer than this.
pub const NEAR_DISTANCE_METERS: u32 = 15;
/// The one-minute reveal applies up to and including this distance.
pub const MIN_DURATION_DISTANCE_METERS: u32 = 15;
const MIN_REVEAL_MS: u64 = 60_000;
const DISTANT_REVEAL_MS: u64 = 5 * 60_000;
const MAX_REVEAL_MS: u64 = 10 * 60_000;
const PER_ARTIFACT_MS: u64 = 60_000;
/// Artifacts above this count add nothing further.
pub const MAX_COUNTED_ARTIFACTS: u32 = 5;

/// Distance tier. `Restricted` is the band between restore and red: work
/// that was already allowed may continue, blocked work stays blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProximityLevel {
    Near,
    Working,
    Restricted,
    Red,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AvaiaProximity {
    pub distance_m: u32,
    pub red_m: u32,
    pub restore_below_m: u32,
    pub level: ProximityLevel,
    pub can_reveal: bool,
    /// Absent means there is no valid new reveal duration.
    pub duration_ms: Option<u64>,
}

/// Deterministic tier, transition and duration.
///
/// `previously_blocked` is the host's stored `!can_reveal` of the last answer;
/// pass `true` when there is none, so an unknown history never authorizes
/// work inside the hysteresis band. The far-away baseline is 5 minutes,
/// growing toward 10 as artifacts are added; within 15m it is one minute.
/// Interpolation has no floating-point or host-specific rounding.
#[must_use]
pub fn avaia_proximity(
    distance_m: u32,
    artifacts: u32,
    previously_blocked: bool,
) -> AvaiaProximity {
    let blocked = distance_m >= RED_DISTANCE_METERS
        || (previously_blocked && distance_m >= REVEAL_DISTANCE_METERS);
    let can_reveal = !blocked;
    let level = if distance_m >= RED_DISTANCE_METERS {
        ProximityLevel::Red
    } else if distance_m >= REVEAL_DISTANCE_METERS {
        ProximityLevel::Restricted
    } else if distance_m < NEAR_DISTANCE_METERS {
        ProximityLevel::Near
    } else {
        ProximityLevel::Working
    };
    let duration_ms = can_reveal.then(|| {
        let span = u64::from(REVEAL_DISTANCE_METERS - MIN_DURATION_DISTANCE_METERS);
        let delta = u64::from(distance_m.saturating_sub(MIN_DURATION_DISTANCE_METERS)).min(span);
        let far_extra = DISTANT_REVEAL_MS - MIN_REVEAL_MS;
        let artifact_extra = u64::from(artifacts.min(MAX_COUNTED_ARTIFACTS)) * PER_ARTIFACT_MS;
        // delta <= span, so this never exceeds MIN + far + 5 artifacts = MAX.
        MIN_REVEAL_MS + delta * (far_extra + artifact_extra) / span
    });
    debug_assert!(duration_ms.is_none_or(|ms| (MIN_REVEAL_MS..=MAX_REVEAL_MS).contains(&ms)));
    AvaiaProximity {
        distance_m,
        red_m: RED_DISTANCE_METERS,
        restore_below_m: REVEAL_DISTANCE_METERS,
        level,
        can_reveal,
        duration_ms,
    }
}

/// Same JSON projection for Wasm and `UniFFI`; never a persisted state transition.
#[must_use]
pub fn avaia_proximity_wire(distance_m: u32, artifacts: u32, previously_blocked: bool) -> String {
    // The projection is plain data and cannot fail to serialize; should it
    // ever, an empty answer authorizes nothing (hosts reject unparsable policy).
    serde_json::to_string(&avaia_proximity(distance_m, artifacts, previously_blocked))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn can(distance: u32, previously_blocked: bool) -> bool {
        avaia_proximity(distance, 0, previously_blocked).can_reveal
    }

    #[test]
    fn near_tier_is_strictly_below_fifteen_but_the_minute_covers_fifteen() {
        for distance in [0, 14] {
            assert_eq!(
                avaia_proximity(distance, 9, false).level,
                ProximityLevel::Near
            );
        }
        assert_eq!(avaia_proximity(15, 9, false).level, ProximityLevel::Working);
        assert_eq!(avaia_proximity(16, 0, false).level, ProximityLevel::Working);
        for distance in [0, 14, 15] {
            assert_eq!(
                avaia_proximity(distance, 999, false).duration_ms,
                Some(60_000)
            );
        }
        assert!(avaia_proximity(16, 3, false).duration_ms.unwrap() > 60_000);
    }

    #[test]
    fn outward_trip_is_open_until_red() {
        // Never blocked before: the whole restricted band stays open.
        for distance in [4_499, 4_500, 4_999] {
            assert!(can(distance, false), "{distance}");
        }
        for distance in [5_000, 5_001, u32::MAX] {
            assert!(!can(distance, false), "{distance}");
        }
        assert_eq!(
            avaia_proximity(4_999, 0, false).level,
            ProximityLevel::Restricted
        );
        assert_eq!(avaia_proximity(5_000, 0, false).level, ProximityLevel::Red);
    }

    #[test]
    fn return_trip_is_restored_only_below_ninety_percent() {
        for distance in [4_500, 4_999, 5_000, u32::MAX] {
            let result = avaia_proximity(distance, 10, true);
            assert!(!result.can_reveal, "{distance}");
            assert_eq!(result.duration_ms, None);
        }
        assert!(can(4_499, true));
        assert!(can(0, true));
    }

    #[test]
    fn a_full_round_trip_follows_the_stored_bit() {
        let mut blocked = true; // unknown history: fail closed
        let path = [0, 4_499, 4_500, 4_999, 5_000, 4_999, 4_500, 4_499, 0];
        let expect = [true, true, true, true, false, false, false, true, true];
        for (distance, expected) in path.into_iter().zip(expect) {
            let answer = avaia_proximity(distance, 0, blocked);
            assert_eq!(answer.can_reveal, expected, "{distance}");
            blocked = !answer.can_reveal;
        }
    }

    #[test]
    fn unknown_history_in_the_band_authorizes_nothing() {
        assert!(!can(4_700, true));
    }

    #[test]
    fn work_takes_longer_with_distance_and_artifacts_and_is_capped_by_construction() {
        let close = avaia_proximity(16, 3, false).duration_ms.unwrap();
        let distant = avaia_proximity(4_499, 3, false).duration_ms.unwrap();
        assert!(close > 60_000);
        assert!(distant > close);
        assert!(distant < 600_000);
        // Inside the band the distance term stops growing at its ceiling.
        for distance in [4_500, 4_800, 4_999] {
            assert_eq!(
                avaia_proximity(distance, 99, false).duration_ms,
                Some(600_000)
            );
            assert_eq!(
                avaia_proximity(distance, 0, false).duration_ms,
                Some(300_000)
            );
        }
        let max = avaia_proximity(4_499, 99, false).duration_ms.unwrap();
        assert!(max <= 600_000);
        assert_eq!(
            avaia_proximity(4_499, 99, false),
            avaia_proximity(4_499, 5, false)
        );
        assert_eq!(
            avaia_proximity(4_499, 0, false).duration_ms.unwrap() / 1_000,
            299
        );
    }

    #[test]
    fn wire_is_stable_json() {
        let value: serde_json::Value =
            serde_json::from_str(&avaia_proximity_wire(15, 7, false)).unwrap();
        assert_eq!(value["duration_ms"], 60_000);
        assert_eq!(value["level"], "working");
        assert_eq!(value["can_reveal"], true);
        assert_eq!(value["restore_below_m"], 4_500);
        let blocked: serde_json::Value =
            serde_json::from_str(&avaia_proximity_wire(5_000, 0, false)).unwrap();
        assert_eq!(blocked["level"], "red");
        assert!(blocked["duration_ms"].is_null());
    }
}
