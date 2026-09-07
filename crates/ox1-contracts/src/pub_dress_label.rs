// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Canonical public DNS-label contract for a Bond `PubDress`.
//!
//! A `PubDress` is case-sensitive while DNS labels are not. The label fold is
//! therefore intentionally non-injective: distinct identities such as
//! `0x0Sky` and `0x0sky` can produce the same label and must be disambiguated by
//! allocation, not by reverse computation.
//!
//! Version 0.1 refuses non-ASCII label derivation. It does not apply IDNA,
//! UTS-46, or transliteration. Canonical `PubDress` values that contain an
//! allowed non-ASCII scalar therefore remain valid identities but have no
//! `PubDressLabel` until a later protocol version explicitly defines a mapping.

use core::{fmt, str::FromStr};

use crate::PubDress;

const PREFIX: &str = "0x";

/// Maximum size of one DNS label in octets.
pub const PUB_DRESS_LABEL_MAX_OCTETS: usize = 63;
/// Maximum number of ASCII characters in a collision-disambiguation suffix.
pub const PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH: usize = 8;

/// Fixed label stem derived from a canonical `PubDress` before allocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PubDressStem {
    value: String,
    folded: bool,
}

impl PubDressStem {
    /// Returns the canonical lowercase ASCII stem.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Reports whether ASCII case folding changed the original `PubDress`.
    #[must_use]
    pub const fn was_folded(&self) -> bool {
        self.folded
    }
}

impl fmt::Display for PubDressStem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

/// Validated, allocatable DNS label for a Bond public address.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PubDressLabel {
    value: String,
}

impl PubDressLabel {
    /// Derives the fixed DNS-label stem from a canonical `PubDress`.
    ///
    /// Only the slug is folded. The discriminator is already constrained by
    /// `PubDress` to one lowercase hexadecimal digit.
    ///
    /// # Errors
    ///
    /// Returns [`PubDressLabelError::NonAscii`] when a canonical `PubDress`
    /// contains any non-ASCII scalar, or another label-shape error when the
    /// folded value cannot be represented as one DNS label.
    pub fn stem(pub_dress: &PubDress) -> Result<PubDressStem, PubDressLabelError> {
        if !pub_dress.as_str().is_ascii() {
            return Err(PubDressLabelError::NonAscii);
        }

        let folded_slug = pub_dress.slug().to_ascii_lowercase();
        let mut value = String::with_capacity(pub_dress.as_str().len());
        value.push_str(PREFIX);
        value.push(pub_dress.discriminator());
        value.push_str(&folded_slug);

        validate_label_text(&value)?;
        validate_bond_namespace(&value)?;

        Ok(PubDressStem {
            folded: value != pub_dress.as_str(),
            value,
        })
    }

    /// Raw-string convenience boundary for callers that have not yet parsed a
    /// canonical `PubDress`.
    ///
    /// This boundary exists so invalid identity input remains distinguishable
    /// from a valid identity that cannot be represented as an ASCII DNS label.
    ///
    /// # Errors
    ///
    /// Returns [`PubDressLabelError::NotAPubDress`] when `value` is not a
    /// canonical `PubDress`; otherwise returns the same errors as [`Self::stem`].
    pub fn stem_from_str(value: &str) -> Result<PubDressStem, PubDressLabelError> {
        let pub_dress = value
            .parse::<PubDress>()
            .map_err(|_| PubDressLabelError::NotAPubDress)?;
        Self::stem(&pub_dress)
    }

    /// Composes a fixed stem and collision-disambiguation suffix.
    ///
    /// The suffix is ASCII-folded exactly like the stem. Validation applies to
    /// the complete label so a trailing hyphen is rejected regardless of which
    /// input contributed it.
    ///
    /// # Errors
    ///
    /// Returns [`PubDressLabelError::SuffixTooLong`] when `suffix` exceeds the
    /// suffix budget, [`PubDressLabelError::NonAscii`] for non-ASCII suffixes,
    /// or another label-shape error when the composition is not allocatable.
    pub fn compose(stem: &PubDressStem, suffix: &str) -> Result<Self, PubDressLabelError> {
        if !suffix.is_ascii() {
            return Err(PubDressLabelError::NonAscii);
        }
        if suffix.len() > PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH {
            return Err(PubDressLabelError::SuffixTooLong);
        }

        let folded_suffix = suffix.to_ascii_lowercase();
        let mut value = String::with_capacity(stem.value.len() + folded_suffix.len());
        value.push_str(&stem.value);
        value.push_str(&folded_suffix);

        validate_label_text(&value)?;
        validate_bond_namespace(&value)?;

        Ok(Self { value })
    }

