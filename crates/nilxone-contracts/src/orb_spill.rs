// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Orbs: what spills out of a fog cell when it opens.
//!
//! Since v3, every cell that opens for a Bond spills its own orbs: five around
//! the cell's centre and five around each landmark in it, `5 + 5 × landmarks`
//! with no cap. The identity service answers a cell's spill (an opaque key,
//! the centre and the landmarks) and keeps nothing; Core lays the orbs from
//! that answer alone, so the service can recompute every orb id it is asked
//! to pay. A cell spill lies until it is picked up, and only on that Bond's
//! device. Between the Bond and its Avaia, the nearer one reaches an orb.
//!
//! Since v4, a spill names a landmark rarity registry version and each
//! landmark's archive id, and every cell orb has a [`RarityV3`]. Core reads a
//! landmark's rarity from its own versioned registry, never from the host.
//! The cell's own five are Common; each landmark's five roll at least rarity
//! `r` with chance `1 / multiplier(r)`, never rarer than the landmark. An orb
//! pays 10, 20, 40, 80 or 160. Both are orb rules alone, apart from find
//! rates and find XP, and both are frozen with the registry version. A rated
//! orb's id names its key, registry, landmark and number, so the same id
//! always rolls and prices the same.
//!
//! The per-find spill below (v1, v2) stays readable for older hosts:
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
use crate::landmark_rarity::{REGISTRIES, Registry, rarity_in, registry_in};
use crate::rarity_v3::RarityV3;
use crate::spoken_line::{CENTIMETERS_PER_DEGREE, cosine_millionths};
use crate::{DecimalU64, GEO_COORDINATE_E7_SCALE, GeoCoordinate, distance_meters};

/// Version of the orb rules below; any change to a number raises it.
pub const ORB_SPILL_VERSION: u32 = 4;
/// What one orb pays whoever picks it up.
pub const ORB_EXPERIENCE: u32 = 10;
/// How long a spill lies before it is gone.
pub const ORB_LIFETIME_MS: u64 = 30 * 60_000;
/// A find's orb is picked up from strictly closer than this; a cell's orb
/// from this close or closer.
pub const ORB_PICKUP_METERS: u32 = 15;
/// What a cell spills for itself, around its centre.
pub const CELL_ORBS: u32 = 5;
/// What a cell spills for each landmark in it, around that landmark.
pub const LANDMARK_ORBS: u32 = 5;
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
const CELL_SCATTER_DOMAIN: &str = "nilx-one.orbs.cell.v1:";
const RATED_SCATTER_DOMAIN: &str = "nilx-one.orbs.rated.v1:";
/// Stands for the cell itself where a rated orb id names a landmark.
const OWN_ORBS: &str = "~";
/// A cell's own orbs lie at most this far, in centimetres, from its centre
/// on each axis.
const CELL_SCATTER_CM: i64 = 2_500;
/// A landmark's orbs lie at most this far, in centimetres, from it on each
/// axis.
const LANDMARK_SCATTER_CM: i64 = 800;
/// A cell key: the service's opaque digest, lowercase hex.
const CELL_KEY_HEX: usize = 64;
const CLUMP_MIN: u8 = 3;
const CLUMP_MAX: u8 = 6;
/// An orb strays at most this far, in centimetres, from its clump's middle.
const SCATTER_CM: i64 = 300;
/// The last clump stops short of the find, in percent of the trail.
const TRAIL_REACH_PERCENT: i64 = 88;
/// Share of the trail spent approaching a landmark waypoint, when present.
const LANDMARK_WAYPOINT_PERCENT: i64 = 75;
const E7: i128 = GEO_COORDINATE_E7_SCALE as i128;
const COSINE_SCALE: i128 = 1_000_000;

/// Stable failure classification for orbs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbError {
    /// Not a canonical artifact id.
    ArtifactId,
    /// Not a cell key: 64 lowercase hex digits.
    CellKey,
    /// Not a published landmark registry version, landmark id or orb number.
    Landmark,
}

impl core::fmt::Display for OrbError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::ArtifactId => "not a canonical artifact id",
            Self::CellKey => "not a cell key",
            Self::Landmark => "not a rated cell orb",
        })
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
    orb_trail_via(artifact_id, from, None, to)
}

