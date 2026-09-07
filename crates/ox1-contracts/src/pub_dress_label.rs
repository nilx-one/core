// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Canonical public DNS-label projection for a Bond `PubDress`.
//!
//! `PubDress` is the exact, case-sensitive identity selected by the Bond.
//! `PubDressLabel` is a separate DNS projection. Core owns the projection so
//! browsers, native clients, and servers cannot drift onto different IDNA rules.
//!
//! The projection is intentionally non-injective. Distinct identities such as
//! `0x0Sky` / `0x0sky` and `0x0Небо` / `0x0небо` can derive the same A-label.
//! Allocation therefore belongs to an atomic storage boundary; a label must
//! never be reverse-computed to identify a Bond.

use core::{fmt, str::FromStr};

use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};
use unicode_bidi::{BidiClass, bidi_class};

use crate::{PubDress, PubDressError, pub_dress::is_unicode_name_scalar};

const PREFIX: &str = "0x";

/// Maximum size of one DNS label in octets.
pub const PUB_DRESS_LABEL_MAX_OCTETS: usize = 63;
/// Maximum number of ASCII characters in a collision-disambiguation suffix.
pub const PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH: usize = 8;
/// Unicode version used by the canonical `PubDress` scalar-category table.
pub const PUB_DRESS_UNICODE_VERSION: &str = "16.0.0";
/// Exact UTS-46 implementation pinned by the Core contract.
pub const PUB_DRESS_UTS46_IMPLEMENTATION: &str = "idna=1.1.0;idna_adapter=1.1.0;idna_mapping=1.1.0";

/// Fixed label stem derived from a canonical `PubDress` before allocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PubDressStem {
    value: String,
    source: String,
    folded: bool,
}

impl PubDressStem {
    /// Returns the canonical lowercase ASCII DNS A-label.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the exact `PubDress` source used for derivation.
    ///
    /// This is retained so a collision suffix is composed before UTS-46 and
    /// the complete Unicode source is encoded exactly once.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Reports whether UTS-46 mapping changed the identity's textual form.
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
    /// Derives the DNS A-label stem from an exact canonical `PubDress`.
    ///
    /// Mapping is UTS-46 non-transitional with STD3 ASCII rules, CheckHyphens,
    /// CheckBidi, CheckJoiners, and VerifyDNSLength enabled. Core never
    /// pre-lowercases or normalizes the identity; UTS-46 owns mapping.
    ///
    /// # Errors
    ///
    /// Returns a stable [`PubDressLabelError`] when the identity is not
    /// representable as a single DNS label.
    pub fn stem(pub_dress: &PubDress) -> Result<PubDressStem, PubDressLabelError> {
        validate_source_scalars(pub_dress)?;
        let value = encode_source(pub_dress.as_str())?;

        Ok(PubDressStem {
            folded: value != pub_dress.as_str(),
            source: pub_dress.as_str().to_owned(),
            value,
        })
    }

    /// Raw-string convenience boundary for callers that have not parsed
    /// `PubDress` yet.
    ///
    /// # Errors
    ///
    /// Invalid identity syntax is reported as [`PubDressLabelError::NotAPubDress`].
    /// A structurally valid identity containing a non-ASCII scalar that is
    /// explicitly outside the human-name scalar policy is classified as
    /// [`PubDressLabelError::DisallowedScalar`].
    pub fn stem_from_str(value: &str) -> Result<PubDressStem, PubDressLabelError> {
        match value.parse::<PubDress>() {
            Ok(pub_dress) => Self::stem(&pub_dress),
            Err(PubDressError::InvalidCharacter) if contains_disallowed_unicode_scalar(value) => {
                Err(PubDressLabelError::DisallowedScalar)
            }
            Err(_) => Err(PubDressLabelError::NotAPubDress),
        }
    }

