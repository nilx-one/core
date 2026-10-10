// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Orbs: what spills out of a fog cell when it opens, and leads to a find the
//! cell holds.
//!
//! Each find in an opened cell spills a short trail of orbs, in small clumps,
//! from the point the host names (the middle of the cell) to where the find
//! lies. How many is a function of the find alone, five to thirty, so the
//! service that claims orbs counts exactly what every client draws. A spill
//! lies for thirty minutes. Whoever comes within fifteen metres of an orb
//! first, an Avaia or a Bond, picks it up for ten experience; who was first
//! is the claiming service's to say, and everyone after gets "crap!".
//!
//! Core keeps no state of its own and decides no claim. The host hands in the
//! spills it knows, what it already picked up, and where its Bond and Avaia
//! are; Core answers what lies where, when each orb lands, and what is within
//! reach. Integer arithmetic only, so every runtime lays the same trail. An
//! orb is presentation and a pick-up intent: never a `BondChain` interaction,
//! a Relationship, or proof of presence.

use serde::{Deserialize, Serialize};

use crate::find_item::{artifact_segment, fnv1a32};
use crate::spoken_line::{CENTIMETERS_PER_DEGREE, cosine_millionths};
use crate::{DecimalU64, GEO_COORDINATE_E7_SCALE, GeoCoordinate, distance_meters};

/// Version of the orb rules below; any change to a number raises it.
pub const ORB_SPILL_VERSION: u32 = 1;
/// What one orb pays whoever picks it up.
pub const ORB_EXPERIENCE: u32 = 10;
/// How long a spill lies before it is gone.
pub const ORB_LIFETIME_MS: u64 = 30 * 60_000;
/// An orb is picked up from strictly closer than this.
pub const ORB_PICKUP_METERS: u32 = 15;
/// The fewest orbs one find spills.
pub const ORB_MIN: u8 = 5;
/// The most orbs one find spills.
pub const ORB_MAX: u8 = 30;
/// How far apart in time the orbs of one trail land: a trickle, not a dump.
pub const ORB_STAGGER_MS: u64 = 70;
/// A trail shorter than this is stretched back from the find.
pub const MIN_TRAIL_METERS: u32 = 30;

const COUNT_DOMAIN: &str = "nilx-one.orbs.count.v1:";
const CLUMP_DOMAIN: &str = "nilx-one.orbs.clump.v1:";
const SCATTER_DOMAIN: &str = "nilx-one.orbs.scatter.v1:";
const CLUMP_MIN: u8 = 3;
const CLUMP_MAX: u8 = 6;
/// An orb strays at most this far, in centimetres, from its clump's middle.
const SCATTER_CM: i64 = 300;
/// The last clump stops short of the find, in percent of the trail.
const TRAIL_REACH_PERCENT: i64 = 88;
const E7: i128 = GEO_COORDINATE_E7_SCALE as i128;
const COSINE_SCALE: i128 = 1_000_000;

/// Stable failure classification for orbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbError {
    /// Not a canonical artifact id.
    ArtifactId,
}

impl core::fmt::Display for OrbError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("not a canonical artifact id")
    }
}

impl std::error::Error for OrbError {}

/// How many orbs a find spills, from five to thirty.
///
/// # Errors
///
/// Returns [`OrbError::ArtifactId`] for anything that is not a canonical
/// artifact id.
pub fn orb_count(artifact_id: &str) -> Result<u8, OrbError> {
    artifact_segment(artifact_id).ok_or(OrbError::ArtifactId)?;
    let span = u32::from(ORB_MAX - ORB_MIN) + 1;
    let extra = fnv1a32(COUNT_DOMAIN, artifact_id) % span;
    // `extra` is below 26, so the sum stays within ORB_MAX.
    Ok(ORB_MIN + u8::try_from(extra).unwrap_or(0))
}

/// The id of one orb: `orb:`, its find, and which of its orbs.
#[must_use]
pub fn orb_id(artifact_id: &str, index: u8) -> String {
    format!("orb:{artifact_id}:{index}")
}

/// One orb on the ground: which it is, the clump it fell in, and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrbSpot {
    pub index: u8,
    pub clump: u8,
    pub at: GeoCoordinate,
}

