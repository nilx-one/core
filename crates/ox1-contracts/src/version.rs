// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Failure returned for a non-canonical Core contract version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionError;

impl fmt::Display for VersionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("contract version is not canonical")
    }
}

impl std::error::Error for VersionError {}

/// Canonical `MAJOR.MINOR.PATCH` contract version without suffixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl ContractVersion {
    /// Normative Core contract version represented without parsing or ambient state.
    pub const CURRENT: Self = Self {
        major: 0,
        minor: 1,
        patch: 0,
    };

    /// Returns whether `provider` is compatible with this consumer-required version.
    #[must_use]
    pub const fn accepts_provider(self, provider: Self) -> bool {
        if self.major == 0 {
            provider.major == 0 && provider.minor == self.minor && provider.patch >= self.patch
        } else {
            provider.major == self.major && provider.minor >= self.minor
        }
    }
}

fn parse_component(value: &str) -> Result<u64, VersionError> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(VersionError);
    }
    value.parse().map_err(|_| VersionError)
}

impl FromStr for ContractVersion {
    type Err = VersionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut components = value.split('.');
        let major = parse_component(components.next().ok_or(VersionError)?)?;
        let minor = parse_component(components.next().ok_or(VersionError)?)?;
        let patch = parse_component(components.next().ok_or(VersionError)?)?;
        if components.next().is_some() {
            return Err(VersionError);
        }
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for ContractVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Serialize for ContractVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ContractVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{ContractVersion, VersionError};

    #[test]
    fn applies_zero_major_compatibility_line() {
        let required: ContractVersion = "0.1.0".parse().expect("required version");
        let provider: ContractVersion = "0.1.4".parse().expect("provider version");
        let incompatible: ContractVersion = "0.2.0".parse().expect("incompatible version");
        assert!(required.accepts_provider(provider));
        assert!(!required.accepts_provider(incompatible));
    }

    #[test]
    fn current_constant_matches_wire_value() {
        assert_eq!(ContractVersion::CURRENT.to_string(), "0.1.0");
    }

    #[test]
    fn rejects_suffixes_and_leading_zeroes() {
        assert_eq!("0.01.0".parse::<ContractVersion>(), Err(VersionError));
        assert_eq!("0.1.0-beta".parse::<ContractVersion>(), Err(VersionError));
    }
}
