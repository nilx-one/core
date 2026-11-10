// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Attested finds, their sale and paid crafts (`docs/find-attestation.md`).
//!
//! Three routes, all `POST`, all carrying CSRF, a signed-in active Bond and a
//! rate limit. No request carries an amount: the service credits and debits
//! Core's prices. Seeds in an answer are decimal strings so no client rounds a
//! large balance through a float.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

pub use crate::value::{InvalidValue, Seeds};

/// `POST`: report a pick-up and get its attestation.
pub const ATTEST_PATH: &str = "/api/v1/identity/finds/attest";
/// `POST`: sell an attested find.
pub const SELL_PATH: &str = "/api/v1/identity/finds/sell";
/// `POST`: pay for and start a craft.
pub const CRAFT_PATH: &str = "/api/v1/identity/crafts";

/// The longest `artifact_id` a pick-up may name.
pub const ARTIFACT_ID_MAX_LEN: usize = 96;
/// An attestation id is a hyphenated UUID.
pub const ATTESTATION_ID_LEN: usize = 36;
/// The length of a client-chosen craft id.
pub const CRAFT_ID_LEN: RangeInclusive<usize> = 8..=64;
/// The most attested finds one craft may use up.
pub const CRAFT_INPUTS_MAX: usize = 8;

const LONGITUDE_LIMIT_DEGREES: f64 = 180.0;
const LATITUDE_LIMIT_DEGREES: f64 = 90.0;
const ITEM_ID_MAX_LEN: usize = 64;

/// Whose hands picked the find up.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Finder {
    /// The person.
    Bond,
    /// Their Avaia.
    Avaia,
}

impl Finder {
    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bond => "bond",
            Self::Avaia => "avaia",
        }
    }

    /// Parses the wire spelling.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bond" => Some(Self::Bond),
            "avaia" => Some(Self::Avaia),
            _ => None,
        }
    }
}

/// The id of an attestation: a lowercase hyphenated version-4 UUID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AttestationId(String);

impl AttestationId {
    /// The id as written on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AttestationId {
    type Error = InvalidValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();
        let shaped = bytes.len() == ATTESTATION_ID_LEN
            && bytes.iter().enumerate().all(|(index, byte)| match index {
                8 | 13 | 18 | 23 => *byte == b'-',
                14 => *byte == b'4',
                19 => matches!(byte, b'8' | b'9' | b'a' | b'b'),
                _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
            });
        if shaped {
            Ok(Self(value))
        } else {
            Err(InvalidValue("not a lowercase version-4 UUID"))
        }
    }
}

impl From<AttestationId> for String {
    fn from(id: AttestationId) -> Self {
        id.0
    }
}

/// The id of the thing a find is: 1 to 64 characters of `a-z`, `0-9` and `_`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ItemId(String);

impl ItemId {
    /// The id as written on the wire.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ItemId {
    type Error = InvalidValue;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let shaped = (1..=ITEM_ID_MAX_LEN).contains(&value.len())
            && value
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'));
        if shaped {
            Ok(Self(value))
        } else {
            Err(InvalidValue("not an item id"))
        }
    }
}

impl From<ItemId> for String {
    fn from(id: ItemId) -> Self {
        id.0
    }
}

refusals! {
    /// Why a pick-up was not attested.
    AttestRefusal {
        Taken => ("taken", 409),
        UnknownFind => ("unknown_find", 422),
        OutOfTime => ("out_of_time", 422),
        TooFar => ("too_far", 422),
        NotTheAvaias => ("not_the_avaias", 422),
        InvalidPickup => ("invalid_pickup", 422),
    }
}

refusals! {
    /// Why an attested find was not sold.
    SaleRefusal {
        UnknownAttestation => ("unknown_attestation", 404),
        NotForSale => ("not_for_sale", 409),
        UsedUp => ("used_up", 409),
    }
}

refusals! {
    /// Why a craft was not started. Nothing is debited on a refusal. A craft
    /// from a client that is not the active one answers
    /// [`SESSION_INACTIVE`](crate::error::SESSION_INACTIVE) instead.
    CraftRefusal {
        InvalidCraft => ("invalid_craft", 422),
        UnknownRecipe => ("unknown_recipe", 404),
        NotEnoughSeeds => ("not_enough_seeds", 409),
        WrongInputs => ("wrong_inputs", 409),
        UnverifiableInputs => ("unverifiable_inputs", 409),
        ConflictingReplay => ("conflicting_replay", 409),
    }
}

/// `POST` [`ATTEST_PATH`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestRequest {
    /// The find being picked up.
    pub artifact_id: String,
    /// Whose hands: [`Finder::as_str`].
    pub finder: String,
    /// When the pick-up happened, in milliseconds since the epoch.
    pub found_at_ms: i64,
    /// Degrees east, within ±180.
    pub longitude: f64,
    /// Degrees north, within ±90.
    pub latitude: f64,
}

/// A pick-up whose shape is valid. Whether it is a real find, in time and near
/// enough is for the service to verify.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidPickup {
    /// The find being picked up.
    pub artifact_id: String,
    /// Whose hands.
    pub finder: Finder,
    /// When, in milliseconds since the epoch.
    pub found_at_ms: i64,
    /// Degrees east × 10⁷.
    pub longitude_e7: i32,
    /// Degrees north × 10⁷.
    pub latitude_e7: i32,
}