/// A displacement in centimetres north and east, as E7 degrees at a latitude.
fn offset(at: GeoCoordinate, north_cm: i64, east_cm: i64) -> GeoCoordinate {
    let cosine = cosine_millionths(i64::from(at.latitude_e7()).abs()).max(1);
    let north_e7 = i128::from(north_cm) * E7 / CENTIMETERS_PER_DEGREE;
    let east_e7 = i128::from(east_cm) * E7 * COSINE_SCALE / (CENTIMETERS_PER_DEGREE * cosine);
    let latitude = i128::from(at.latitude_e7()) + north_e7;
    let longitude = i128::from(at.longitude_e7()) + east_e7;
    i32::try_from(longitude)
        .ok()
        .zip(i32::try_from(latitude).ok())
        .and_then(|(longitude, latitude)| GeoCoordinate::new(longitude, latitude).ok())
        .unwrap_or(at)
}

/// The point `numerator / denominator` of the way from `from` to `to`.
fn between(
    from: GeoCoordinate,
    to: GeoCoordinate,
    numerator: i64,
    denominator: i64,
) -> GeoCoordinate {
    let step = |a: i32, b: i32| {
        let value = i64::from(a) + (i64::from(b) - i64::from(a)) * numerator / denominator.max(1);
        i32::try_from(value).unwrap_or(a)
    };
    GeoCoordinate::new(
        step(from.longitude_e7(), to.longitude_e7()),
        step(from.latitude_e7(), to.latitude_e7()),
    )
    .unwrap_or(to)
}

/// The distance between two nearby points in whole centimetres: the same
/// equirectangular integer approximation as [`distance_meters`], finer.
fn distance_cm(from: GeoCoordinate, to: GeoCoordinate) -> i128 {
    let north = i128::from(to.latitude_e7()) - i128::from(from.latitude_e7());
    let east = i128::from(to.longitude_e7()) - i128::from(from.longitude_e7());
    let middle = (i64::from(to.latitude_e7()) + i64::from(from.latitude_e7())).abs() / 2;
    let north_cm = north * CENTIMETERS_PER_DEGREE / E7;
    let east_cm = east * CENTIMETERS_PER_DEGREE * cosine_millionths(middle) / (E7 * COSINE_SCALE);
    i128::try_from(
        (north_cm * north_cm + east_cm * east_cm)
            .unsigned_abs()
            .isqrt(),
    )
    .unwrap_or(i128::MAX)
}

/// Where a trail to `to` starts: `from`, unless that is too close to make a
/// trail of, in which case the same way back, [`MIN_TRAIL_METERS`] from the
/// find; straight at the find, or nowhere at all, it comes in from the
/// south-west.
#[must_use]
pub fn trail_start(from: GeoCoordinate, to: GeoCoordinate) -> GeoCoordinate {
    let min_cm = i128::from(MIN_TRAIL_METERS) * 100;
    let cm = distance_cm(from, to);
    if cm >= min_cm {
        return from;
    }
    if cm == 0 {
        // 30 m at 45°: about 21.21 m south and 21.21 m west.
        let leg = i64::from(MIN_TRAIL_METERS) * 7_071 / 100;
        return offset(to, -leg, -leg);
    }
    let cm = i64::try_from(cm).unwrap_or(1);
    between(to, from, i64::from(MIN_TRAIL_METERS) * 100, cm)
}

/// The clump sizes of a find's trail: three to six each, never a remainder
/// too small to be a clump of its own.
fn clump_sizes(artifact_id: &str, count: u8) -> Vec<u8> {
    let mut sizes = Vec::new();
    let mut left = count;
    let span = u32::from(CLUMP_MAX - CLUMP_MIN) + 1;
    while left > 0 {
        let draw = fnv1a32(CLUMP_DOMAIN, &format!("{artifact_id}:{}", sizes.len())) % span;
        let mut size = CLUMP_MIN + u8::try_from(draw).unwrap_or(0);
        if left < size + CLUMP_MIN {
            size = if left <= CLUMP_MAX {
                left
            } else {
                left - CLUMP_MIN
            };
        }
        sizes.push(size);
        left -= size;
    }
    sizes
}

/// A scatter in `[-SCATTER_CM, SCATTER_CM]` for one axis of one orb.
fn scatter(artifact_id: &str, index: u8, axis: char) -> i64 {
    let draw = fnv1a32(SCATTER_DOMAIN, &format!("{artifact_id}:{index}:{axis}"));
    i64::from(draw % 601) - SCATTER_CM
}

