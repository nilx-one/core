// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::DecimalU64;

/// Decimal scale used for portable geographic coordinates.
///
/// E7 preserves roughly centimetre-level geographic precision while keeping the
/// contract integer-based and deterministic across Rust, WebAssembly, and FFI.
pub const GEO_COORDINATE_E7_SCALE: i32 = 10_000_000;
const LONGITUDE_E7_LIMIT: i32 = 180 * GEO_COORDINATE_E7_SCALE;
const LATITUDE_E7_LIMIT: i32 = 90 * GEO_COORDINATE_E7_SCALE;

/// A WGS84 coordinate encoded as signed decimal degrees multiplied by 10^7.
///
/// The JSON representation uses decimal strings because Core contract `0.1.0`
/// forbids JSON numeric tokens. UTS #46 does not apply to geographic values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GeoCoordinate {
    longitude_e7: i32,
    latitude_e7: i32,
}

impl GeoCoordinate {
    /// Creates a validated coordinate from E7 components.
    ///
    /// # Errors
    ///
    /// Returns [`GeoCoordinateError`] when longitude or latitude is outside the
    /// WGS84 degree range.
    pub const fn new(longitude_e7: i32, latitude_e7: i32) -> Result<Self, GeoCoordinateError> {
        if longitude_e7 < -LONGITUDE_E7_LIMIT || longitude_e7 > LONGITUDE_E7_LIMIT {
            return Err(GeoCoordinateError::LongitudeOutOfRange);
        }
        if latitude_e7 < -LATITUDE_E7_LIMIT || latitude_e7 > LATITUDE_E7_LIMIT {
            return Err(GeoCoordinateError::LatitudeOutOfRange);
        }
        Ok(Self {
            longitude_e7,
            latitude_e7,
        })
    }

    /// Returns longitude in E7 signed degrees.
    #[must_use]
    pub const fn longitude_e7(self) -> i32 {
        self.longitude_e7
    }

    /// Returns latitude in E7 signed degrees.
    #[must_use]
    pub const fn latitude_e7(self) -> i32 {
        self.latitude_e7
    }

    /// Converts longitude to degrees for platform geographic APIs.
    #[must_use]
    pub fn longitude_degrees(self) -> f64 {
        f64::from(self.longitude_e7) / f64::from(GEO_COORDINATE_E7_SCALE)
    }

    /// Converts latitude to degrees for platform geographic APIs.
    #[must_use]
    pub fn latitude_degrees(self) -> f64 {
        f64::from(self.latitude_e7) / f64::from(GEO_COORDINATE_E7_SCALE)
    }

    /// Quantizes a finite WGS84 degree pair to E7.
    ///
    /// # Errors
    ///
    /// Returns [`GeoCoordinateError`] for non-finite values or values outside
    /// the valid longitude/latitude range.
    pub fn from_degrees(longitude: f64, latitude: f64) -> Result<Self, GeoCoordinateError> {
        if !longitude.is_finite() || !latitude.is_finite() {
            return Err(GeoCoordinateError::NonFinite);
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(GeoCoordinateError::LongitudeOutOfRange);
        }
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(GeoCoordinateError::LatitudeOutOfRange);
        }

        #[allow(clippy::cast_possible_truncation)]
        let longitude_e7 = (longitude * f64::from(GEO_COORDINATE_E7_SCALE)).round() as i32;
        #[allow(clippy::cast_possible_truncation)]
        let latitude_e7 = (latitude * f64::from(GEO_COORDINATE_E7_SCALE)).round() as i32;
        Self::new(longitude_e7, latitude_e7)
    }
}

impl Serialize for GeoCoordinate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            latitude_e7: &'a str,
            longitude_e7: &'a str,
        }

        let longitude = self.longitude_e7.to_string();
        let latitude = self.latitude_e7.to_string();
        Wire {
            latitude_e7: &latitude,
            longitude_e7: &longitude,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for GeoCoordinate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            longitude_e7: String,
            latitude_e7: String,
        }

        let wire = Wire::deserialize(deserializer)?;
        let longitude = parse_canonical_i32(&wire.longitude_e7).map_err(D::Error::custom)?;
        let latitude = parse_canonical_i32(&wire.latitude_e7).map_err(D::Error::custom)?;
        Self::new(longitude, latitude).map_err(D::Error::custom)
    }
}