    /// Composes an ASCII collision suffix with the exact identity source and
    /// then runs UTS-46 once over the complete source.
    ///
    /// Appending to an already encoded `xn--` A-label is forbidden because the
    /// result would no longer be the encoding of the intended Unicode label.
    ///
    /// # Errors
    ///
    /// The suffix must be at most eight lowercase ASCII letters or digits.
    pub fn compose(stem: &PubDressStem, suffix: &str) -> Result<Self, PubDressLabelError> {
        if suffix.len() > PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH {
            return Err(PubDressLabelError::SuffixTooLong);
        }
        if !suffix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            return Err(PubDressLabelError::InvalidCharacter);
        }

        let mut source = String::with_capacity(stem.source.len() + suffix.len());
        source.push_str(&stem.source);
        source.push_str(suffix);

        let value = encode_source(&source)?;
        Ok(Self { value })
    }

    /// Parses and canonicalizes a DNS A-label supplied to a resolution boundary.
    ///
    /// The label is accepted only if strict UTS-46 decoding yields a canonical
    /// Bond `PubDress`. The decoded identity is validation evidence only and
    /// must not be used as reverse identity lookup.
    ///
    /// # Errors
    ///
    /// Returns a stable [`PubDressLabelError`] for invalid DNS or Bond namespace
    /// input.
    pub fn parse(label: &str) -> Result<Self, PubDressLabelError> {
        if !label.is_ascii() {
            return Err(PubDressLabelError::InvalidCharacter);
        }

        let value = label.to_ascii_lowercase();
        validate_a_label_text(&value)?;

        let uts46 = Uts46::new();
        let (unicode, result) =
            uts46.to_unicode(value.as_bytes(), AsciiDenyList::STD3, Hyphens::Check);
        if result.is_err() || unicode.parse::<PubDress>().is_err() {
            return Err(PubDressLabelError::InvalidCharacter);
        }

        Ok(Self { value })
    }

    /// Returns the canonical lowercase ASCII DNS A-label.
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
    /// ASCII source contains a scalar forbidden by the DNS projection.
    InvalidCharacter,
    /// A non-ASCII scalar is outside the Unicode human-name policy.
    DisallowedScalar,
    /// UTS-46 rejected the label under its bidirectional-text rules.
    BidiRule,
    /// UTS-46 rejected an otherwise eligible source.
    NotEncodable,
    /// The complete source or A-label begins or ends with `-`.
    BoundaryHyphen,
    /// The complete DNS A-label exceeds 63 octets.
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
            Self::InvalidCharacter => "invalid_character",
            Self::DisallowedScalar => "disallowed_scalar",
            Self::BidiRule => "bidi_rule",
            Self::NotEncodable => "not_encodable",
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

fn validate_source_scalars(pub_dress: &PubDress) -> Result<(), PubDressLabelError> {
    for scalar in pub_dress.slug().chars() {
        if scalar.is_ascii() {
            if scalar.is_ascii_alphanumeric() || scalar == '-' {
                continue;
            }
            return Err(PubDressLabelError::InvalidCharacter);
        }
        if !is_unicode_name_scalar(scalar) {
            return Err(PubDressLabelError::DisallowedScalar);
        }
    }

    if pub_dress.as_str().ends_with('-') {
        return Err(PubDressLabelError::BoundaryHyphen);
    }

    Ok(())
}

fn encode_source(source: &str) -> Result<String, PubDressLabelError> {
    if source.starts_with('-') || source.ends_with('-') {
        return Err(PubDressLabelError::BoundaryHyphen);
    }

    let uts46 = Uts46::new();
    let mapped = uts46
        .to_ascii(
            source.as_bytes(),
            AsciiDenyList::STD3,
            Hyphens::Check,
            DnsLength::Ignore,
        )
        .map_err(|_| classify_uts46_error(source))?;

    if mapped.len() > PUB_DRESS_LABEL_MAX_OCTETS {
        return Err(PubDressLabelError::TooLong);
    }

    uts46
        .to_ascii(
            source.as_bytes(),
            AsciiDenyList::STD3,
            Hyphens::Check,
            DnsLength::Verify,
        )
        .map_err(|_| classify_uts46_error(source))?;

    let value = mapped.into_owned();
    validate_a_label_text(&value)?;
    Ok(value)
}