impl AttestRequest {
    /// A request for `finder`.
    #[must_use]
    pub fn new(
        artifact_id: impl Into<String>,
        finder: Finder,
        found_at_ms: i64,
        longitude: f64,
        latitude: f64,
    ) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            finder: finder.as_str().to_owned(),
            found_at_ms,
            longitude,
            latitude,
        }
    }

    /// Checks the shape: a known finder, a place on Earth and a find id of a
    /// sane length. A client may call this before it sends; the service does.
    ///
    /// # Errors
    ///
    /// [`AttestRefusal::InvalidPickup`] when any of those fails.
    pub fn validate(&self) -> Result<ValidPickup, AttestRefusal> {
        let (Some(finder), Some(longitude_e7), Some(latitude_e7)) = (
            Finder::parse(&self.finder),
            degrees_e7(self.longitude, LONGITUDE_LIMIT_DEGREES),
            degrees_e7(self.latitude, LATITUDE_LIMIT_DEGREES),
        ) else {
            return Err(AttestRefusal::InvalidPickup);
        };
        if self.artifact_id.len() > ARTIFACT_ID_MAX_LEN {
            return Err(AttestRefusal::InvalidPickup);
        }
        Ok(ValidPickup {
            artifact_id: self.artifact_id.clone(),
            finder,
            found_at_ms: self.found_at_ms,
            longitude_e7,
            latitude_e7,
        })
    }
}

fn degrees_e7(value: f64, limit: f64) -> Option<i32> {
    if !value.is_finite() || value.abs() > limit {
        return None;
    }
    // The limit keeps the scaled value inside `i32`.
    #[allow(clippy::cast_possible_truncation)]
    Some((value * 1e7).round() as i32)
}

/// What an attestation request came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttestOutcome {
    /// The pick-up is attested now.
    Attested,
    /// The same Bond attested it before: nothing is credited again.
    AlreadyYours,
}

/// The answer to [`AttestRequest`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestAnswer {
    /// Attested now or before.
    pub outcome: AttestOutcome,
    /// The id every later request names.
    pub attestation_id: AttestationId,
    /// What was found.
    pub item_id: ItemId,
    /// What the pick-up credited; zero when it was attested before.
    pub seeds_credited: Seeds,
}

/// `POST` [`SELL_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SellRequest {
    /// The attestation of the find being sold.
    pub attestation_id: String,
}

impl SellRequest {
    /// A request to sell `attestation_id`.
    #[must_use]
    pub fn new(attestation_id: &AttestationId) -> Self {
        Self {
            attestation_id: attestation_id.as_str().to_owned(),
        }
    }

    /// Checks the id has the length of an attestation id.
    ///
    /// # Errors
    ///
    /// [`SaleRefusal::UnknownAttestation`]: an id of another length names no
    /// attestation.
    pub fn validate(&self) -> Result<(), SaleRefusal> {
        if self.attestation_id.len() == ATTESTATION_ID_LEN {
            Ok(())
        } else {
            Err(SaleRefusal::UnknownAttestation)
        }
    }
}

/// What a sale came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaleOutcome {
    /// Sold now.
    Sold,
    /// Sold before: the same credit, nothing again.
    AlreadySold,
}

/// The answer to [`SellRequest`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SellAnswer {
    /// Sold now or before.
    pub outcome: SaleOutcome,
    /// What the sale credited.
    pub seeds_credited: Seeds,
}

/// `POST` [`CRAFT_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CraftRequest {
    /// A client-chosen id: a replay of the same craft is not charged twice.
    pub craft_id: String,
    /// Core's recipe.
    pub recipe: String,
    /// The attested finds the recipe uses up.
    #[serde(default)]
    pub inputs: Vec<String>,
}

impl CraftRequest {
    /// A request to craft `recipe` from `inputs`.
    #[must_use]
    pub fn new(
        craft_id: impl Into<String>,
        recipe: impl Into<String>,
        inputs: &[AttestationId],
    ) -> Self {
        Self {
            craft_id: craft_id.into(),
            recipe: recipe.into(),
            inputs: inputs.iter().map(|id| id.as_str().to_owned()).collect(),
        }
    }

    /// Checks the craft id and the number of inputs.
    ///
    /// # Errors
    ///
    /// [`CraftRefusal::InvalidCraft`] when the id is not 8 to 64 characters of
    /// letters, digits and `-`, or there are more than eight inputs.
    pub fn validate(&self) -> Result<(), CraftRefusal> {
        if CRAFT_ID_LEN.contains(&self.craft_id.len())
            && self
                .craft_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && self.inputs.len() <= CRAFT_INPUTS_MAX
        {
            Ok(())
        } else {
            Err(CraftRefusal::InvalidCraft)
        }
    }
}

/// What a craft came to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftStart {
    /// Paid and started now.
    Started,
    /// Started before: the same debit, nothing again.
    AlreadyStarted,
}

/// The answer to [`CraftRequest`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CraftAnswer {
    /// Started now or before.
    pub outcome: CraftStart,
    /// What the craft debited.
    pub seeds_debited: Seeds,
}

#[cfg(test)]
mod tests;
