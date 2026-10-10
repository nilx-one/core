// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use unicode_normalization::is_nfc;

use crate::{DecimalU64, GEO_COORDINATE_E7_SCALE, GeoCoordinate, PubDress, SpokenLineId};

/// Maximum length of one spoken line, in Unicode scalar values.
pub const SPOKEN_TEXT_MAX_SCALARS: usize = 280;

/// Largest earshot radius the distance approximation is specified for.
pub const EARSHOT_MAX_METERS: u32 = 5_000;

/// Text a Bond speaks aloud on the map.
///
/// A spoken text is short, already NFC-normalized, and free of control
/// characters other than line feed. It is preserved exactly: no trimming,
/// folding, or truncation happens at this boundary, so a producer that has a
/// longer source must decide what to say before it constructs the value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpokenText(String);

impl SpokenText {
    /// Returns the exact canonical text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SpokenText {
    type Error = SpokenTextError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(SpokenTextError::Empty);
        }
        if value.chars().count() > SPOKEN_TEXT_MAX_SCALARS {
            return Err(SpokenTextError::TooLong);
        }
        if value.chars().any(is_disallowed_scalar) {
            return Err(SpokenTextError::DisallowedScalar);
        }
        if value.trim() != value {
            return Err(SpokenTextError::SurroundingWhitespace);
        }
        if !is_nfc(&value) {
            return Err(SpokenTextError::NotNfc);
        }
        Ok(Self(value))
    }
}

impl FromStr for SpokenText {
    type Err = SpokenTextError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for SpokenText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for SpokenText {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SpokenText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(D::Error::custom)
    }
}

fn is_disallowed_scalar(value: char) -> bool {
    (value.is_control() && value != '\n') || matches!(value, '\u{2028}' | '\u{2029}')
}

/// Stable failure classification for [`SpokenText`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpokenTextError {
    /// The text is empty.
    Empty,
    /// The text exceeds [`SPOKEN_TEXT_MAX_SCALARS`] Unicode scalar values.
    TooLong,
    /// The text contains a control character other than line feed, or a line
    /// or paragraph separator.
    DisallowedScalar,
    /// The text starts or ends with whitespace.
    SurroundingWhitespace,
    /// The text is not in Unicode normalization form C.
    NotNfc,
}

impl SpokenTextError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::TooLong => "too_long",
            Self::DisallowedScalar => "disallowed_scalar",
            Self::SurroundingWhitespace => "surrounding_whitespace",
            Self::NotNfc => "not_nfc",
        }
    }
}

impl fmt::Display for SpokenTextError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for SpokenTextError {}

/// One line a Bond speaks aloud, addressed to whoever is within earshot.
///
/// A spoken line is presentation of an utterance. It is not an Interaction,
/// creates no `BondChain` entry, implies no consent or acquaintance between
/// the speaker and a listener, and does not establish where either of them
/// physically was. Who hears it is decided by [`within_earshot`] against
/// locations the caller already holds; the line carries no coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpokenLine {
    id: SpokenLineId,
    speaker: PubDress,
    text: SpokenText,
    spoken_at: DecimalU64,
}

impl SpokenLine {
    /// Creates one line. `spoken_at` is Unix epoch seconds.
    #[must_use]
    pub const fn new(
        id: SpokenLineId,
        speaker: PubDress,
        text: SpokenText,
        spoken_at: DecimalU64,
    ) -> Self {
        Self {
            id,
            speaker,
            text,
            spoken_at,
        }
    }

    /// Stable identity of the line; the same utterance always has the same id.
    #[must_use]
    pub const fn id(&self) -> &SpokenLineId {
        &self.id
    }

    /// The Bond that speaks.
    #[must_use]
    pub const fn speaker(&self) -> &PubDress {
        &self.speaker
    }

    /// What is spoken.
    #[must_use]
    pub const fn text(&self) -> &SpokenText {
        &self.text
    }

    /// When it was spoken, in Unix epoch seconds.
    #[must_use]
    pub const fn spoken_at(&self) -> DecimalU64 {
        self.spoken_at
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SpokenLineWire {
    id: SpokenLineId,
    spoken_at: DecimalU64,
    speaker: String,
    text: SpokenText,
}

impl Serialize for SpokenLine {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SpokenLineWire {
            id: self.id.clone(),
            spoken_at: self.spoken_at,
            speaker: self.speaker.as_str().to_owned(),
            text: self.text.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SpokenLine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SpokenLineWire::deserialize(deserializer)?;
        let speaker = wire.speaker.parse().map_err(D::Error::custom)?;
        Ok(Self::new(wire.id, speaker, wire.text, wire.spoken_at))
    }
}

/// How far a spoken line carries, in whole meters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EarshotRadius(u32);

impl EarshotRadius {
    /// Creates a radius between 1 and [`EARSHOT_MAX_METERS`] meters inclusive.
    ///
    /// # Errors
    ///
    /// Returns [`EarshotRadiusError`] outside that range.
    pub const fn new(meters: u32) -> Result<Self, EarshotRadiusError> {
        if meters == 0 || meters > EARSHOT_MAX_METERS {
            Err(EarshotRadiusError)
        } else {
            Ok(Self(meters))
        }
    }

    /// The radius in whole meters.
    #[must_use]
    pub const fn meters(self) -> u32 {
        self.0
    }
}

/// Failure returned when an earshot radius is outside the specified range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarshotRadiusError;

impl fmt::Display for EarshotRadiusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "earshot radius must be between 1 and {EARSHOT_MAX_METERS} meters"
        )
    }
}