fn classify_uts46_error(source: &str) -> PubDressLabelError {
    if source.chars().any(|scalar| {
        matches!(
            bidi_class(scalar),
            BidiClass::R | BidiClass::AL | BidiClass::AN
        )
    }) {
        PubDressLabelError::BidiRule
    } else {
        PubDressLabelError::NotEncodable
    }
}

fn validate_a_label_text(value: &str) -> Result<(), PubDressLabelError> {
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

fn contains_disallowed_unicode_scalar(value: &str) -> bool {
    let Some(body) = value.strip_prefix(PREFIX) else {
        return false;
    };
    let mut scalars = body.chars();
    let Some(discriminator) = scalars.next() else {
        return false;
    };
    if !matches!(discriminator, '0'..='9' | 'a'..='f') {
        return false;
    }
    let slug = scalars.as_str();
    let scalar_count = slug.chars().count();
    if !(2..=32).contains(&scalar_count) {
        return false;
    }

    slug.chars()
        .any(|scalar| !scalar.is_ascii() && !is_unicode_name_scalar(scalar))
}

#[cfg(test)]
mod tests {
    use super::{
        PUB_DRESS_LABEL_MAX_OCTETS, PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH, PUB_DRESS_UNICODE_VERSION,
        PUB_DRESS_UTS46_IMPLEMENTATION, PubDressLabel, PubDressLabelError,
    };
    use crate::PubDress;

    fn stem(value: &str) -> super::PubDressStem {
        let pub_dress: PubDress = value.parse().expect("test pub_dress must be canonical");
        PubDressLabel::stem(&pub_dress).expect("test pub_dress must have a DNS label")
    }

    #[test]
    fn pins_unicode_and_uts46_contract_versions() {
        assert_eq!(PUB_DRESS_UNICODE_VERSION, "16.0.0");
        assert_eq!(
            PUB_DRESS_UTS46_IMPLEMENTATION,
            "idna=1.1.0;idna_adapter=1.1.0;idna_mapping=1.1.0"
        );
    }

    #[test]
    fn derives_ascii_and_unicode_vectors() {
        let cases = [
            ("0xda-sha", "0xda-sha"),
            ("0xdA-Sha", "0xda-sha"),
            ("0x0Sky", "0x0sky"),
            ("0x0sky", "0x0sky"),
            ("0x0небо", "xn--0x0-dddt1cj"),
            ("0x0Небо", "xn--0x0-dddt1cj"),
            ("0xdпривіт", "xn--0xd-hdd3a5bhs3p"),
            ("0x0café", "xn--0x0caf-gva"),
            ("0x0日本", "xn--0x0-v08fl0d"),
            ("0x0straße", "xn--0x0strae-wya"),
        ];

        for (input, expected) in cases {
            let pub_dress: PubDress = input.parse().expect("canonical test vector");
            let actual = PubDressLabel::stem(&pub_dress).expect("representable test vector");
            assert_eq!(actual.as_str(), expected, "wrong label for {input}");
        }
    }

    #[test]
    fn keeps_distinct_identities_while_exposing_dns_collisions() {
        let unicode_upper: PubDress = "0x0Небо".parse().expect("canonical pub_dress");
        let unicode_lower: PubDress = "0x0небо".parse().expect("canonical pub_dress");
        let ascii_upper: PubDress = "0x0Sky".parse().expect("canonical pub_dress");
        let ascii_lower: PubDress = "0x0sky".parse().expect("canonical pub_dress");

        assert_ne!(unicode_upper, unicode_lower);
        assert_ne!(ascii_upper, ascii_lower);
        assert_eq!(
            PubDressLabel::stem(&unicode_upper)
                .expect("representable")
                .as_str(),
            PubDressLabel::stem(&unicode_lower)
                .expect("representable")
                .as_str()
        );
        assert_eq!(
            PubDressLabel::stem(&ascii_upper)
                .expect("representable")
                .as_str(),
            PubDressLabel::stem(&ascii_lower)
                .expect("representable")
                .as_str()
        );
    }

    #[test]
    fn never_rewrites_the_pub_dress_source() {
        let pub_dress: PubDress = "0x0Небо".parse().expect("canonical pub_dress");
        let stem = PubDressLabel::stem(&pub_dress).expect("representable");
        assert_eq!(pub_dress.as_str(), "0x0Небо");
        assert_eq!(stem.source(), "0x0Небо");
        assert!(stem.was_folded());
    }

    #[test]
    fn classifies_disallowed_and_bidi_sources() {
        assert_eq!(
            PubDressLabel::stem_from_str("0x0🌍"),
            Err(PubDressLabelError::DisallowedScalar)
        );

        let symbols: PubDress = "0x0₴€".parse().expect("canonical identity symbols");
        assert_eq!(
            PubDressLabel::stem(&symbols),
            Err(PubDressLabelError::DisallowedScalar)
        );

        for value in ["0x0א", "0x0ء"] {
            assert_eq!(
                PubDressLabel::stem_from_str(value),
                Err(PubDressLabelError::BidiRule),
                "wrong classification for {value}"
            );
        }
    }

    #[test]
    fn raw_boundary_keeps_identity_failures_distinct() {
        for value in ["sky", "0xgsky", "0xDsky", "0x0a b"] {
            assert_eq!(
                PubDressLabel::stem_from_str(value),
                Err(PubDressLabelError::NotAPubDress),
                "misclassified {value:?}"
            );
        }
    }

    #[test]
    fn rejects_ascii_source_that_dns_cannot_carry() {
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
    fn composes_suffix_before_encoding() {
        let unicode = stem("0x0небо");
        let ascii = stem("0xda-sha");

        assert_eq!(
            PubDressLabel::compose(&unicode, "42")
                .expect("valid unicode label")
                .as_str(),
            PubDressLabel::stem_from_str("0x0небо42")
                .expect("same complete source must encode identically")
                .as_str()
        );
        assert_eq!(
            PubDressLabel::compose(&ascii, "7412")
                .expect("valid label")
                .as_str(),
            "0xda-sha7412"
        );
        assert_eq!(
            PubDressLabel::compose(&ascii, "TWO"),
            Err(PubDressLabelError::InvalidCharacter)
        );
        assert_eq!(
            PubDressLabel::compose(&ascii, "2-"),
            Err(PubDressLabelError::InvalidCharacter)
        );
        assert_eq!(
            PubDressLabel::compose(&ascii, "111111111"),
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
    fn parses_ascii_and_ace_labels_without_reverse_identity_authority() {
        let ascii = PubDressLabel::parse("0x0SKY42").expect("valid Bond label");
        let unicode = PubDressLabel::parse("XN--0X0-DDDT1CJ").expect("valid Bond A-label");

        assert_eq!(ascii.as_str(), "0x0sky42");
        assert_eq!(unicode.as_str(), "xn--0x0-dddt1cj");
        assert_eq!(
            PubDressLabel::parse("www"),
            Err(PubDressLabelError::InvalidCharacter)
        );
    }

    #[test]
    fn rejects_non_ascii_boundary_and_overlong_dns_labels() {
        assert_eq!(
            PubDressLabel::parse("0x0небо"),
            Err(PubDressLabelError::InvalidCharacter)
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
            (PubDressLabelError::InvalidCharacter, "invalid_character"),
            (PubDressLabelError::DisallowedScalar, "disallowed_scalar"),
            (PubDressLabelError::BidiRule, "bidi_rule"),
            (PubDressLabelError::NotEncodable, "not_encodable"),
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
