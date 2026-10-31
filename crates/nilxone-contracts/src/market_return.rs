// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The trip back from a Market visit, as a client carries it.
//!
//! A tab that hands its Bond over to 0xda-market keeps the xPing route it
//! flew out on, so that xPing can fly home once the tab is back. Only
//! Identity's completion check confirms a purchase; this contract only says
//! which kept route a client may fly, and how long an order Identity could
//! not yet confirm is asked about again. See `docs/xping-flyover.md`.

use core::str::FromStr;

use serde::Deserialize;

use crate::{GeoCoordinate, PubDress, StationPubDress};

/// The business whose stations a Market visit leaves from.
pub const MARKET_OWNER: &str = "0xda-market";

/// How long an order Identity has not confirmed is still asked about.
pub const MARKET_RETURN_PENDING_MS: u64 = 24 * 60 * 60 * 1000;

/// While a page is open, an unconfirmed order is asked about again after
/// each of these delays, then not until the tab's next page load.
pub const MARKET_RETURN_RECHECK_MS: [u64; 3] = [30_000, 120_000, 600_000];

#[derive(Deserialize)]
struct TripWire {
    origin: GeoCoordinate,
    destination: DestinationWire,
}

#[derive(Deserialize)]
struct DestinationWire {
    id: PointIdWire,
    pub_dress: String,
    coordinate: GeoCoordinate,
}

#[derive(Deserialize)]
struct PointIdWire {
    owner_pub_dress: String,
    number: u64,
}

/// Why a kept route may not be flown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketReturnTripError {
    /// It does not parse, or a coordinate is out of range.
    Invalid,
    /// Its destination is not one of 0xda-market's stations.
    NotMarketStation,
}

impl MarketReturnTripError {
    /// Stable wire code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::NotMarketStation => "not_market_station",
        }
    }
}

/// Whether `trip`, a kept `CourierPlan` as JSON, is a route a client may fly
/// home along: both ends are valid coordinates and it leads to one of
/// 0xda-market's stations. Fields this contract does not read are ignored.
///
/// # Errors
///
/// [`MarketReturnTripError`] for a route that must be dropped.
pub fn market_return_trip(trip: &str) -> Result<(), MarketReturnTripError> {
    let trip: TripWire = serde_json::from_str(trip).map_err(|_| MarketReturnTripError::Invalid)?;
    let _ = (
        trip.origin,
        trip.destination.coordinate,
        trip.destination.id.number,
    );
    let owner = PubDress::from_str(MARKET_OWNER).map_err(|_| MarketReturnTripError::Invalid)?;
    if trip.destination.id.owner_pub_dress != MARKET_OWNER
        || StationPubDress::for_owner(&owner, &trip.destination.pub_dress).is_err()
    {
        return Err(MarketReturnTripError::NotMarketStation);
    }
    Ok(())
}

/// [`market_return_trip`] as JSON for Wasm and `UniFFI`: `{"ok":true}` or
/// `{"ok":false,"error":<code>}`.
#[must_use]
pub fn market_return_trip_wire(trip: &str) -> String {
    match market_return_trip(trip) {
        Ok(()) => serde_json::json!({ "ok": true }),
        Err(error) => serde_json::json!({ "ok": false, "error": error.code() }),
    }
    .to_string()
}

/// The pending-order policy as JSON for clients:
/// `{"pending_ms":…,"recheck_ms":[…]}`.
#[must_use]
pub fn market_return_policy_wire() -> String {
    serde_json::json!({
        "pending_ms": MARKET_RETURN_PENDING_MS,
        "recheck_ms": MARKET_RETURN_RECHECK_MS,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trip(origin: (&str, &str), owner: &str, station: &str, to: (&str, &str)) -> String {
        serde_json::json!({
            "origin": { "longitude_e7": origin.0, "latitude_e7": origin.1 },
            "destination": {
                "id": { "owner_pub_dress": owner, "number": 0 },
                "pub_dress": station,
                "description": "Golden Gate",
                "isVerified": false,
                "coordinate": { "longitude_e7": to.0, "latitude_e7": to.1 },
            },
            "started_at_ms": "1000",
        })
        .to_string()
    }

    const KYIV: (&str, &str) = ("305000000", "504000000");
    const GATE: (&str, &str) = ("305131000", "504489000");

    #[test]
    fn a_route_to_a_market_station_may_be_flown() {
        let good = trip(KYIV, MARKET_OWNER, "0xda-market-zoloti-vorota", GATE);
        assert_eq!(market_return_trip(&good), Ok(()));
        assert_eq!(market_return_trip_wire(&good), r#"{"ok":true}"#);
    }

    #[test]
    fn a_route_out_of_range_or_malformed_is_dropped() {
        for bad in [
            trip(
                ("9999999999", "504000000"),
                MARKET_OWNER,
                "0xda-market-zoloti-vorota",
                GATE,
            ),
            trip(
                KYIV,
                MARKET_OWNER,
                "0xda-market-zoloti-vorota",
                ("305131000", "-900000001"),
            ),
            trip(
                ("30.5", "50.4"),
                MARKET_OWNER,
                "0xda-market-zoloti-vorota",
                GATE,
            ),
            "{}".to_owned(),
            "not json".to_owned(),
        ] {
            assert_eq!(
                market_return_trip(&bad),
                Err(MarketReturnTripError::Invalid),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_route_must_lead_to_one_of_markets_stations() {
        for (owner, station) in [
            ("0x0sky", "0xda-market-zoloti-vorota"),
            (MARKET_OWNER, "0x0sky-zoloti-vorota"),
            (MARKET_OWNER, "0xda-market"),
            (MARKET_OWNER, "javascript:alert(1)"),
        ] {
            let bad = trip(KYIV, owner, station, GATE);
            assert_eq!(
                market_return_trip(&bad),
                Err(MarketReturnTripError::NotMarketStation),
                "{owner} {station}"
            );
            assert_eq!(
                market_return_trip_wire(&bad),
                r#"{"error":"not_market_station","ok":false}"#
            );
        }
    }

    #[test]
    fn the_policy_is_a_day_with_three_page_rechecks() {
        assert_eq!(
            market_return_policy_wire(),
            r#"{"pending_ms":86400000,"recheck_ms":[30000,120000,600000]}"#
        );
    }
}
