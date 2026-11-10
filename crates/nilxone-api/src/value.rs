// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Values several routes share.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A value that is not shaped as the contract requires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidValue(pub(crate) &'static str);

impl fmt::Display for InvalidValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for InvalidValue {}

/// An amount of seeds, written as a decimal string with no sign, no leading
/// zero and at most 19 digits.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Seeds(pub u64);

impl TryFrom<String> for Seeds {
    type Error = InvalidValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let digits = value.as_bytes();
        let canonical = !digits.is_empty()
            && digits.len() <= 19
            && digits.iter().all(u8::is_ascii_digit)
            && (digits.len() == 1 || digits[0] != b'0');
        if !canonical {
            return Err(InvalidValue(
                "seeds are a canonical decimal string of at most 19 digits",
            ));
        }
        value
            .parse()
            .map(Self)
            .map_err(|_| InvalidValue("seeds do not fit 64 bits"))
    }
}

impl From<Seeds> for String {
    fn from(seeds: Seeds) -> Self {
        seeds.0.to_string()
    }
}
