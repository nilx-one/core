// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::{fmt, str::FromStr};

const PREFIX: &str = "0x";
const MIN_SLUG_SCALARS: usize = 2;
const MAX_SLUG_SCALARS: usize = 32;

/// Canonical human-readable 0x1 public address.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PubDress {
    value: String,
    discriminator: char,
    slug: String,
}

impl PubDress {
    /// Returns the exact canonical representation without normalization.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the person-selected lowercase hexadecimal discriminator.
    #[must_use]
    pub fn discriminator(&self) -> char {
        self.discriminator
    }

    /// Returns the exact case-sensitive slug.
    #[must_use]
    pub fn slug(&self) -> &str {
        &self.slug
    }
}

impl TryFrom<String> for PubDress {
    type Error = PubDressError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let (discriminator, slug) = validate(&value)?;
        Ok(Self {
            slug: slug.to_owned(),
            value,
            discriminator,
        })
    }
}

impl FromStr for PubDress {
    type Err = PubDressError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for PubDress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

/// Stable failure classification for canonical `pub_dress` validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PubDressError {
    /// The literal `0x` prefix is absent.
    InvalidPrefix,
    /// The discriminator is not one lowercase hexadecimal digit.
    InvalidDiscriminator,
    /// The slug contains fewer than 2 or more than 32 Unicode scalar values.
    InvalidLength,
    /// A slug scalar is outside the canonical allowlist.
    InvalidCharacter,
}

impl PubDressError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidPrefix => "invalid_prefix",
            Self::InvalidDiscriminator => "invalid_discriminator",
            Self::InvalidLength => "invalid_length",
            Self::InvalidCharacter => "invalid_character",
        }
    }
}

impl fmt::Display for PubDressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for PubDressError {}

fn validate(value: &str) -> Result<(char, &str), PubDressError> {
    let body = value
        .strip_prefix(PREFIX)
        .ok_or(PubDressError::InvalidPrefix)?;
    let mut scalars = body.chars();
    let discriminator = scalars.next().ok_or(PubDressError::InvalidDiscriminator)?;
    if !matches!(discriminator, '0'..='9' | 'a'..='f') {
        return Err(PubDressError::InvalidDiscriminator);
    }
    let slug = scalars.as_str();
    let scalar_count = slug.chars().count();

    if !(MIN_SLUG_SCALARS..=MAX_SLUG_SCALARS).contains(&scalar_count) {
        return Err(PubDressError::InvalidLength);
    }

    if !slug.chars().all(is_allowed_slug_scalar) {
        return Err(PubDressError::InvalidCharacter);
    }

    Ok((discriminator, slug))
}

fn is_allowed_slug_scalar(value: char) -> bool {
    value.is_ascii_alphabetic()
        || value.is_ascii_digit()
        || matches!(
            value,
            '-' | '/'
                | ':'
                | ';'
                | '('
                | ')'
                | '₴'
                | '&'
                | '@'
                | '"'
                | '.'
                | ','
                | '?'
                | '!'
                | '\''
                | '['
                | ']'
                | '{'
                | '}'
                | '#'
                | '%'
                | '^'
                | '*'
                | '+'
                | '='
                | '_'
                | '\\'
                | '|'
                | '~'
                | '<'
                | '>'
                | '€'
                | '$'
                | '£'
                | '•'
        )
}

#[cfg(test)]
mod tests {
    use super::{PubDress, PubDressError};

    #[test]
    fn preserves_every_canonical_scalar_exactly() {
        let values = [
            "0x0sky",
            "0x0Sky",
            "0xaaB",
            "0xfa/b?c#d%20",
            "0x0₴€$£•",
            "0xf-/:;()&@\".,?!'[]{}#%^*+=_\\|~<>",
        ];

        for value in values {
            let parsed: PubDress = value.parse().expect("canonical pub_dress must parse");
            assert_eq!(parsed.as_str(), value);
        }
    }

    #[test]
    fn classifies_prefix_length_and_character_failures() {
        assert_eq!(
            "1xsky".parse::<PubDress>(),
            Err(PubDressError::InvalidPrefix)
        );
        assert_eq!(
            "0xgsky".parse::<PubDress>(),
            Err(PubDressError::InvalidDiscriminator)
        );
        assert_eq!(
            "0xaa".parse::<PubDress>(),
            Err(PubDressError::InvalidLength)
        );
        assert_eq!(
            "0x0a b".parse::<PubDress>(),
            Err(PubDressError::InvalidCharacter)
        );
    }

    #[test]
    fn rejects_values_that_would_require_rewriting() {
        for value in [
            "0x0привіт",
            "0x0a🙂",
            "0x0a\u{0301}",
            "0x0‘a",
            " 0xsky",
            "0x0sky ",
        ] {
            assert!(value.parse::<PubDress>().is_err(), "accepted {value:?}");
        }
    }

    #[test]
    fn exposes_stable_binding_codes() {
        assert_eq!(PubDressError::InvalidPrefix.code(), "invalid_prefix");
        assert_eq!(
            PubDressError::InvalidDiscriminator.code(),
            "invalid_discriminator"
        );
        assert_eq!(PubDressError::InvalidLength.code(), "invalid_length");
        assert_eq!(PubDressError::InvalidCharacter.code(), "invalid_character");
    }

    #[test]
    fn keeps_discriminator_and_case_sensitive_slug_separate() {
        let lower: PubDress = "0x0sky".parse().expect("valid pub_dress");
        let upper: PubDress = "0x0Sky".parse().expect("valid pub_dress");

        assert_eq!(lower.discriminator(), '0');
        assert_eq!(lower.slug(), "sky");
        assert_ne!(lower, upper);
    }
}
