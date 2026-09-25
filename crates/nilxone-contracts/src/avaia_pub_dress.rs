// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::{fmt, str::FromStr};

use crate::pub_dress::{PubDress, is_allowed_slug_scalar};

const MIN_SLUG_SCALARS: usize = 2;
// A valid human slug may contain 32 scalars. Default derivation appends the
// mandatory two-scalar `ai` suffix, so the Avaia grammar must represent 34.
const MAX_SLUG_SCALARS: usize = 34;
const AI_SUFFIX: &str = "ai";

/// Canonical public address for an Avaia owned by a human Bond.
///
/// Unlike a human [`PubDress`], this address has no literal `0x` or `x`
/// prefix. Its first scalar is the owner's immutable hexadecimal
/// discriminator and its slug always ends in `ai`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AvaiaPubDress {
    value: String,
    owner_discriminator: char,
    slug: String,
}

impl AvaiaPubDress {
    /// Derives the canonical default Avaia address for a parsed human Bond.
    ///
    /// The operation is total for every valid [`PubDress`]: both types share
    /// the same scalar allowlist and the Avaia maximum accounts for appending
    /// `ai` to a maximum-length human slug.
    #[must_use]
    pub fn derive_default(owner: &PubDress) -> Self {
        let owner_slug = owner.slug();
        let stem = default_stem(owner_slug);
        let mut slug = String::with_capacity(stem.len() + AI_SUFFIX.len());
        slug.push_str(stem);
        slug.push_str(AI_SUFFIX);

        let mut value = String::with_capacity(1 + slug.len());
        value.push(owner.discriminator());
        value.push_str(&slug);

        Self {
            value,
            owner_discriminator: owner.discriminator(),
            slug,
        }
    }

    /// Returns the exact canonical representation without normalization.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the discriminator inherited from the owning human Bond.
    #[must_use]
    pub fn owner_discriminator(&self) -> char {
        self.owner_discriminator
    }

    /// Returns the exact case-sensitive Avaia slug.
    #[must_use]
    pub fn slug(&self) -> &str {
        &self.slug
    }
}

impl TryFrom<String> for AvaiaPubDress {
    type Error = AvaiaPubDressError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (owner_discriminator, slug) = validate(&value)?;
        Ok(Self {
            slug: slug.to_owned(),
            value,
            owner_discriminator,
        })
    }
}

impl FromStr for AvaiaPubDress {
    type Err = AvaiaPubDressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for AvaiaPubDress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

/// Stable failure classification for canonical owned-Avaia address validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvaiaPubDressError {
    /// The first scalar is not one lowercase hexadecimal discriminator.
    InvalidDiscriminator,
    /// The Avaia slug is outside the canonical 2–34 scalar range.
    InvalidLength,
    /// A slug scalar is outside the canonical public-address allowlist.
    InvalidCharacter,
    /// The Avaia slug does not end in the literal lowercase ASCII `ai` suffix.
    MissingAiSuffix,
}

impl AvaiaPubDressError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidDiscriminator => "invalid_discriminator",
            Self::InvalidLength => "invalid_length",
            Self::InvalidCharacter => "invalid_character",
            Self::MissingAiSuffix => "missing_ai_suffix",
        }
    }
}

impl fmt::Display for AvaiaPubDressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for AvaiaPubDressError {}

fn validate(value: &str) -> Result<(char, &str), AvaiaPubDressError> {
    let mut scalars = value.chars();
    let owner_discriminator = scalars
        .next()
        .ok_or(AvaiaPubDressError::InvalidDiscriminator)?;
    if !matches!(owner_discriminator, '0'..='9' | 'a'..='f') {
        return Err(AvaiaPubDressError::InvalidDiscriminator);
    }

    let slug = scalars.as_str();
    let scalar_count = slug.chars().count();
    if !(MIN_SLUG_SCALARS..=MAX_SLUG_SCALARS).contains(&scalar_count) {
        return Err(AvaiaPubDressError::InvalidLength);
    }
    if !slug.chars().all(is_allowed_slug_scalar) {
        return Err(AvaiaPubDressError::InvalidCharacter);
    }
    if !slug.ends_with(AI_SUFFIX) {
        return Err(AvaiaPubDressError::MissingAiSuffix);
    }

    Ok((owner_discriminator, slug))
}

fn default_stem(slug: &str) -> &str {
    if slug.chars().count() <= 1 {
        return slug;
    }

    let Some((last_index, last)) = slug.char_indices().next_back() else {
        return slug;
    };
    if matches!(last, 'a' | 'e' | 'i' | 'o' | 'u' | 'y') {
        &slug[..last_index]
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::{AvaiaPubDress, AvaiaPubDressError};
    use crate::PubDress;

    #[test]
    fn derives_normative_default_addresses() {
        let cases = [
            ("0x0sky", "0skai"),
            ("0x0mira", "0mirai"),
            ("0x0ze", "0zai"),
            ("0x0sk", "0skai"),
            ("0xda-sha.", "da-sha.ai"),
        ];

        for (owner, expected) in cases {
            let owner: PubDress = owner.parse().expect("valid human pub_dress");
            let derived = AvaiaPubDress::derive_default(&owner);
            assert_eq!(derived.as_str(), expected);
            assert_eq!(derived.owner_discriminator(), owner.discriminator());
        }
    }

    #[test]
    fn derivation_is_total_at_the_human_maximum() {
        let owner_value = format!("0x0{}", "b".repeat(32));
        let owner: PubDress = owner_value.parse().expect("maximum human slug is valid");
        let derived = AvaiaPubDress::derive_default(&owner);

        assert_eq!(derived.slug().chars().count(), 34);
        assert!(derived.slug().ends_with("ai"));
        assert_eq!(derived.as_str().chars().count(), 35);
        assert_eq!(derived.as_str().parse::<AvaiaPubDress>(), Ok(derived));
    }

    #[test]
    fn parses_exact_case_and_punctuation_without_normalization() {
        for value in ["0Skai", "da-sha.ai", "f-/:;()&@\".,?!'[]{}#%^*+=_\\|~<>ai"] {
            let parsed: AvaiaPubDress = value.parse().expect("canonical Avaia address");
            assert_eq!(parsed.as_str(), value);
        }
    }

    #[test]
    fn rejects_old_prefix_missing_suffix_and_invalid_content() {
        assert_eq!(
            "x0skai".parse::<AvaiaPubDress>(),
            Err(AvaiaPubDressError::InvalidDiscriminator)
        );
        assert_eq!(
            "gskai".parse::<AvaiaPubDress>(),
            Err(AvaiaPubDressError::InvalidDiscriminator)
        );
        assert_eq!(
            "0sky".parse::<AvaiaPubDress>(),
            Err(AvaiaPubDressError::MissingAiSuffix)
        );
        assert_eq!(
            "0a i".parse::<AvaiaPubDress>(),
            Err(AvaiaPubDressError::InvalidCharacter)
        );
    }

    #[test]
    fn exposes_stable_binding_codes() {
        assert_eq!(
            AvaiaPubDressError::InvalidDiscriminator.code(),
            "invalid_discriminator"
        );
        assert_eq!(AvaiaPubDressError::InvalidLength.code(), "invalid_length");
        assert_eq!(
            AvaiaPubDressError::InvalidCharacter.code(),
            "invalid_character"
        );
        assert_eq!(
            AvaiaPubDressError::MissingAiSuffix.code(),
            "missing_ai_suffix"
        );
    }
}