    /// Parses and canonicalizes a label supplied to a resolution boundary.
    ///
    /// ASCII uppercase is folded to lowercase because DNS label comparison is
    /// case-insensitive. The returned value is always in the Bond `0x`
    /// namespace.
    ///
    /// # Errors
    ///
    /// Returns a stable [`PubDressLabelError`] when `label` is non-ASCII,
    /// outside the Bond namespace, or not a valid single DNS label.
    pub fn parse(label: &str) -> Result<Self, PubDressLabelError> {
        if !label.is_ascii() {
            return Err(PubDressLabelError::NonAscii);
        }

        let value = label.to_ascii_lowercase();
        validate_label_text(&value)?;
        validate_bond_namespace(&value)?;

        Ok(Self { value })
    }

    /// Returns the canonical lowercase ASCII label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl FromStr for PubDressLabel {
    type Err = PubDressLabelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl fmt::Display for PubDressLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

/// Stable failure classification for `PubDressLabel` derivation and parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubDressLabelError {
    /// Raw input was not a canonical `PubDress`.
    NotAPubDress,
    /// Version 0.1 intentionally has no non-ASCII address mapping.
    NonAscii,
    /// The ASCII label contains a character outside `[a-z0-9-]` or is outside
    /// the Bond `0x` namespace.
    InvalidCharacter,
    /// The complete label begins or ends with `-`.
    BoundaryHyphen,
    /// The complete DNS label exceeds 63 octets.
    TooLong,
    /// A collision suffix exceeds its eight-character budget.
    SuffixTooLong,
}

impl PubDressLabelError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAPubDress => "not_a_pub_dress",
            Self::NonAscii => "non_ascii",
            Self::InvalidCharacter => "invalid_character",
            Self::BoundaryHyphen => "boundary_hyphen",
            Self::TooLong => "too_long",
            Self::SuffixTooLong => "suffix_too_long",
        }
    }
}

impl fmt::Display for PubDressLabelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for PubDressLabelError {}

fn validate_label_text(value: &str) -> Result<(), PubDressLabelError> {
    if value.len() > PUB_DRESS_LABEL_MAX_OCTETS {
        return Err(PubDressLabelError::TooLong);
    }
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(PubDressLabelError::InvalidCharacter);
    }
    if value.starts_with('-') || value.ends_with('-') {
        return Err(PubDressLabelError::BoundaryHyphen);
    }
    Ok(())
}