/// Draw a find's orbs through an optional, real landmark waypoint before
/// ending at the same find. Without a waypoint this is exactly the v1 trail,
/// preserving every old artifact and orb id. The goal remains the find.
///
/// # Errors
///
/// Returns [`OrbError::ArtifactId`] for a noncanonical artifact id.
pub fn orb_trail_via(
    artifact_id: &str,
    from: GeoCoordinate,
    via: Option<GeoCoordinate>,
    to: GeoCoordinate,
) -> Result<Vec<OrbSpot>, OrbError> {
    let count = orb_count(artifact_id)?;
    let start = trail_start(from, via.unwrap_or(to));
    let sizes = clump_sizes(artifact_id, count);
    let clumps = i64::try_from(sizes.len()).unwrap_or(1);
    let mut spots = Vec::with_capacity(usize::from(count));
    let mut index: u8 = 0;
    for (clump, size) in (0_u8..).zip(sizes) {
        let center = if let Some(waypoint) = via {
            // Reserve roughly three quarters of the clumps for the first
            // leg, including one landing ON the waypoint; the remainder
            // advances 88% of the second leg toward the original find.
            // A one-clump trail stops at the landmark, with its actual find
            // still drawn separately as the goal.
            let approach_clumps =
                ((clumps * LANDMARK_WAYPOINT_PERCENT + 99) / 100).min((clumps - 1).max(1));
            let step = i64::from(clump) + 1;
            if step <= approach_clumps {
                between(start, waypoint, step, approach_clumps)
            } else {
                between(
                    waypoint,
                    to,
                    TRAIL_REACH_PERCENT * (step - approach_clumps),
                    100 * (clumps - approach_clumps),
                )
            }
        } else {
            between(
                start,
                to,
                TRAIL_REACH_PERCENT * (i64::from(clump) + 1),
                100 * clumps,
            )
        };
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

/// How many orbs a cell spills: five for itself and five for each landmark in
/// it. No cap.
#[must_use]
pub fn cell_orb_count(landmarks: usize) -> u32 {
    let landmarks = u32::try_from(landmarks).unwrap_or(u32::MAX);
    CELL_ORBS.saturating_add(LANDMARK_ORBS.saturating_mul(landmarks))
}

/// The id of one of a cell's orbs (v3): `orb:cell:`, the cell key, and which
/// orb.
///
/// # Errors
///
/// Returns [`OrbError::CellKey`] for anything that is not a cell key.
pub fn cell_orb_id(key: &str, index: u32) -> Result<String, OrbError> {
    if !is_cell_key(key) {
        return Err(OrbError::CellKey);
    }
    Ok(format!("orb:cell:{key}:{index}"))
}

fn is_cell_key(key: &str) -> bool {
    key.len() == CELL_KEY_HEX
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A scatter in `[-limit, limit]` centimetres for one axis of a cell's orb.
fn cell_scatter(key: &str, index: u32, axis: char, limit: i64) -> i64 {
    let span = u32::try_from(2 * limit + 1).unwrap_or(1);
    i64::from(fnv1a32(CELL_SCATTER_DOMAIN, &format!("{key}:{index}:{axis}")) % span) - limit
}

/// The same, for a rated orb, seeded by its own place in the spill.
fn rated_scatter(seed: &str, axis: char, limit: i64) -> i64 {
    let span = u32::try_from(2 * limit + 1).unwrap_or(1);
    i64::from(fnv1a32(RATED_SCATTER_DOMAIN, &format!("{seed}:{axis}")) % span) - limit
}

/// One orb a cell spilled (v3): which, around which landmark (none for the
/// cell's own five), and where.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellOrbSpot {
    pub index: u32,
    pub landmark: Option<u32>,
    pub at: GeoCoordinate,
}

/// Where a cell's orbs lie (v3): the first [`CELL_ORBS`] around its centre,
/// then [`LANDMARK_ORBS`] around each landmark, in the order the landmarks
/// are given. The key alone seeds the scatter, so the same answer always lays
/// the same orbs. Every v3 orb is Common.
///
/// # Errors
///
/// Returns [`OrbError::CellKey`] for anything that is not a cell key.
pub fn cell_orbs(
    key: &str,
    center: GeoCoordinate,
    landmarks: &[GeoCoordinate],
) -> Result<Vec<CellOrbSpot>, OrbError> {
    if !is_cell_key(key) {
        return Err(OrbError::CellKey);
    }
    let around = core::iter::once((None, center, CELL_SCATTER_CM, CELL_ORBS)).chain(
        (0_u32..)
            .zip(landmarks)
            .map(|(landmark, at)| (Some(landmark), *at, LANDMARK_SCATTER_CM, LANDMARK_ORBS)),
    );
    let mut spots = Vec::new();
    let mut index: u32 = 0;
    for (landmark, middle, limit, count) in around {
        for _ in 0..count {
            spots.push(CellOrbSpot {
                index,
                landmark,
                at: offset(
                    middle,
                    cell_scatter(key, index, 'n', limit),
                    cell_scatter(key, index, 'e', limit),
                ),
            });
            index = index.saturating_add(1);
        }
    }
    Ok(spots)
}

/// A landmark in a cell, as the service read it from the published archive:
/// its stable archive feature id, and where it stands. Its rarity is never
/// given: Core reads it from the landmark rarity registry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellLandmark {
    pub id: String,
    pub at: GeoCoordinate,
}

/// The id of a rated cell orb (v4):
/// `orb:cell:<key>:<registry>:<landmark id, or ~ for the cell's own>:<n>`,
/// `n` from 0 to 4. It names everything its rarity depends on, so the same id
/// always has the same rarity and pays the same.
///
/// # Errors
///
/// Returns [`OrbError::CellKey`] for a bad key, and [`OrbError::Landmark`]
/// for a landmark id, registry version or `n` that cannot be.
pub fn rated_orb_id(
    key: &str,
    registry: u32,
    landmark: Option<&str>,
    n: u32,
) -> Result<String, OrbError> {
    rated_orb_id_in(&REGISTRIES, key, registry, landmark, n)
}

fn rated_orb_id_in(
    registries: &[Registry],
    key: &str,
    registry: u32,
    landmark: Option<&str>,
    n: u32,
) -> Result<String, OrbError> {
    if !is_cell_key(key) {
        return Err(OrbError::CellKey);
    }
    let valid = match landmark {
        Some(id) => rarity_in(registries, registry, id).is_some(),
        None => registry_in(registries, registry).is_some(),
    };
    if !valid || n >= LANDMARK_ORBS {
        return Err(OrbError::Landmark);
    }
    Ok(format!(
        "orb:cell:{key}:{registry}:{}:{n}",
        landmark.unwrap_or(OWN_ORBS)
    ))
}

/// What a rated cell orb is: its rarity and what it pays, read from its id
/// alone. The identity service prices a pick-up with this, after checking
/// that the key is its own for that Bond and cell and that the landmark lies
/// in that cell.
///
/// # Errors
///
/// Returns [`OrbError::CellKey`] or [`OrbError::Landmark`] for an id that is
/// not a rated cell orb.
pub fn rated_orb_award(orb_id: &str) -> Result<(RarityV3, u32), OrbError> {
    rated_orb_award_in(&REGISTRIES, orb_id)
}

fn rated_orb_award_in(registries: &[Registry], orb_id: &str) -> Result<(RarityV3, u32), OrbError> {
    let rest = orb_id.strip_prefix("orb:cell:").ok_or(OrbError::CellKey)?;
    let mut parts = rest.split(':');
    let (Some(key), Some(registry), Some(landmark), Some(n), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return Err(OrbError::Landmark);
    };
    let registry: u32 = canonical_u32(registry).ok_or(OrbError::Landmark)?;
    let n: u32 = canonical_u32(n).ok_or(OrbError::Landmark)?;
    let landmark = (landmark != OWN_ORBS).then_some(landmark);
    // Re-derive the id: only a canonical one is an orb.
    if rated_orb_id_in(registries, key, registry, landmark, n)? != orb_id {
        return Err(OrbError::Landmark);
    }
    let rules = registry_in(registries, registry).ok_or(OrbError::Landmark)?;
    let rarity = rated_orb_rarity_in(registries, key, registry, landmark, n);
    let paid = rules.experience[usize::from(rarity.tier() - 1)];
    Ok((rarity, paid))
}

fn canonical_u32(text: &str) -> Option<u32> {
    let value: u32 = text.parse().ok()?;
    (value.to_string() == text).then_some(value)
}

/// The rarity of rated orb `n` around `landmark` (Common for the cell's own).
/// Under that registry's own frozen roll and hash domain.
fn rated_orb_rarity_in(
    registries: &[Registry],
    key: &str,
    registry: u32,
    landmark: Option<&str>,
    n: u32,
) -> RarityV3 {
    let (Some(id), Some(rules)) = (landmark, registry_in(registries, registry)) else {
        return RarityV3::Common;
    };
    let ceiling = rarity_in(registries, registry, id).unwrap_or(RarityV3::Common);
    (rules.roll)(
        fnv1a32(rules.roll_domain, &format!("{key}:{registry}:{id}:{n}")),
        ceiling,
    )
}

/// One rated orb a cell spilled (v4): its id, the landmark it lies around
/// (none for the cell's own five), where, and how rare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatedOrbSpot {
    pub id: String,
    pub landmark: Option<String>,
    pub at: GeoCoordinate,
    pub rarity: RarityV3,
}