/// Where a find's orbs lie: clumps spaced evenly from `from` towards `to`,
/// each orb a few metres from its clump's middle, numbered from the start of
/// the trail so a trail that lands in order lands towards the find.
///
/// # Errors
///
/// Returns [`OrbError::ArtifactId`] for anything that is not a canonical
/// artifact id.
pub fn orb_trail(
    artifact_id: &str,
    from: GeoCoordinate,
    to: GeoCoordinate,
) -> Result<Vec<OrbSpot>, OrbError> {
    let count = orb_count(artifact_id)?;
    let start = trail_start(from, to);
    let sizes = clump_sizes(artifact_id, count);
    let clumps = i64::try_from(sizes.len()).unwrap_or(1);
    let mut spots = Vec::with_capacity(usize::from(count));
    let mut index: u8 = 0;
    for (clump, size) in (0_u8..).zip(sizes) {
        let center = between(
            start,
            to,
            TRAIL_REACH_PERCENT * (i64::from(clump) + 1),
            100 * clumps,
        );
        for _ in 0..size {
            spots.push(OrbSpot {
                index,
                clump,
                at: offset(
                    center,
                    scatter(artifact_id, index, 'n'),
                    scatter(artifact_id, index, 'e'),
                ),
            });
            index += 1;
        }
    }
    Ok(spots)
}

/// One spill as the host knows it: the find, where its trail runs, when this
/// device first drew it, when it is gone, and which orbs someone took.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownSpill {
    pub artifact_id: String,
    pub from: GeoCoordinate,
    pub to: GeoCoordinate,
    pub appeared_at: DecimalU64,
    pub expires_at: DecimalU64,
    pub count: u8,
    #[serde(default)]
    pub taken: Vec<u8>,
}

/// Everything the host knows about the orbs around it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbWorld {
    pub spills: Vec<KnownSpill>,
    /// Orb ids this device already picked up, whatever the service said yet.
    #[serde(default)]
    pub picked: Vec<String>,
    /// The Bond's own observation, only while it may pick up.
    #[serde(default)]
    pub bond: Option<GeoCoordinate>,
    /// The Avaia's body, only while it walks the world.
    #[serde(default)]
    pub avaia: Option<GeoCoordinate>,
}

/// What a drawn orb is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrbKind {
    Orb,
    /// The find a trail leads to.
    Goal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DrawnOrb {
    pub id: String,
    pub kind: OrbKind,
    pub at: GeoCoordinate,
    pub lands_at: DecimalU64,
}

/// Core's answer: what to draw, and what the Bond and the Avaia reach now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrbView {
    pub orbs: Vec<DrawnOrb>,
    pub bond_reach: Vec<String>,
    pub avaia_reach: Vec<String>,
    /// When the soonest live spill is gone, if any lies.
    pub next_expiry: Option<DecimalU64>,
}

/// The orbs lying at `now_ms`, what each of the Bond and the Avaia reaches,
/// nearest first. A spill that is gone, an orb someone took or this device
/// picked up, and a spill whose count is not its find's lie nowhere.
#[must_use]
pub fn orb_world(world: &OrbWorld, now_ms: u64) -> OrbView {
    let mut orbs = Vec::new();
    let mut lying: Vec<(String, GeoCoordinate)> = Vec::new();
    let mut next_expiry: Option<u64> = None;
    for spill in &world.spills {
        let expires = spill.expires_at.get();
        if now_ms >= expires || orb_count(&spill.artifact_id) != Ok(spill.count) {
            continue;
        }
        let Ok(trail) = orb_trail(&spill.artifact_id, spill.from, spill.to) else {
            continue;
        };
        next_expiry = Some(next_expiry.map_or(expires, |soonest| soonest.min(expires)));
        let appeared = spill.appeared_at.get();
        let mut any = false;
        for spot in trail {
            let id = orb_id(&spill.artifact_id, spot.index);
            if spill.taken.contains(&spot.index) || world.picked.contains(&id) {
                continue;
            }
            any = true;
            orbs.push(DrawnOrb {
                id: id.clone(),
                kind: OrbKind::Orb,
                at: spot.at,
                lands_at: DecimalU64::new(
                    appeared.saturating_add((u64::from(spot.index) + 1) * ORB_STAGGER_MS),
                ),
            });
            lying.push((id, spot.at));
        }
        if any {
            orbs.push(DrawnOrb {
                id: format!("goal:{}", spill.artifact_id),
                kind: OrbKind::Goal,
                at: spill.to,
                lands_at: DecimalU64::new(
                    appeared.saturating_add((u64::from(spill.count) + 2) * ORB_STAGGER_MS),
                ),
            });
        }
    }
    let reach = |point: Option<GeoCoordinate>| -> Vec<String> {
        let Some(point) = point else {
            return Vec::new();
        };
        let mut near: Vec<(u32, &String)> = lying
            .iter()
            .map(|(id, at)| (distance_meters(point, *at), id))
            .filter(|(meters, _)| *meters < ORB_PICKUP_METERS)
            .collect();
        near.sort();
        near.into_iter().map(|(_, id)| id.clone()).collect()
    };
    OrbView {
        bond_reach: reach(world.bond),
        avaia_reach: reach(world.avaia),
        orbs,
        next_expiry: next_expiry.map(DecimalU64::new),
    }
}