fn validate_bond_namespace(value: &str) -> Result<(), PubDressLabelError> {
    let bytes = value.as_bytes();
    if bytes.len() < 5
        || !value.starts_with(PREFIX)
        || !matches!(bytes[2], b'0'..=b'9' | b'a'..=b'f')
    {
        return Err(PubDressLabelError::InvalidCharacter);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        PUB_DRESS_LABEL_MAX_OCTETS, PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH, PubDressLabel,
        PubDressLabelError,
    };
    use crate::PubDress;

    fn stem(value: &str) -> super::PubDressStem {
        let pub_dress: PubDress = value.parse().expect("test pub_dress must be canonical");
        PubDressLabel::stem(&pub_dress).expect("test pub_dress must have an ASCII label")
    }

    #[test]
    fn derives_the_handoff_stem_vectors() {
        let cases = [
            ("0xda-sha", "0xda-sha", false),
            ("0xdA-Sha", "0xda-sha", true),
            ("0x0Sky", "0x0sky", true),
            ("0x0sky", "0x0sky", false),
        ];

        for (input, expected, folded) in cases {
            let pub_dress: PubDress = input.parse().expect("canonical test vector");
            let actual = PubDressLabel::stem(&pub_dress).expect("representable test vector");
            assert_eq!(actual.as_str(), expected);
            assert_eq!(actual.was_folded(), folded);
            assert!(actual.as_str().starts_with("0x"));
        }
    }

    #[test]
    fn keeps_case_distinct_identities_distinct_while_folding_their_labels_together() {
        let upper: PubDress = "0x0Sky".parse().expect("canonical pub_dress");
        let lower: PubDress = "0x0sky".parse().expect("canonical pub_dress");

        assert_ne!(upper, lower);
        assert_eq!(
            PubDressLabel::stem(&upper).expect("representable").as_str(),
            PubDressLabel::stem(&lower).expect("representable").as_str()
        );
    }

    #[test]
    fn raw_boundary_classifies_non_pub_dress_inputs() {
        for value in ["sky", "0xgsky", "0xDsky", "0x0небо"] {
            assert_eq!(
                PubDressLabel::stem_from_str(value),
                Err(PubDressLabelError::NotAPubDress),
                "misclassified {value:?}"
            );
        }
    }

    #[test]
    fn refuses_non_ascii_scalars_that_are_valid_in_the_current_pub_dress_contract() {
        let pub_dress: PubDress = "0x0₴€".parse().expect("canonical non-ASCII pub_dress");
        assert_eq!(
            PubDressLabel::stem(&pub_dress),
            Err(PubDressLabelError::NonAscii)
        );
    }

    #[test]
    fn rejects_unsupported_ascii_and_boundary_hyphen_stems() {
        let underscore: PubDress = "0x0sky_one".parse().expect("canonical pub_dress");
        let trailing_hyphen: PubDress = "0x0sky-".parse().expect("canonical pub_dress");

        assert_eq!(
            PubDressLabel::stem(&underscore),
            Err(PubDressLabelError::InvalidCharacter)
        );
        assert_eq!(
            PubDressLabel::stem(&trailing_hyphen),
            Err(PubDressLabelError::BoundaryHyphen)
        );
    }

    #[test]
    fn composes_the_handoff_suffix_vectors() {
        let sha = stem("0xda-sha");
        let sky = stem("0x0sky");

        assert_eq!(
            PubDressLabel::compose(&sha, "")
                .expect("valid label")
                .as_str(),
            "0xda-sha"
        );
        assert_eq!(
            PubDressLabel::compose(&sha, "7412")
                .expect("valid label")
                .as_str(),
            "0xda-sha7412"
        );
        assert_eq!(
            PubDressLabel::compose(&sky, "TWO")
                .expect("valid folded suffix")
                .as_str(),
            "0x0skytwo"
        );
        assert_eq!(
            PubDressLabel::compose(&sky, "2-"),
            Err(PubDressLabelError::BoundaryHyphen)
        );
        assert_eq!(
            PubDressLabel::compose(&sky, "a.b"),
            Err(PubDressLabelError::InvalidCharacter)
        );
        assert_eq!(
            PubDressLabel::compose(&sky, "111111111"),
            Err(PubDressLabelError::SuffixTooLong)
        );
    }

    #[test]
    fn decimal_suffixes_within_budget_always_compose() {
        let stem = stem("0xda-sha");
        for length in 0..=PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH {
            let suffix = "7".repeat(length);
            let label =
                PubDressLabel::compose(&stem, &suffix).expect("decimal suffix must compose");
            assert!(label.as_str().starts_with("0x"));
        }
    }

    #[test]
    fn parse_canonicalizes_ascii_case_and_rejects_service_namespace() {
        let label = PubDressLabel::parse("0x0SKY42").expect("valid Bond label");
        assert_eq!(label.as_str(), "0x0sky42");
        assert_eq!(
            PubDressLabel::parse("www"),
            Err(PubDressLabelError::InvalidCharacter)
        );
    }

    #[test]
    fn parse_rejects_non_ascii_boundary_and_overlong_labels() {
        assert_eq!(
            PubDressLabel::parse("0x0₴€"),
            Err(PubDressLabelError::NonAscii)
        );
        assert_eq!(
            PubDressLabel::parse("0x0sky-"),
            Err(PubDressLabelError::BoundaryHyphen)
        );

        let overlong = format!("0x0{}", "a".repeat(PUB_DRESS_LABEL_MAX_OCTETS));
        assert_eq!(
            PubDressLabel::parse(&overlong),
            Err(PubDressLabelError::TooLong)
        );
    }

    #[test]
    fn exposes_stable_failure_codes() {
        let cases = [
            (PubDressLabelError::NotAPubDress, "not_a_pub_dress"),
            (PubDressLabelError::NonAscii, "non_ascii"),
            (PubDressLabelError::InvalidCharacter, "invalid_character"),
            (PubDressLabelError::BoundaryHyphen, "boundary_hyphen"),
            (PubDressLabelError::TooLong, "too_long"),
            (PubDressLabelError::SuffixTooLong, "suffix_too_long"),
        ];

        for (error, code) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.to_string(), code);
        }
    }
}
