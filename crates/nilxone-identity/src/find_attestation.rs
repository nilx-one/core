// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Whether a reported pick-up is one the service may attest
//! (docs/find-attestation.md). Pure rules: no storage, no clock of its own.
//!
//! A pick-up names its find by artifact id, says who picked it up, when and
//! where. The service rolls the id itself (`finds::roll_artifact`), so the
//! find is real and its tier is the roll's. The place has to lie within
//! [`PICKUP_RADIUS_METERS`] of the find's segment, and the time inside the
//! find's week, not in the future and not older than [`PICKUP_MAX_AGE_MS`].
//! Core then names the item and its prices.

use nilxone_api::finds::AttestRefusal;
use nilxone_contracts::{FindTier, item_for_find, item_kind};

use crate::finds::{self, FindRoll};

/// A find lies at most 25 m to either side of the walk that rolled it
/// (`FIND_OFFSET_METERS`), shows within 15 m (`PERCEPTION_METERS`), and a
/// phone's fix is allowed 10 m more.
pub const PICKUP_RADIUS_METERS: f64 = 25.0 + 15.0 + 10.0;

/// How long a device may hold a pick-up before reporting it: a day offline.
pub const PICKUP_MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

/// How far ahead of the service's clock a device's clock may run.
pub const CLOCK_SKEW_MS: i64 = 60 * 1000;

/// Metres in a degree of latitude, as the segment grid counts them.
const METERS_PER_DEGREE: f64 = 111_195.0;
/// `ROW_DEGREES` and `COLUMN_DEGREES` in `artifact-contract`.
const ROW_DEGREES: f64 = 50.0 / METERS_PER_DEGREE;
const COLUMN_DEGREES: f64 = ROW_DEGREES * 1.5698;

// Who picked the find up is part of the wire contract: one definition for the
// service and every client.
pub use nilxone_api::finds::Finder;

/// A pick-up as a device reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportedPickup {
    pub artifact_id: String,
    pub finder: Finder,
    pub found_at_ms: i64,
    /// Degrees × 10⁷, as `GeoCoordinate` stores them.
    pub longitude_e7: i32,
    pub latitude_e7: i32,
}

/// A pick-up the service may attest, priced by Core.
#[derive(Clone, Debug, PartialEq)]
pub struct VerifiedPickup {
    pub reported: ReportedPickup,
    pub artifact_sha: String,
    pub epoch: i64,
    pub tier: u8,
    pub item_id: &'static str,
    /// Money credited on the spot (small change), else 0.
    pub pickup_credit: u64,
}

/// Why a pick-up is not attested.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PickupRefusal {
    #[error("not a find this service rolls")]
    UnknownFind,
    #[error("the pick-up time is outside the find's week or too old")]
    OutOfTime,
    #[error("the pick-up place is not near the find")]
    TooFar,
    #[error("the Avaia does not pick up a find of this tier")]
    NotTheAvaias,
}

impl PickupRefusal {
    /// The wire contract's refusal for this: its code and HTTP status are
    /// defined there, once.
    #[must_use]
    pub const fn wire(self) -> AttestRefusal {
        match self {
            Self::UnknownFind => AttestRefusal::UnknownFind,
            Self::OutOfTime => AttestRefusal::OutOfTime,
            Self::TooFar => AttestRefusal::TooFar,
            Self::NotTheAvaias => AttestRefusal::NotTheAvaias,
        }
    }

    #[must_use]
    pub const fn code(self) -> &'static str {
        self.wire().code()
    }
}