impl std::error::Error for EarshotRadiusError {}

/// Meters per degree of latitude used by the distance approximation.
pub(crate) const CENTIMETERS_PER_DEGREE: i128 = 11_132_000;
const E7: i128 = GEO_COORDINATE_E7_SCALE as i128;
const COSINE_SCALE: i128 = 1_000_000;

/// `cos(d°)` in millionths for whole degrees 0..=90.
const COSINE_PER_DEGREE: [i64; 91] = [
    1_000_000, 999_848, 999_391, 998_630, 997_564, 996_195, 994_522, 992_546, 990_268, 987_688,
    984_808, 981_627, 978_148, 974_370, 970_296, 965_926, 961_262, 956_305, 951_057, 945_519,
    939_693, 933_580, 927_184, 920_505, 913_545, 906_308, 898_794, 891_007, 882_948, 874_620,
    866_025, 857_167, 848_048, 838_671, 829_038, 819_152, 809_017, 798_636, 788_011, 777_146,
    766_044, 754_710, 743_145, 731_354, 719_340, 707_107, 694_658, 681_998, 669_131, 656_059,
    642_788, 629_320, 615_661, 601_815, 587_785, 573_576, 559_193, 544_639, 529_919, 515_038,
    500_000, 484_810, 469_472, 453_990, 438_371, 422_618, 406_737, 390_731, 374_607, 358_368,
    342_020, 325_568, 309_017, 292_372, 275_637, 258_819, 241_922, 224_951, 207_912, 190_809,
    173_648, 156_434, 139_173, 121_869, 104_528, 87_156, 69_756, 52_336, 34_899, 17_452, 0,
];

/// `cos` of an absolute latitude in E7 degrees, in millionths, by linear
/// interpolation of [`COSINE_PER_DEGREE`]. Integer-only, so every runtime agrees.
pub(crate) fn cosine_millionths(absolute_latitude_e7: i64) -> i128 {
    let degree_e7 = i64::from(GEO_COORDINATE_E7_SCALE);
    let whole = usize::try_from(absolute_latitude_e7 / degree_e7).map_or(90, |v| v.min(90));
    let fraction = i128::from(absolute_latitude_e7 % degree_e7);
    let low = i128::from(COSINE_PER_DEGREE[whole]);
    let high = i128::from(COSINE_PER_DEGREE[(whole + 1).min(90)]);
    low + (high - low) * fraction / E7
}

/// Distance between two coordinates in whole meters, rounded down.
///
/// The result is an equirectangular approximation computed with integer
/// arithmetic only, so it is identical on every runtime. It is specified for
/// distances up to [`EARSHOT_MAX_METERS`] away from the poles; it must not be
/// used for navigation or for anything that needs a geodesic.
#[must_use]
pub fn distance_meters(from: GeoCoordinate, to: GeoCoordinate) -> u32 {
    let full_turn = 360 * i64::from(GEO_COORDINATE_E7_SCALE);
    let half_turn = full_turn / 2;
    let mut delta_longitude = i64::from(to.longitude_e7()) - i64::from(from.longitude_e7());
    if delta_longitude > half_turn {
        delta_longitude -= full_turn;
    } else if delta_longitude < -half_turn {
        delta_longitude += full_turn;
    }
    let delta_latitude = i64::from(to.latitude_e7()) - i64::from(from.latitude_e7());
    let middle_latitude = (i64::from(to.latitude_e7()) + i64::from(from.latitude_e7())).abs() / 2;

    let north_cm = i128::from(delta_latitude) * CENTIMETERS_PER_DEGREE / E7;
    let east_cm =
        i128::from(delta_longitude) * CENTIMETERS_PER_DEGREE * cosine_millionths(middle_latitude)
            / (E7 * COSINE_SCALE);
    let squared = (north_cm * north_cm + east_cm * east_cm).unsigned_abs();
    let meters = squared.isqrt() / 100;
    u32::try_from(meters).unwrap_or(u32::MAX)
}