/// Where a cell's rated orbs lie (v4): five around its centre, Common, then
/// five around each landmark, rolled under that landmark's rarity in
/// `registry`. Landmarks are taken in id order and each id once, so neither
/// the order they are given in nor a repeat changes any orb; each orb's place
/// and rarity depend only on the key, the registry, its landmark and `n`.
///
/// # Errors
///
/// Returns [`OrbError::CellKey`] for a bad key and [`OrbError::Landmark`] for
/// an unpublished registry version or a landmark id that cannot be.
pub fn cell_orbs_rated(
    key: &str,
    center: GeoCoordinate,
    registry: u32,
    landmarks: &[CellLandmark],
) -> Result<Vec<RatedOrbSpot>, OrbError> {
    if !is_cell_key(key) {
        return Err(OrbError::CellKey);
    }
    let mut sorted: Vec<&CellLandmark> = landmarks.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    // A repeat must agree on where its landmark stands; otherwise which one
    // survives would move an orb under the same id.
    if sorted
        .windows(2)
        .any(|pair| pair[0].id == pair[1].id && pair[0].at != pair[1].at)
    {
        return Err(OrbError::Landmark);
    }
    sorted.dedup_by(|a, b| a.id == b.id);
    let around = core::iter::once((None, center, CELL_SCATTER_CM)).chain(
        sorted
            .into_iter()
            .map(|landmark| (Some(landmark.id.as_str()), landmark.at, LANDMARK_SCATTER_CM)),
    );
    let mut spots = Vec::new();
    for (landmark, middle, limit) in around {
        for n in 0..LANDMARK_ORBS {
            let id = rated_orb_id(key, registry, landmark, n)?;
            let seed = format!("{key}:{}:{n}", landmark.unwrap_or(OWN_ORBS));
            spots.push(RatedOrbSpot {
                rarity: rated_orb_rarity_in(&REGISTRIES, key, registry, landmark, n),
                landmark: landmark.map(str::to_owned),
                at: offset(
                    middle,
                    rated_scatter(&seed, 'n', limit),
                    rated_scatter(&seed, 'e', limit),
                ),
                id,
            });
        }
    }
    Ok(spots)
}

/// One cell's spill as the service answered it: its key, its centre, its
/// landmarks, and when this device first drew it. It lies until picked up.
///
/// v3 gives bare `landmarks` and no `registry`; every orb is Common. v4 gives
/// the `registry` version and `rated_landmarks` with their archive ids, and
/// Core reads each landmark's rarity from that registry. A spill mixing the
/// two lies nowhere.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellSpill {
    pub key: String,
    pub center: GeoCoordinate,
    #[serde(default)]
    pub landmarks: Vec<GeoCoordinate>,
    /// The landmark rarity registry version the service answered with (v4).
    #[serde(default)]
    pub registry: Option<u32>,
    /// The cell's landmarks with their archive ids (v4).
    #[serde(default)]
    pub rated_landmarks: Vec<CellLandmark>,
    pub appeared_at: DecimalU64,
}