fn parse_canonical_i32(value: &str) -> Result<i32, GeoCoordinateError> {
    if value.is_empty()
        || value == "-0"
        || (value.starts_with('0') && value.len() > 1)
        || (value.starts_with("-0") && value.len() > 2)
    {
        return Err(GeoCoordinateError::NonCanonicalInteger);
    }
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(GeoCoordinateError::NonCanonicalInteger);
    }
    value
        .parse::<i32>()
        .map_err(|_| GeoCoordinateError::NonCanonicalInteger)
}

/// Provenance of the active Bond location coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BondLocationMode {
    /// Coordinate came from an explicit current-location observation.
    Live,
    /// Coordinate was explicitly selected by authorized application admin logic.
    Manual,
}

/// Single-owner operational location associated with one Bond.
///
/// This value is not an Interaction, BondChain, Relationship, or public map
/// presence. `Manual` is a declared point and must never be interpreted as a
/// physical observation merely because it is persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BondLocation {
    pub coordinate: GeoCoordinate,
    pub mode: BondLocationMode,
    pub updated_at: DecimalU64,
}

impl BondLocation {
    /// Creates one location state from a coordinate, provenance mode, and the
    /// timestamp at which the input was accepted.
    #[must_use]
    pub const fn new(
        coordinate: GeoCoordinate,
        mode: BondLocationMode,
        updated_at: DecimalU64,
    ) -> Self {
        Self {
            coordinate,
            mode,
            updated_at,
        }
    }
}

/// Stable validation failures for [`GeoCoordinate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoCoordinateError {
    NonFinite,
    LongitudeOutOfRange,
    LatitudeOutOfRange,
    NonCanonicalInteger,
}

impl fmt::Display for GeoCoordinateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFinite => "coordinate must be finite",
            Self::LongitudeOutOfRange => "longitude is outside WGS84 range",
            Self::LatitudeOutOfRange => "latitude is outside WGS84 range",
            Self::NonCanonicalInteger => "coordinate component is not canonical decimal i32",
        })
    }
}

impl std::error::Error for GeoCoordinateError {}

#[cfg(test)]
mod tests {
    use crate::{BondLocation, BondLocationMode, DecimalU64, GeoCoordinate, canonical_json};

    #[test]
    fn quantizes_degrees_to_e7_and_back() {
        let coordinate = GeoCoordinate::from_degrees(30.5234, 50.4501).expect("valid coordinate");
        assert_eq!(coordinate.longitude_e7(), 305_234_000);
        assert_eq!(coordinate.latitude_e7(), 504_501_000);
        assert!((coordinate.longitude_degrees() - 30.5234).abs() < f64::EPSILON);
        assert!((coordinate.latitude_degrees() - 50.4501).abs() < f64::EPSILON);
    }

    #[test]
    fn rejects_invalid_geography() {
        assert!(GeoCoordinate::from_degrees(f64::NAN, 0.0).is_err());
        assert!(GeoCoordinate::from_degrees(180.000_000_1, 0.0).is_err());
        assert!(GeoCoordinate::from_degrees(0.0, -90.000_000_1).is_err());
    }

    #[test]
    fn location_is_canonical_json_without_numeric_tokens() {
        let coordinate = GeoCoordinate::from_degrees(30.5234, 50.4501).expect("valid coordinate");
        let location = BondLocation::new(
            coordinate,
            BondLocationMode::Live,
            DecimalU64::new(1_800_000_000),
        );
        let encoded = canonical_json(&location).expect("location must be canonical");
        assert_eq!(
            encoded,
            br#"{"coordinate":{"latitude_e7":"504501000","longitude_e7":"305234000"},"mode":"live","updated_at":"1800000000"}"#
        );
    }

    #[test]
    fn rejects_noncanonical_wire_components() {
        for longitude in ["0305234000", "-0", "+1"] {
            let input = format!(
                r#"{{"longitude_e7":"{longitude}","latitude_e7":"504501000"}}"#
            );
            assert!(serde_json::from_str::<GeoCoordinate>(&input).is_err());
        }
    }
}