/// Checks a reported pick-up against the find it names, at `now_ms` on the
/// service's clock.
///
/// # Errors
///
/// The first rule the pick-up breaks, as [`PickupRefusal`].
pub fn verify_pickup(
    reported: ReportedPickup,
    now_ms: i64,
) -> Result<VerifiedPickup, PickupRefusal> {
    let roll: FindRoll = finds::roll_artifact(finds::FIND_PACK_ID, &reported.artifact_id)
        .map_err(|_| PickupRefusal::UnknownFind)?;
    if reported.finder == Finder::Avaia && roll.tier > finds::AVAIA_PICKUP_MAX_TIER {
        return Err(PickupRefusal::NotTheAvaias);
    }
    let found_at = reported.found_at_ms;
    if found_at > now_ms.saturating_add(CLOCK_SKEW_MS)
        || found_at < now_ms.saturating_sub(PICKUP_MAX_AGE_MS)
        || finds::epoch_of(found_at) != roll.epoch
    {
        return Err(PickupRefusal::OutOfTime);
    }
    let (row, column) = segment_of(&reported.artifact_id).ok_or(PickupRefusal::UnknownFind)?;
    if distance_to_segment_meters(row, column, reported.longitude_e7, reported.latitude_e7)
        > PICKUP_RADIUS_METERS
    {
        return Err(PickupRefusal::TooFar);
    }
    let tier = FindTier::new(roll.tier).ok_or(PickupRefusal::UnknownFind)?;
    let item =
        item_for_find(&reported.artifact_id, tier).map_err(|_| PickupRefusal::UnknownFind)?;
    Ok(VerifiedPickup {
        artifact_sha: finds::artifact_sha(&reported.artifact_id),
        epoch: roll.epoch,
        tier: roll.tier,
        item_id: item.id,
        pickup_credit: item.seeds,
        reported,
    })
}

/// What selling the item `item_id` credits, by Core's price, or `None` for
/// money and for a thing nobody buys.
#[must_use]
pub fn sale_price(item_id: &str) -> Option<u64> {
    item_kind(item_id).and_then(|kind| kind.sell_price)
}

fn segment_of(artifact_id: &str) -> Option<(i64, i64)> {
    let mut parts = artifact_id.strip_prefix("art:seg:")?.split(':');
    let row = parts.next()?.parse().ok()?;
    let column = parts.next()?.parse().ok()?;
    Some((row, column))
}