/// One spill as the host knows it: the find, where its trail runs, when this
/// device first drew it, when it is gone, and which orbs someone took.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownSpill {
    pub artifact_id: String,
    pub from: GeoCoordinate,
    /// Optional archived landmark waypoint. It never replaces the real find.
    #[serde(default)]
    pub via: Option<GeoCoordinate>,
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
    /// The cells that spilled for this Bond (v3).
    #[serde(default)]
    pub cells: Vec<CellSpill>,
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
    /// A rated cell orb's rarity code (v4); find orbs and v3 cell orbs carry
    /// none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rarity: Option<&'static str>,
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

/// One orb on the ground, for reach: its id, where, and whether it is a
/// cell's (reached inclusively, by the nearer of the two) or a find's.
struct Lying {
    id: String,
    at: GeoCoordinate,
    cell: bool,
}

/// The orbs lying at `now_ms`, what each of the Bond and the Avaia reaches,
/// nearest first. A spill that is gone, an orb someone took or this device
/// picked up, and a spill whose count is not its find's lie nowhere. A cell's
/// orb within reach of both goes to the nearer, the Bond on a tie.
#[must_use]
pub fn orb_world(world: &OrbWorld, now_ms: u64) -> OrbView {
    let mut orbs = Vec::new();
    let mut lying: Vec<Lying> = Vec::new();
    let mut next_expiry: Option<u64> = None;
    for spill in &world.spills {
        let expires = spill.expires_at.get();
        if now_ms >= expires || orb_count(&spill.artifact_id) != Ok(spill.count) {
            continue;
        }
        let Ok(trail) = orb_trail_via(&spill.artifact_id, spill.from, spill.via, spill.to) else {
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
                rarity: None,
            });
            lying.push(Lying {
                id,
                at: spot.at,
                cell: false,
            });
        }
        if any {
            orbs.push(DrawnOrb {
                id: format!("goal:{}", spill.artifact_id),
                kind: OrbKind::Goal,
                at: spill.to,
                lands_at: DecimalU64::new(
                    appeared.saturating_add((u64::from(spill.count) + 2) * ORB_STAGGER_MS),
                ),
                rarity: None,
            });
        }
    }
    lay_cells(world, &mut orbs, &mut lying);
    let (bond_reach, avaia_reach) = reach(world, &lying);
    OrbView {
        bond_reach,
        avaia_reach,
        orbs,
        next_expiry: next_expiry.map(DecimalU64::new),
    }
}

/// Lays the cells' orbs this device has not picked up yet: rated (v4) when
/// the spill names a registry, else v3. A spill mixing the two lies nowhere.
fn lay_cells(world: &OrbWorld, orbs: &mut Vec<DrawnOrb>, lying: &mut Vec<Lying>) {
    for cell in &world.cells {
        let laid: Vec<(String, GeoCoordinate, Option<&'static str>)> = match (
            cell.registry,
            cell.landmarks.is_empty(),
            cell.rated_landmarks.is_empty(),
        ) {
            (Some(registry), true, _) => {
                match cell_orbs_rated(&cell.key, cell.center, registry, &cell.rated_landmarks) {
                    Ok(spots) => spots
                        .into_iter()
                        .map(|spot| (spot.id, spot.at, Some(spot.rarity.code())))
                        .collect(),
                    Err(_) => continue,
                }
            }
            (None, _, true) => match cell_orbs(&cell.key, cell.center, &cell.landmarks) {
                Ok(spots) => spots
                    .into_iter()
                    .map(|spot| {
                        (
                            format!("orb:cell:{}:{}", cell.key, spot.index),
                            spot.at,
                            None,
                        )
                    })
                    .collect(),
                Err(_) => continue,
            },
            _ => continue,
        };
        let appeared = cell.appeared_at.get();
        for (index, (id, at, rarity)) in (0_u64..).zip(laid) {
            if world.picked.contains(&id) {
                continue;
            }
            orbs.push(DrawnOrb {
                id: id.clone(),
                kind: OrbKind::Orb,
                at,
                lands_at: DecimalU64::new(
                    appeared.saturating_add((index + 1).saturating_mul(ORB_STAGGER_MS)),
                ),
                rarity,
            });
            lying.push(Lying { id, at, cell: true });
        }
    }
}

