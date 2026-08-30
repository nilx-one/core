// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use core::fmt;
use serde::Serialize;
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

/// Failure while producing canonical contract JSON bytes.
#[derive(Debug)]
pub enum CanonicalJsonError {
    Serialization(serde_json::Error),
    NumberToken,
    NonNfcString,
    NonAsciiObjectKey,
}

impl fmt::Display for CanonicalJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(error) => write!(formatter, "JSON serialization failed: {error}"),
            Self::NumberToken => formatter.write_str("JSON numeric tokens are forbidden"),
            Self::NonNfcString => formatter.write_str("JSON strings must be NFC"),
            Self::NonAsciiObjectKey => {
                formatter.write_str("Core contract object keys must be canonical ASCII")
            }
        }
    }
}

impl std::error::Error for CanonicalJsonError {}

impl From<serde_json::Error> for CanonicalJsonError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

fn is_nfc(value: &str) -> bool {
    value.nfc().eq(value.chars())
}

fn validate(value: &Value) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null | Value::Bool(_) => Ok(()),
        Value::Number(_) => Err(CanonicalJsonError::NumberToken),
        Value::String(string) => {
            if is_nfc(string) {
                Ok(())
            } else {
                Err(CanonicalJsonError::NonNfcString)
            }
        }
        Value::Array(values) => values.iter().try_for_each(validate),
        Value::Object(object) => object.iter().try_for_each(|(key, nested)| {
            if !key.is_ascii() {
                return Err(CanonicalJsonError::NonAsciiObjectKey);
            }
            validate(nested)
        }),
    }
}

/// Serializes a typed contract value into deterministic canonical JSON bytes.
///
/// Contract `0.1.0` forbids JSON numbers and defines only ASCII object member names.
/// With those constraints, `serde_json`'s ordered map representation yields the same
/// member ordering required by the contract's RFC 8785 profile.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalJsonError> {
    let json = serde_json::to_value(value)?;
    validate(&json)?;
    Ok(serde_json::to_vec(&json)?)
}

#[cfg(test)]
mod tests {
    use super::{CanonicalJsonError, canonical_json};
    use serde::Serialize;

    #[derive(Serialize)]
    struct StringValue<'a> {
        z: &'a str,
        a: &'a str,
    }

    #[test]
    fn orders_closed_ascii_members_deterministically() {
        let encoded = canonical_json(&StringValue { z: "2", a: "1" }).expect("canonical JSON");
        assert_eq!(encoded, br#"{"a":"1","z":"2"}"#);
    }

    #[test]
    fn rejects_json_numbers() {
        let error = canonical_json(&42_u64).expect_err("numbers are forbidden");
        assert!(matches!(error, CanonicalJsonError::NumberToken));
    }
}