/// [`orb_world`] as JSON for Wasm and `UniFFI`: `{"ok":true,"view":…}`, or
/// `{"ok":false,"error":"invalid"}` for input that does not parse.
#[must_use]
pub fn orb_world_wire(world: &str, now_ms: u64) -> String {
    match serde_json::from_str::<OrbWorld>(world) {
        Ok(world) => {
            serde_json::json!({ "ok": true, "view": orb_world(&world, now_ms) }).to_string()
        }
        Err(_) => serde_json::json!({ "ok": false, "error": "invalid" }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KYIV: &str = "art:seg:312346:298243:e2908:1:0";

    fn point(longitude_e7: i32, latitude_e7: i32) -> GeoCoordinate {
        GeoCoordinate::new(longitude_e7, latitude_e7).expect("coordinate")
    }

    fn find() -> GeoCoordinate {
        point(304_469_000, 504_655_000)
    }

    /// About 80 m south-west of the find.
    fn cell_middle() -> GeoCoordinate {
        point(304_461_000, 504_650_000)
    }

    #[test]
    fn a_find_spills_five_to_thirty_the_same_every_time() {
        let mut seen = std::collections::BTreeSet::new();
        for row in 0..600 {
            let id = format!("art:seg:{}:298243:e2908:1:0", 312_000 + row);
            let count = orb_count(&id).expect("count");
            assert_eq!(orb_count(&id), Ok(count));
            assert!((ORB_MIN..=ORB_MAX).contains(&count));
            seen.insert(count);
        }
        assert_eq!(seen.first(), Some(&ORB_MIN));
        assert_eq!(seen.last(), Some(&ORB_MAX));
        assert_eq!(orb_count("art:nope"), Err(OrbError::ArtifactId));
        assert_eq!(orb_count("art:seg:01:2:e1:1:0"), Err(OrbError::ArtifactId));
    }

    // Golden counts: the identity service claims against these.
    #[test]
    fn matches_the_golden_counts() {
        let counts: Vec<u8> = (0..5)
            .map(|row| {
                orb_count(&format!("art:seg:{}:298243:e2908:1:0", 312_346 + row)).expect("count")
            })
            .collect();
        assert_eq!(counts, GOLDEN_COUNTS);
    }

    const GOLDEN_COUNTS: [u8; 5] = [20, 23, 14, 7, 27];

    #[test]
    fn a_trail_falls_in_clumps_of_three_to_six_towards_the_find() {
        for row in 0..200 {
            let id = format!("art:seg:{}:298243:e2908:1:0", 312_000 + row);
            let trail = orb_trail(&id, cell_middle(), find()).expect("trail");
            assert_eq!(trail.len(), usize::from(orb_count(&id).expect("count")));
            assert!(
                trail
                    .iter()
                    .enumerate()
                    .all(|(i, spot)| usize::from(spot.index) == i)
            );
            let clumps = trail.last().expect("an orb").clump + 1;
            for clump in 0..clumps {
                let size = trail.iter().filter(|spot| spot.clump == clump).count();
                assert!((3..=6).contains(&size), "{id}: clump of {size}");
            }
            // Each clump's middle is nearer the find than the one before.
            let mean = |clump: u8| {
                let near: Vec<u32> = trail
                    .iter()
                    .filter(|spot| spot.clump == clump)
                    .map(|spot| distance_meters(spot.at, find()))
                    .collect();
                near.iter().sum::<u32>() / u32::try_from(near.len()).expect("a clump")
            };
            for clump in 1..clumps {
                assert!(mean(clump) < mean(clump - 1), "{id}: clump {clump}");
            }
        }
        assert_eq!(
            orb_trail(KYIV, cell_middle(), find()),
            orb_trail(KYIV, cell_middle(), find())
        );
    }

    #[test]
    fn a_short_trail_is_stretched_back_from_the_find() {
        let start = trail_start(find(), find());
        assert!((29..=31).contains(&distance_meters(start, find())));
        let close = point(304_469_500, 504_655_300);
        let stretched = trail_start(close, find());
        assert!((29..=31).contains(&distance_meters(stretched, find())));
        assert_eq!(trail_start(cell_middle(), find()), cell_middle());
    }

    fn world(bond: Option<GeoCoordinate>, avaia: Option<GeoCoordinate>) -> OrbWorld {
        OrbWorld {
            spills: vec![KnownSpill {
                artifact_id: KYIV.to_owned(),
                from: cell_middle(),
                to: find(),
                appeared_at: DecimalU64::new(1_000),
                expires_at: DecimalU64::new(1_000 + ORB_LIFETIME_MS),
                count: orb_count(KYIV).expect("count"),
                taken: vec![1],
            }],
            picked: vec![orb_id(KYIV, 2)],
            bond,
            avaia,
        }
    }

    #[test]
    fn the_world_draws_what_lies_and_lands_it_in_order() {
        let count = orb_count(KYIV).expect("count");
        let view = orb_world(&world(None, None), 1_000);
        let orbs: Vec<&DrawnOrb> = view
            .orbs
            .iter()
            .filter(|o| o.kind == OrbKind::Orb)
            .collect();
        assert_eq!(orbs.len(), usize::from(count) - 2);
        assert!(
            !orbs
                .iter()
                .any(|o| o.id == orb_id(KYIV, 1) || o.id == orb_id(KYIV, 2))
        );
        assert!(
            orbs.windows(2)
                .all(|pair| pair[0].lands_at < pair[1].lands_at)
        );
        let goal = view
            .orbs
            .iter()
            .find(|o| o.kind == OrbKind::Goal)
            .expect("goal");
        assert_eq!(goal.at, find());
        assert_eq!(
            view.next_expiry,
            Some(DecimalU64::new(1_000 + ORB_LIFETIME_MS))
        );

        let gone = orb_world(&world(None, None), 1_000 + ORB_LIFETIME_MS);
        assert!(gone.orbs.is_empty());
        assert_eq!(gone.next_expiry, None);
    }

    #[test]
    fn whoever_is_within_fifteen_metres_reaches_the_orbs_nearest_first() {
        let trail = orb_trail(KYIV, cell_middle(), find()).expect("trail");
        let at = trail[0].at;
        let view = orb_world(&world(None, Some(at)), 1_000);
        assert_eq!(view.avaia_reach.first(), Some(&orb_id(KYIV, 0)));
        assert!(view.bond_reach.is_empty());
        assert!(!view.avaia_reach.contains(&orb_id(KYIV, 1)));
        let far = point(304_369_000, 504_655_000);
        assert!(
            orb_world(&world(Some(far), None), 1_000)
                .bond_reach
                .is_empty()
        );
    }

    #[test]
    fn a_spill_whose_count_is_not_its_finds_lies_nowhere() {
        let mut world = world(None, None);
        world.spills[0].count = world.spills[0].count.wrapping_add(1);
        assert!(orb_world(&world, 1_000).orbs.is_empty());
    }

    #[test]
    fn the_wire_reads_decimal_strings_and_refuses_numbers() {
        let input = format!(
            r#"{{"spills":[{{"artifact_id":"{KYIV}","from":{{"longitude_e7":"304459000","latitude_e7":"504649000"}},"to":{{"longitude_e7":"304469000","latitude_e7":"504655000"}},"appeared_at":"1000","expires_at":"1801000","count":{},"taken":[]}}],"avaia":{{"longitude_e7":"304469000","latitude_e7":"504655000"}}}}"#,
            orb_count(KYIV).expect("count")
        );
        let answer = orb_world_wire(&input, 1_000);
        assert!(answer.starts_with(r#"{"ok":true"#), "{answer}");
        assert!(answer.contains(r#""lands_at":"1070""#), "{answer}");
        assert_eq!(
            orb_world_wire(r#"{"spills":[{"artifact_id":"x","appeared_at":1}]}"#, 0),
            r#"{"error":"invalid","ok":false}"#
        );
    }
}