/// The distance from a point to the nearest edge of a segment cell, `0`
/// inside it. Local flat-earth metres: plenty at tens of metres.
fn distance_to_segment_meters(row: i64, column: i64, longitude_e7: i32, latitude_e7: i32) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let (row, column) = (row as f64, column as f64);
    let south = row * ROW_DEGREES - 90.0;
    let west = column * COLUMN_DEGREES - 180.0;
    let latitude = f64::from(latitude_e7) / 1e7;
    let longitude = f64::from(longitude_e7) / 1e7;
    let near_latitude = latitude.clamp(south, south + ROW_DEGREES);
    let near_longitude = longitude.clamp(west, west + COLUMN_DEGREES);
    let north_meters = (latitude - near_latitude) * METERS_PER_DEGREE;
    let east_meters =
        (longitude - near_longitude) * METERS_PER_DEGREE * latitude.to_radians().cos();
    north_meters.hypot(east_meters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_codes_are_the_contracts_and_have_not_changed() {
        let codes: Vec<_> = [
            PickupRefusal::UnknownFind,
            PickupRefusal::OutOfTime,
            PickupRefusal::TooFar,
            PickupRefusal::NotTheAvaias,
        ]
        .iter()
        .map(|refusal| (refusal.code(), refusal.wire().http_status()))
        .collect();
        assert_eq!(
            codes,
            [
                ("unknown_find", 422),
                ("out_of_time", 422),
                ("too_far", 422),
                ("not_the_avaias", 422)
            ]
        );
        assert_eq!(Finder::parse("avaia"), Some(Finder::Avaia));
        assert_eq!(Finder::Bond.as_str(), "bond");
    }

    /// A find the pack rolls, and a moment in its week.
    fn rolled() -> (FindRoll, i64, i64) {
        let epoch = 2_961;
        for row in 311_900..312_400 {
            for column in 298_000..298_100 {
                if let Some(roll) =
                    finds::roll_segment(finds::FIND_PACK_ID, 1, epoch, row, column).unwrap()
                    && roll.tier <= 3
                {
                    return (roll, row, column);
                }
            }
        }
        unreachable!("Kyiv rolls a common find somewhere")
    }

    fn week_start(epoch: i64) -> i64 {
        epoch * finds::EPOCH_MS + 4 * 24 * 60 * 60 * 1000
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn centre(row: i64, column: i64) -> (i32, i32) {
        let latitude = (row as f64 + 0.5) * ROW_DEGREES - 90.0;
        let longitude = (column as f64 + 0.5) * COLUMN_DEGREES - 180.0;
        (
            (longitude * 1e7).round() as i32,
            (latitude * 1e7).round() as i32,
        )
    }

    fn pickup(roll: &FindRoll, at: i64, (longitude_e7, latitude_e7): (i32, i32)) -> ReportedPickup {
        ReportedPickup {
            artifact_id: roll.artifact_id.clone(),
            finder: Finder::Bond,
            found_at_ms: at,
            longitude_e7,
            latitude_e7,
        }
    }

    #[test]
    fn attests_a_real_find_where_and_when_it_lay() {
        let (roll, row, column) = rolled();
        let at = week_start(roll.epoch) + 3_600_000;
        let verified = verify_pickup(pickup(&roll, at, centre(row, column)), at + 1_000).unwrap();
        assert_eq!(verified.tier, roll.tier);
        assert_eq!(verified.epoch, roll.epoch);
        assert_eq!(
            verified.artifact_sha,
            finds::artifact_sha(&roll.artifact_id)
        );
        let tier = FindTier::new(roll.tier).unwrap();
        let item = item_for_find(&roll.artifact_id, tier).unwrap();
        assert_eq!(verified.item_id, item.id);
        assert_eq!(verified.pickup_credit, item.seeds);
    }

    #[test]
    fn refuses_a_made_up_find() {
        let (roll, row, column) = rolled();
        let at = week_start(roll.epoch);
        let mut reported = pickup(&roll, at, centre(row, column));
        reported.artifact_id = format!("art:seg:{row}:{column}:e{}:1:1", roll.epoch);
        assert_eq!(verify_pickup(reported, at), Err(PickupRefusal::UnknownFind));
    }

    #[test]
    fn refuses_a_place_far_from_the_find() {
        let (roll, row, column) = rolled();
        let at = week_start(roll.epoch);
        let (longitude, latitude) = centre(row, column);
        // About 45 m outside the cell's northern edge: inside the radius.
        let near = (
            longitude,
            latitude + 25 * 10_000_000 / 111_195 + 45 * 10_000_000 / 111_195,
        );
        assert!(verify_pickup(pickup(&roll, at, near), at).is_ok());
        // About 70 m out: too far.
        let far = (
            longitude,
            latitude + 25 * 10_000_000 / 111_195 + 70 * 10_000_000 / 111_195,
        );
        assert_eq!(
            verify_pickup(pickup(&roll, at, far), at),
            Err(PickupRefusal::TooFar)
        );
    }

    #[test]
    fn refuses_a_time_outside_the_week_the_future_or_a_stale_report() {
        let (roll, row, column) = rolled();
        let start = week_start(roll.epoch);
        let place = centre(row, column);
        assert_eq!(
            verify_pickup(pickup(&roll, start - 1, place), start),
            Err(PickupRefusal::OutOfTime)
        );
        assert_eq!(
            verify_pickup(pickup(&roll, start + 120_000, place), start),
            Err(PickupRefusal::OutOfTime)
        );
        assert_eq!(
            verify_pickup(pickup(&roll, start, place), start + PICKUP_MAX_AGE_MS + 1),
            Err(PickupRefusal::OutOfTime)
        );
    }

    #[test]
    fn the_avaia_picks_up_only_common_finds() {
        let epoch = 2_961;
        let rare = (311_900..313_000)
            .flat_map(|row| (298_000..298_200).map(move |column| (row, column)))
            .find_map(|(row, column)| {
                finds::roll_segment(finds::FIND_PACK_ID, 1, epoch, row, column)
                    .unwrap()
                    .filter(|roll| roll.tier >= 4)
                    .map(|roll| (roll, row, column))
            })
            .expect("a rare find in range");
        let (roll, row, column) = rare;
        let at = week_start(epoch);
        let mut reported = pickup(&roll, at, centre(row, column));
        reported.finder = Finder::Avaia;
        assert_eq!(
            verify_pickup(reported, at),
            Err(PickupRefusal::NotTheAvaias)
        );
    }

    #[test]
    fn prices_things_and_never_money_or_flyers() {
        assert_eq!(sale_price("bottle"), Some(5));
        assert_eq!(sale_price("cd_player"), Some(600));
        assert_eq!(sale_price("flyer"), None);
        assert_eq!(sale_price("small_change"), None);
        assert_eq!(sale_price("nonsense"), None);
    }
}