/// Whether a listener at `listener` can hear a speaker at `speaker`.
///
/// The bound is inclusive and symmetric.
#[must_use]
pub fn within_earshot(
    speaker: GeoCoordinate,
    listener: GeoCoordinate,
    radius: EarshotRadius,
) -> bool {
    distance_meters(speaker, listener) <= radius.meters()
}

#[cfg(test)]
mod tests {
    use super::{
        EARSHOT_MAX_METERS, EarshotRadius, SPOKEN_TEXT_MAX_SCALARS, SpokenLine, SpokenText,
        SpokenTextError, distance_meters, within_earshot,
    };
    use crate::{DecimalU64, GeoCoordinate, PubDress, SpokenLineId, canonical_json};

    fn at(longitude: f64, latitude: f64) -> GeoCoordinate {
        GeoCoordinate::from_degrees(longitude, latitude).expect("valid coordinate")
    }

    fn haversine_meters(a: GeoCoordinate, b: GeoCoordinate) -> f64 {
        let (lat_a, lat_b) = (
            a.latitude_degrees().to_radians(),
            b.latitude_degrees().to_radians(),
        );
        let d_lat = lat_b - lat_a;
        let d_lon = (b.longitude_degrees() - a.longitude_degrees()).to_radians();
        let h =
            (d_lat / 2.0).sin().powi(2) + lat_a.cos() * lat_b.cos() * (d_lon / 2.0).sin().powi(2);
        2.0 * 6_371_008.8 * h.sqrt().asin()
    }

    #[test]
    fn spoken_text_accepts_ordinary_speech_exactly() {
        for value in ["привіт", "hello\nworld", "café", "😀 ok"] {
            let parsed: SpokenText = value.parse().expect("speech must parse");
            assert_eq!(parsed.as_str(), value);
        }
    }

    #[test]
    fn spoken_text_classifies_failures() {
        assert_eq!("".parse::<SpokenText>(), Err(SpokenTextError::Empty));
        assert_eq!(
            "a".repeat(SPOKEN_TEXT_MAX_SCALARS + 1)
                .parse::<SpokenText>(),
            Err(SpokenTextError::TooLong)
        );
        assert!(
            "a".repeat(SPOKEN_TEXT_MAX_SCALARS)
                .parse::<SpokenText>()
                .is_ok()
        );
        for value in ["a\u{0007}b", "a\tb", "a\u{2028}b", "a\r\nb"] {
            assert_eq!(
                value.parse::<SpokenText>(),
                Err(SpokenTextError::DisallowedScalar),
                "accepted {value:?}"
            );
        }
        for value in [" a", "a ", "\na"] {
            assert_eq!(
                value.parse::<SpokenText>(),
                Err(SpokenTextError::SurroundingWhitespace),
                "accepted {value:?}"
            );
        }
        assert_eq!(
            "cafe\u{0301}".parse::<SpokenText>(),
            Err(SpokenTextError::NotNfc)
        );
    }

    #[test]
    fn spoken_text_counts_scalars_not_bytes() {
        assert!(
            "я".repeat(SPOKEN_TEXT_MAX_SCALARS)
                .parse::<SpokenText>()
                .is_ok()
        );
    }

    #[test]
    fn spoken_text_exposes_stable_binding_codes() {
        assert_eq!(SpokenTextError::Empty.code(), "empty");
        assert_eq!(SpokenTextError::TooLong.code(), "too_long");
        assert_eq!(
            SpokenTextError::DisallowedScalar.code(),
            "disallowed_scalar"
        );
        assert_eq!(
            SpokenTextError::SurroundingWhitespace.code(),
            "surrounding_whitespace"
        );
        assert_eq!(SpokenTextError::NotNfc.code(), "not_nfc");
    }

    #[test]
    fn line_is_canonical_json_and_round_trips() {
        let line = SpokenLine::new(
            format!("line_{}", "a".repeat(64))
                .parse::<SpokenLineId>()
                .expect("id"),
            "0x0sky".parse::<PubDress>().expect("pub_dress"),
            "привіт".parse().expect("text"),
            DecimalU64::new(1_800_000_000),
        );
        let encoded = canonical_json(&line).expect("line must be canonical");
        assert_eq!(
            String::from_utf8(encoded.clone()).expect("utf8"),
            format!(
                r#"{{"id":"line_{}","speaker":"0x0sky","spoken_at":"1800000000","text":"привіт"}}"#,
                "a".repeat(64)
            )
        );
        let decoded: SpokenLine = serde_json::from_slice(&encoded).expect("round trip");
        assert_eq!(decoded, line);
    }

