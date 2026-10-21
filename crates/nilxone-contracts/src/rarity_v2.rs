// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Rarity v2 as a coefficient (0x1 `product-direction/rarity-coefficient.md`).
//!
//! Five rarities, each with a coefficient that doubles per step: 1, 2, 4, 8,
//! 16. The coefficient alone says how often a rarity appears (at least rarity
//! `r` with chance `1 / c(r)`) and what it is worth (a base award times `c`).
//! A roll never exceeds the ceiling it is given, so an orb is never rarer than
//! the landmark it spilled around.
//!
//! Find tiers keep their v1 [`crate::Rarity`]; this type renames nothing.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// How rare an orb or a landmark is, commonest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum RarityV2 {
    #[default]
    Common,
    Uncommon,
    Rare,
    Special,
    Exclusive,
}

impl RarityV2 {
    /// Every rarity, commonest first.
    pub const ALL: [Self; 5] = [
        Self::Common,
        Self::Uncommon,
        Self::Rare,
        Self::Special,
        Self::Exclusive,
    ];

    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Special => "special",
            Self::Exclusive => "exclusive",
        }
    }

    /// Parses a binding-safe code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rarity| rarity.code() == code)
    }

    /// Steps above Common: 0 to 4.
    #[must_use]
    pub const fn step(self) -> u32 {
        match self {
            Self::Common => 0,
            Self::Uncommon => 1,
            Self::Rare => 2,
            Self::Special => 3,
            Self::Exclusive => 4,
        }
    }

    /// The coefficient, `2^step`: 1, 2, 4, 8 or 16.
    #[must_use]
    pub const fn coefficient(self) -> u32 {
        1 << self.step()
    }

    /// The rarity a uniform 32-bit `draw` rolls under `ceiling`: at least
    /// rarity `r` with chance `1 / c(r)`, never above the ceiling.
    #[must_use]
    pub fn roll(draw: u32, ceiling: Self) -> Self {
        let step = draw.leading_zeros().min(ceiling.step());
        Self::ALL
            .into_iter()
            .find(|rarity| rarity.step() == step)
            .unwrap_or(Self::Common)
    }
}

impl Serialize for RarityV2 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for RarityV2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = String::deserialize(deserializer)?;
        Self::from_code(&code).ok_or_else(|| D::Error::custom("not a rarity code"))
    }
}

#[cfg(test)]
mod tests {
    use super::RarityV2;

    #[test]
    fn each_step_doubles_the_coefficient() {
        let coefficients: Vec<u32> = RarityV2::ALL.iter().map(|r| r.coefficient()).collect();
        assert_eq!(coefficients, [1, 2, 4, 8, 16]);
        for rarity in RarityV2::ALL {
            assert_eq!(RarityV2::from_code(rarity.code()), Some(rarity));
        }
        assert_eq!(RarityV2::from_code("legendary"), None);
        assert_eq!(RarityV2::default(), RarityV2::Common);
    }

    #[test]
    fn a_roll_is_at_least_r_with_chance_one_in_its_coefficient() {
        // Every leading-zero count is equally represented by stepping the
        // draw's high bits through all 2^16 values.
        let draws: Vec<u32> = (0..=u16::MAX)
            .map(|high| u32::from(high) << 16 | 0x1234)
            .collect();
        let total = u32::try_from(draws.len()).expect("count");
        for rarity in RarityV2::ALL {
            let at_least = draws
                .iter()
                .filter(|draw| RarityV2::roll(**draw, RarityV2::Exclusive) >= rarity)
                .count();
            assert_eq!(
                u32::try_from(at_least).expect("count"),
                total / rarity.coefficient()
            );
        }
    }

    #[test]
    fn a_roll_never_exceeds_its_ceiling() {
        assert_eq!(RarityV2::roll(0, RarityV2::Exclusive), RarityV2::Exclusive);
        assert_eq!(RarityV2::roll(0, RarityV2::Rare), RarityV2::Rare);
        assert_eq!(RarityV2::roll(0, RarityV2::Common), RarityV2::Common);
        assert_eq!(
            RarityV2::roll(u32::MAX, RarityV2::Exclusive),
            RarityV2::Common
        );
    }

    #[test]
    fn the_wire_is_the_code() {
        assert_eq!(
            serde_json::to_string(&RarityV2::Special).expect("json"),
            r#""special""#
        );
        assert_eq!(
            serde_json::from_str::<RarityV2>(r#""exclusive""#).expect("json"),
            RarityV2::Exclusive
        );
        assert!(serde_json::from_str::<RarityV2>(r#""legendary""#).is_err());
    }
}
