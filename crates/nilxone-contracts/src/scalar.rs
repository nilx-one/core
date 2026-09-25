// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// Error returned when a decimal `u64` string is not canonical or is out of range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecimalU64Error;

impl fmt::Display for DecimalU64Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unsigned integer string is not canonical u64")
    }
}

impl std::error::Error for DecimalU64Error {}

/// Cross-runtime unsigned integer represented as a canonical decimal JSON string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DecimalU64(u64);

impl DecimalU64 {
    /// Creates a value from an internal `u64`.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the internal numeric value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl FromStr for DecimalU64 {
    type Err = DecimalU64Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(DecimalU64Error);
        }
        value.parse::<u64>().map(Self).map_err(|_| DecimalU64Error)
    }
}

impl fmt::Display for DecimalU64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl Serialize for DecimalU64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for DecimalU64 {
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
    use super::{DecimalU64, DecimalU64Error};

    #[test]
    fn rejects_leading_zeroes() {
        assert_eq!("01".parse::<DecimalU64>(), Err(DecimalU64Error));
    }

    #[test]
    fn serializes_as_json_string() {
        let encoded = serde_json::to_string(&DecimalU64::new(42)).expect("serialization must work");
        assert_eq!(encoded, "\"42\"");
    }
}
