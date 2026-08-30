// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Failure returned when a binding-safe identifier is not canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentifierError;

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("identifier is not canonical")
    }
}

impl std::error::Error for IdentifierError {}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

macro_rules! identifier_type {
    ($name:ident, $prefix:literal, $hex_length:expr) => {
        #[doc = concat!("Canonical ", stringify!($name), " value.")]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Returns the canonical ASCII representation.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdentifierError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                let Some(rest) = value.strip_prefix($prefix) else {
                    return Err(IdentifierError);
                };
                if !is_lower_hex(rest, $hex_length) {
                    return Err(IdentifierError);
                }
                Ok(Self(value))
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.to_owned().try_into()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.try_into().map_err(D::Error::custom)
            }
        }
    };
}

identifier_type!(BondId, "bond_", 64);
identifier_type!(BondChainId, "bch_", 64);
identifier_type!(OperationId, "op_", 32);
identifier_type!(Sha256Digest, "sha256_", 64);

#[cfg(test)]
mod tests {
    use super::{BondId, IdentifierError};

    #[test]
    fn accepts_canonical_bond_identifier() {
        let value = format!("bond_{}", "a".repeat(64));
        let parsed: BondId = value.parse().expect("canonical Bond id must parse");
        assert_eq!(parsed.as_str(), value);
    }

    #[test]
    fn rejects_uppercase_hex() {
        let value = format!("bond_{}", "A".repeat(64));
        assert_eq!(value.parse::<BondId>(), Err(IdentifierError));
    }
}