/// What the Bond and the Avaia reach, nearest first. A cell's orb within
/// reach of both goes to the nearer, the Bond on a tie.
fn reach(world: &OrbWorld, lying: &[Lying]) -> (Vec<String>, Vec<String>) {
    let within = |point: Option<GeoCoordinate>, orb: &Lying| -> Option<u32> {
        let meters = distance_meters(point?, orb.at);
        let reached = if orb.cell {
            meters <= ORB_PICKUP_METERS
        } else {
            meters < ORB_PICKUP_METERS
        };
        reached.then_some(meters)
    };
    let mut bond_near: Vec<(u32, &String)> = Vec::new();
    let mut avaia_near: Vec<(u32, &String)> = Vec::new();
    for orb in lying {
        let bond = within(world.bond, orb);
        let avaia = within(world.avaia, orb);
        match (bond, avaia) {
            (Some(b), Some(a)) if orb.cell => {
                if b <= a {
                    bond_near.push((b, &orb.id));
                } else {
                    avaia_near.push((a, &orb.id));
                }
            }
            _ => {
                if let Some(b) = bond {
                    bond_near.push((b, &orb.id));
                }
                if let Some(a) = avaia {
                    avaia_near.push((a, &orb.id));
                }
            }
        }
    }
    let nearest_first = |mut near: Vec<(u32, &String)>| -> Vec<String> {
        near.sort();
        near.into_iter().map(|(_, id)| id.clone()).collect()
    };
    (nearest_first(bond_near), nearest_first(avaia_near))
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
    fn a_landmark_waypoint_bends_the_trail_without_moving_the_find_goal() {
        let waypoint = point(304_470_000, 504_665_000);
        let direct = orb_trail(KYIV, cell_middle(), find()).expect("direct");
        let guided = orb_trail_via(KYIV, cell_middle(), Some(waypoint), find()).expect("guided");
        assert_eq!(guided.len(), direct.len());
        assert_ne!(guided, direct);
        assert_eq!(
            orb_trail_via(KYIV, cell_middle(), None, find()).expect("legacy"),
            direct
        );
        assert!(
            guided
                .iter()
                .any(|orb| distance_meters(orb.at, waypoint) < 18),
            "a landmark must have a visible trail clump nearby"
        );
        let world = OrbWorld {
            spills: vec![KnownSpill {
                artifact_id: KYIV.to_owned(),
                from: cell_middle(),
                via: Some(waypoint),
                to: find(),
                appeared_at: DecimalU64::new(1_000),
                expires_at: DecimalU64::new(1_801_000),
                count: orb_count(KYIV).expect("count"),
                taken: Vec::new(),
            }],
            cells: Vec::new(),
            picked: Vec::new(),
            bond: None,
            avaia: None,
        };
        let view = orb_world(&world, 2_000);
        let goal = view
            .orbs
            .iter()
            .find(|orb| orb.kind == OrbKind::Goal)
            .expect("real find goal");
        assert_eq!(goal.at, find());
    }

    #[test]
    fn v2_waypoints_preserve_legacy_wire_ids_and_the_original_goal() {
        let legacy = serde_json::json!({
            "spills": [{
                "artifact_id": KYIV,
                "from": { "longitude_e7": "304461000", "latitude_e7": "504650000" },
                "to": { "longitude_e7": "304469000", "latitude_e7": "504655000" },
                "appeared_at": "1000",
                "expires_at": "1801000",
                "count": 20,
                "taken": []
            }],
            "picked": [],
            "bond": null,
            "avaia": null
        });
        let direct: serde_json::Value =
            serde_json::from_str(&orb_world_wire(&legacy.to_string(), 2_000)).expect("legacy JSON");
        let mut explicit_null = legacy.clone();
        explicit_null["spills"][0]["via"] = serde_json::Value::Null;
        let null_wire: serde_json::Value =
            serde_json::from_str(&orb_world_wire(&explicit_null.to_string(), 2_000))
                .expect("null waypoint JSON");
        assert_eq!(
            direct, null_wire,
            "omitting via retains exact legacy layout"
        );

        let mut guided = legacy;
        guided["spills"][0]["via"] = serde_json::json!({
            "longitude_e7": "304470000",
            "latitude_e7": "504665000"
        });
        let via_wire: serde_json::Value =
            serde_json::from_str(&orb_world_wire(&guided.to_string(), 2_000)).expect("guided JSON");
        let direct_orbs = direct["view"]["orbs"].as_array().expect("direct orbs");
        let guided_orbs = via_wire["view"]["orbs"].as_array().expect("guided orbs");
        assert_eq!(direct_orbs.len(), guided_orbs.len());
        assert!(
            direct_orbs
                .iter()
                .zip(guided_orbs)
                .any(|(a, b)| a["at"] != b["at"])
        );
        for (a, b) in direct_orbs.iter().zip(guided_orbs) {
            assert_eq!(a["id"], b["id"]);
            assert_eq!(a["kind"], b["kind"]);
            assert_eq!(a["lands_at"], b["lands_at"]);
            if a["kind"] == "goal" {
                assert_eq!(a["at"], b["at"]);
            }
        }
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
                via: None,
                to: find(),
                appeared_at: DecimalU64::new(1_000),
                expires_at: DecimalU64::new(1_000 + ORB_LIFETIME_MS),
                count: orb_count(KYIV).expect("count"),
                taken: vec![1],
            }],
            cells: Vec::new(),
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

    const KEY: &str = "5f1c0a7e9b2d4c6e8a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f";

    fn landmark_a() -> GeoCoordinate {
        point(304_470_000, 504_665_000)
    }

    fn landmark_b() -> GeoCoordinate {
        point(304_455_000, 504_645_000)
    }

    fn cell(landmarks: Vec<GeoCoordinate>) -> CellSpill {
        CellSpill {
            key: KEY.to_owned(),
            center: cell_middle(),
            landmarks,
            registry: None,
            rated_landmarks: Vec::new(),
            appeared_at: DecimalU64::new(1_000),
        }
    }

    fn cell_world(
        landmarks: Vec<GeoCoordinate>,
        bond: Option<GeoCoordinate>,
        avaia: Option<GeoCoordinate>,
    ) -> OrbWorld {
        OrbWorld {
            spills: Vec::new(),
            cells: vec![cell(landmarks)],
            picked: Vec::new(),
            bond,
            avaia,
        }
    }

    #[test]
    fn a_cell_spills_five_and_five_more_for_each_landmark_without_a_cap() {
        assert_eq!(cell_orb_count(0), 5);
        assert_eq!(cell_orb_count(2), 15);
        assert_eq!(cell_orb_count(100), 505);
        let many: Vec<GeoCoordinate> = (0..60).map(|_| landmark_a()).collect();
        let spots = cell_orbs(KEY, cell_middle(), &many).expect("orbs");
        assert_eq!(spots.len(), 305);
        assert!(
            spots
                .iter()
                .zip(0_u32..)
                .all(|(spot, index)| spot.index == index)
        );
    }

    #[test]
    fn a_cells_orbs_lie_around_its_centre_and_each_landmark() {
        let both = [landmark_a(), landmark_b()];
        let spots = cell_orbs(KEY, cell_middle(), &both).expect("orbs");
        assert_eq!(spots.len(), 15);
        for spot in &spots[..5] {
            assert_eq!(spot.landmark, None);
            assert!(distance_meters(spot.at, cell_middle()) <= 36);
        }
        for (spot, landmark) in spots[5..].iter().zip([0, 0, 0, 0, 0, 1, 1, 1, 1, 1]) {
            assert_eq!(spot.landmark, Some(landmark));
            let at = if landmark == 0 {
                landmark_a()
            } else {
                landmark_b()
            };
            assert!(distance_meters(spot.at, at) <= 12);
        }
        assert_eq!(cell_orbs(KEY, cell_middle(), &both), Ok(spots));
        let other = KEY.replace('5', "6");
        assert_ne!(
            cell_orbs(&other, cell_middle(), &[]).expect("orbs")[0].at,
            cell_orbs(KEY, cell_middle(), &[]).expect("orbs")[0].at
        );
    }

    #[test]
    fn a_cell_key_is_sixty_four_lowercase_hex_digits() {
        assert_eq!(cell_orb_id(KEY, 7), Ok(format!("orb:cell:{KEY}:7")));
        for bad in [
            "",
            "abc",
            &KEY.to_uppercase(),
            &format!("{KEY}0"),
            &KEY.replace('a', "g"),
        ] {
            assert_eq!(cell_orb_id(bad, 0), Err(OrbError::CellKey), "{bad}");
            assert_eq!(cell_orbs(bad, cell_middle(), &[]), Err(OrbError::CellKey));
        }
        let mut world = cell_world(Vec::new(), None, None);
        world.cells[0].key = "nope".to_owned();
        assert!(orb_world(&world, 1_000).orbs.is_empty());
    }

    #[test]
    fn a_cell_spill_lies_until_picked_and_lands_in_order() {
        let mut world = cell_world(vec![landmark_a()], None, None);
        world.picked.push(cell_orb_id(KEY, 3).expect("id"));
        let view = orb_world(&world, u64::MAX);
        assert_eq!(view.orbs.len(), 9);
        assert!(view.orbs.iter().all(|orb| orb.kind == OrbKind::Orb));
        assert!(!view.orbs.iter().any(|orb| orb.id.ends_with(":3")));
        assert!(
            view.orbs
                .windows(2)
                .all(|pair| pair[0].lands_at < pair[1].lands_at)
        );
        assert_eq!(view.orbs[0].lands_at, DecimalU64::new(1_070));
        assert_eq!(view.next_expiry, None);
    }

    #[test]
    fn a_cells_orb_is_reached_at_fifteen_metres_by_the_nearer_of_the_two() {
        let spots = cell_orbs(KEY, cell_middle(), &[]).expect("orbs");
        let at = spots[0].at;
        let id = cell_orb_id(KEY, 0).expect("id");
        // Both stand on it: the Bond takes the tie.
        let view = orb_world(&cell_world(Vec::new(), Some(at), Some(at)), 1_000);
        assert!(view.bond_reach.contains(&id));
        assert!(!view.avaia_reach.contains(&id));
        // The Avaia stands on it, the Bond a few metres off: the Avaia's.
        let near = offset(at, 500, 0);
        let view = orb_world(&cell_world(Vec::new(), Some(near), Some(at)), 1_000);
        assert!(view.avaia_reach.contains(&id));
        assert!(!view.bond_reach.contains(&id));
        // Exactly fifteen metres still reaches; sixteen does not.
        let reached = (1_400..1_700)
            .map(|cm| offset(at, cm, 0))
            .find(|point| distance_meters(*point, at) == ORB_PICKUP_METERS)
            .expect("a point fifteen metres off");
        let view = orb_world(&cell_world(Vec::new(), Some(reached), None), 1_000);
        assert!(view.bond_reach.contains(&id));
        let far = offset(at, 0, 5_000);
        assert!(
            !orb_world(&cell_world(Vec::new(), None, Some(far)), 1_000)
                .avaia_reach
                .contains(&id)
        );
    }

    #[test]
    fn the_v3_wire_reads_cells_beside_the_find_spills() {
        let input = format!(
            r#"{{"spills":[],"cells":[{{"key":"{KEY}","center":{{"longitude_e7":"304461000","latitude_e7":"504650000"}},"landmarks":[{{"longitude_e7":"304470000","latitude_e7":"504665000"}}],"appeared_at":"1000"}}],"picked":[],"bond":null,"avaia":null}}"#
        );
        let answer: serde_json::Value =
            serde_json::from_str(&orb_world_wire(&input, 2_000)).expect("JSON");
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["view"]["orbs"].as_array().expect("orbs").len(), 10);
        assert_eq!(answer["view"]["orbs"][0]["id"], format!("orb:cell:{KEY}:0"));
        assert_eq!(
            orb_world_wire(
                &input.replace(
                    r#""appeared_at":"1000""#,
                    r#""appeared_at":"1000","count":10"#
                ),
                2_000
            ),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    fn key_of(seed: u32) -> String {
        format!(
            "{:064x}",
            u128::from(seed).wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c834)
        )
    }

    fn sight(id: &str, at: GeoCoordinate) -> CellLandmark {
        CellLandmark {
            id: id.to_owned(),
            at,
        }
    }

    #[test]
    fn an_orb_pays_ten_times_its_multiplier_apart_from_find_xp() {
        let orb: Vec<u32> = RarityV3::ALL
            .into_iter()
            .map(|rarity| crate::orb_experience(1, rarity).expect("registry 1"))
            .collect();
        assert_eq!(orb, [10, 20, 40, 80, 160]);
        let find: Vec<u32> = RarityV3::ALL
            .into_iter()
            .map(RarityV3::base_find_xp)
            .collect();
        assert_ne!(orb, find);
    }

    #[test]
    fn an_orb_roll_is_at_least_r_with_chance_one_in_its_multiplier() {
        let draws: Vec<u32> = (0..=u16::MAX)
            .map(|high| (u32::from(high) << 16) | 0x1234)
            .collect();
        let total = u64::try_from(draws.len()).expect("count");
        for rarity in RarityV3::ALL {
            let at_least = draws
                .iter()
                .filter(|draw| crate::roll_halving(**draw, RarityV3::Exclusive) >= rarity)
                .count();
            assert_eq!(
                u64::try_from(at_least).expect("count"),
                total / rarity.price_multiplier()
            );
        }
        assert_eq!(crate::roll_halving(0, RarityV3::Rare), RarityV3::Rare);
        assert_eq!(crate::roll_halving(0, RarityV3::Common), RarityV3::Common);
        assert_eq!(
            crate::roll_halving(u32::MAX, RarityV3::Exclusive),
            RarityV3::Common
        );
    }

    #[test]
    fn a_rated_spill_is_five_common_and_five_per_landmark_in_id_order() {
        let given = [sight("n2", landmark_b()), sight("n1", landmark_a())];
        let spots = cell_orbs_rated(KEY, cell_middle(), 1, &given).expect("orbs");
        assert_eq!(spots.len(), 15);
        assert!(spots[..5].iter().all(|spot| spot.landmark.is_none()
            && spot.rarity == RarityV3::Common
            && distance_meters(spot.at, cell_middle()) <= 36));
        assert!(
            spots[5..10]
                .iter()
                .all(|spot| spot.landmark.as_deref() == Some("n1")
                    && distance_meters(spot.at, landmark_a()) <= 12)
        );
        assert!(
            spots[10..]
                .iter()
                .all(|spot| spot.landmark.as_deref() == Some("n2")
                    && distance_meters(spot.at, landmark_b()) <= 12)
        );
        assert_eq!(spots[0].id, format!("orb:cell:{KEY}:1:~:0"));
        assert_eq!(spots[5].id, format!("orb:cell:{KEY}:1:n1:0"));
        // Order and repeats change nothing.
        let reordered = [
            sight("n1", landmark_a()),
            sight("n2", landmark_b()),
            sight("n1", landmark_a()),
        ];
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 1, &reordered),
            Ok(spots.clone())
        );
        // An orb does not move or change when another landmark comes or goes.
        let alone =
            cell_orbs_rated(KEY, cell_middle(), 1, &[sight("n2", landmark_b())]).expect("orbs");
        assert_eq!(alone[5..], spots[10..]);
    }

    #[test]
    fn a_rated_spill_refuses_an_unpublished_registry_or_a_bad_landmark_id() {
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 0, &[]),
            Err(OrbError::Landmark)
        );
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 2, &[]),
            Err(OrbError::Landmark)
        );
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 1, &[sight("a:b", landmark_a())]),
            Err(OrbError::Landmark)
        );
        assert_eq!(
            cell_orbs_rated("nope", cell_middle(), 1, &[]),
            Err(OrbError::CellKey)
        );
    }

    #[test]
    fn a_rated_orb_prices_from_its_id_alone_and_only_a_canonical_id() {
        let spots =
            cell_orbs_rated(KEY, cell_middle(), 1, &[sight("n1", landmark_a())]).expect("orbs");
        for spot in &spots {
            assert_eq!(
                rated_orb_award(&spot.id),
                Ok((
                    spot.rarity,
                    crate::orb_experience(1, spot.rarity).expect("registry 1")
                ))
            );
        }
        // Golden: registry 1 names nothing yet, so every orb is Common.
        assert_eq!(
            rated_orb_award(&format!("orb:cell:{KEY}:1:n1:4")),
            Ok((RarityV3::Common, 10))
        );
        for bad in [
            format!("orb:cell:{KEY}:1:n1:5"),
            format!("orb:cell:{KEY}:01:n1:0"),
            format!("orb:cell:{KEY}:2:n1:0"),
            format!("orb:cell:{KEY}:1:n1:0:0"),
            format!("orb:cell:{KEY}:1:a.b:+1"),
            format!("orb:cell:{KEY}:0"),
            "orb:art:seg:1".to_owned(),
        ] {
            assert!(rated_orb_award(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_later_registry_never_rerolls_or_reprices_an_earlier_orb_id() {
        // A later version rates n1 Exclusive, pays half again as much and
        // rolls everything Common. None of it reaches an id laid under 1.
        fn always_common(_: u32, _: RarityV3) -> RarityV3 {
            RarityV3::Common
        }
        let registries = [
            REGISTRIES[0],
            Registry {
                landmarks: &[("n1", RarityV3::Exclusive)],
                experience: [15, 30, 60, 120, 240],
                roll: always_common,
                roll_domain: "nilx-one.orbs.rarity.test:",
            },
        ];
        for seed in 0..64 {
            let key = key_of(seed);
            for n in 0..LANDMARK_ORBS {
                let old = format!("orb:cell:{key}:1:n1:{n}");
                assert_eq!(
                    rated_orb_award_in(&registries, &old),
                    rated_orb_award(&old),
                    "{old}"
                );
                let new = format!("orb:cell:{key}:2:n1:{n}");
                assert_eq!(
                    rated_orb_award_in(&registries, &new),
                    Ok((RarityV3::Common, 15))
                );
            }
        }
        // Golden: registry 1 names nothing, so this orb is Common for 10.
        assert_eq!(
            rated_orb_award(&format!("orb:cell:{KEY}:1:n1:0")),
            Ok((RarityV3::Common, 10))
        );
    }

    #[test]
    fn a_repeated_landmark_must_agree_on_where_it_stands() {
        let conflicting = [sight("n1", landmark_a()), sight("n1", landmark_b())];
        let reversed = [sight("n1", landmark_b()), sight("n1", landmark_a())];
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 1, &conflicting),
            Err(OrbError::Landmark)
        );
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 1, &reversed),
            Err(OrbError::Landmark)
        );
        let agreeing = [sight("n1", landmark_a()), sight("n1", landmark_a())];
        assert_eq!(
            cell_orbs_rated(KEY, cell_middle(), 1, &agreeing),
            cell_orbs_rated(KEY, cell_middle(), 1, &[sight("n1", landmark_a())])
        );
    }

    #[test]
    fn about_one_in_sixteen_orbs_under_an_exclusive_ceiling_is_exclusive() {
        let mut counts = [0_u64; 5];
        let mut total = 0_u64;
        for seed in 0..4_000 {
            let key = key_of(seed);
            for n in 0..LANDMARK_ORBS {
                let draw = fnv1a32(REGISTRIES[0].roll_domain, &format!("{key}:1:n1:{n}"));
                let rarity = crate::roll_halving(draw, RarityV3::Exclusive);
                counts[usize::from(rarity.tier() - 1)] += 1;
                total += 1;
            }
        }
        // 20 000 rolls: at least rarity r is expected total / multiplier(r).
        for rarity in RarityV3::ALL {
            let at_least: u64 = counts[usize::from(rarity.tier() - 1)..].iter().sum();
            let expected = total / rarity.price_multiplier();
            assert!(
                at_least.abs_diff(expected) * 100 <= expected * 15,
                "{rarity:?}: {at_least} of {total}, expected about {expected}"
            );
        }
    }

    #[test]
    fn the_v4_wire_takes_ids_never_rarities_and_reports_each_orbs_rarity() {
        let rated = format!(
            r#"{{"spills":[],"cells":[{{"key":"{KEY}","center":{{"longitude_e7":"304461000","latitude_e7":"504650000"}},"registry":1,"rated_landmarks":[{{"id":"n1","at":{{"longitude_e7":"304470000","latitude_e7":"504665000"}}}}],"appeared_at":"1000"}}]}}"#
        );
        let answer: serde_json::Value =
            serde_json::from_str(&orb_world_wire(&rated, 2_000)).expect("JSON");
        let orbs = answer["view"]["orbs"].as_array().expect("orbs");
        assert_eq!(orbs.len(), 10);
        assert_eq!(orbs[0]["id"], format!("orb:cell:{KEY}:1:~:0"));
        assert!(orbs.iter().all(|orb| orb["rarity"] == "common"));
        // A host cannot hand in a rarity.
        let tampered = rated.replace(r#""id":"n1","#, r#""id":"n1","rarity":"exclusive","#);
        assert_eq!(
            orb_world_wire(&tampered, 2_000),
            r#"{"error":"invalid","ok":false}"#
        );
        // Mixing v3 landmarks into a rated spill lays nothing.
        let mixed = rated.replace(
            r#""registry":1,"#,
            r#""registry":1,"landmarks":[{"longitude_e7":"304470000","latitude_e7":"504665000"}],"#,
        );
        let mixed: serde_json::Value =
            serde_json::from_str(&orb_world_wire(&mixed, 2_000)).expect("JSON");
        assert_eq!(mixed["view"]["orbs"].as_array().expect("orbs").len(), 0);
        // v3 cells and finds carry no rarity: their output is unchanged.
        let v3 = format!(
            r#"{{"spills":[],"cells":[{{"key":"{KEY}","center":{{"longitude_e7":"304461000","latitude_e7":"504650000"}},"landmarks":[{{"longitude_e7":"304470000","latitude_e7":"504665000"}}],"appeared_at":"1000"}}]}}"#
        );
        let v3 = orb_world_wire(&v3, 2_000);
        assert!(v3.contains(&format!(r#""id":"orb:cell:{KEY}:0""#)), "{v3}");
        assert!(!v3.contains("rarity"), "{v3}");
    }
}