    #[test]
    fn line_rejects_unknown_fields_and_invalid_parts() {
        let id = format!("line_{}", "a".repeat(64));
        let good = format!(r#"{{"id":"{id}","spoken_at":"1","speaker":"0x0sky","text":"hi"}}"#);
        assert!(serde_json::from_str::<SpokenLine>(&good).is_ok());
        for bad in [
            format!(r#"{{"id":"{id}","spoken_at":"1","speaker":"0x0sky","text":"hi","x":"1"}}"#),
            format!(r#"{{"id":"{id}","spoken_at":"1","speaker":"sky","text":"hi"}}"#),
            format!(r#"{{"id":"{id}","spoken_at":"01","speaker":"0x0sky","text":"hi"}}"#),
            format!(r#"{{"id":"{id}","spoken_at":"1","speaker":"0x0sky","text":" hi"}}"#),
            r#"{"id":"line_A","spoken_at":"1","speaker":"0x0sky","text":"hi"}"#.to_owned(),
        ] {
            assert!(
                serde_json::from_str::<SpokenLine>(&bad).is_err(),
                "accepted {bad}"
            );
        }
    }

    #[test]
    fn radius_is_bounded() {
        assert!(EarshotRadius::new(0).is_err());
        assert!(EarshotRadius::new(1).is_ok());
        assert!(EarshotRadius::new(EARSHOT_MAX_METERS).is_ok());
        assert!(EarshotRadius::new(EARSHOT_MAX_METERS + 1).is_err());
    }

    #[test]
    fn identical_points_are_zero_meters_apart() {
        let point = at(30.5234, 50.4501);
        assert_eq!(distance_meters(point, point), 0);
    }

    #[test]
    fn distance_is_symmetric() {
        let a = at(30.5234, 50.4501);
        let b = at(30.5262, 50.4519);
        assert_eq!(distance_meters(a, b), distance_meters(b, a));
    }

    #[test]
    fn one_hundredth_degree_of_latitude_is_about_1113_meters() {
        let a = at(30.5234, 50.4500);
        let b = at(30.5234, 50.4600);
        assert_eq!(distance_meters(a, b), 1_113);
    }

    #[test]
    fn longitude_shrinks_with_latitude() {
        let equator = distance_meters(at(0.0, 0.0), at(0.01, 0.0));
        let kyiv = distance_meters(at(30.5234, 50.4501), at(30.5334, 50.4501));
        let high = distance_meters(at(10.0, 80.0), at(10.01, 80.0));
        assert_eq!(equator, 1_113);
        assert!((700..=720).contains(&kyiv), "kyiv {kyiv}");
        assert!((190..=200).contains(&high), "high {high}");
    }

    #[test]
    fn antimeridian_is_continuous() {
        let west = at(-179.995, 10.0);
        let east = at(179.995, 10.0);
        let meters = distance_meters(west, east);
        assert!((1_080..=1_115).contains(&meters), "{meters}");
    }

    #[test]
    fn approximation_tracks_haversine_within_earshot() {
        let origins = [
            (0.0, 0.0),
            (30.5234, 50.4501),
            (-73.98, 40.75),
            (18.4, -33.9),
            (10.0, 70.0),
        ];
        let steps = [
            (0.0, 0.0),
            (0.001, 0.0),
            (0.0, 0.004),
            (0.02, 0.02),
            (-0.03, 0.01),
            (0.04, -0.04),
        ];
        for (lon, lat) in origins {
            for (d_lon, d_lat) in steps {
                let a = at(lon, lat);
                let b = at(lon + d_lon, lat + d_lat);
                let exact = haversine_meters(a, b);
                if exact > f64::from(EARSHOT_MAX_METERS) {
                    continue;
                }
                let approx = f64::from(distance_meters(a, b));
                assert!(
                    (approx - exact).abs() <= 1.0 + exact * 0.01,
                    "({lon},{lat}) + ({d_lon},{d_lat}): approx {approx} vs exact {exact}"
                );
            }
        }
    }

    #[test]
    fn poles_and_extremes_do_not_panic() {
        let north = at(0.0, 90.0);
        let south = at(180.0, -90.0);
        let _ = distance_meters(north, south);
        let _ = distance_meters(at(-180.0, -90.0), at(180.0, 90.0));
        assert_eq!(distance_meters(north, north), 0);
    }

    #[test]
    fn earshot_bound_is_inclusive() {
        let speaker = at(30.5234, 50.4500);
        let listener = at(30.5234, 50.4600);
        let exact = distance_meters(speaker, listener);
        assert!(within_earshot(
            speaker,
            listener,
            EarshotRadius::new(exact).expect("radius")
        ));
        assert!(!within_earshot(
            speaker,
            listener,
            EarshotRadius::new(exact - 1).expect("radius")
        ));
    }
}
