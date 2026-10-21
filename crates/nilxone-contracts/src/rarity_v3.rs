// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Proposed five-tier rarity economy; inert until the v3 find/claim migration.
//!
//! This is deliberately a different type from legacy `find_item::Rarity`.
//! Numeric tiers 1–5 must not be interpreted without the find-catalog version:
//! legacy tiers 1–3 all meant Common and tier 6 meant Legendary.
//!
//! Canonical product direction:
//! https://github.com/nilx-one/0x1/blob/master/documents/product-direction/five-tier-rarity-economy.md

use core::num::NonZeroU64;

/// Target rarity scale version; not a live find catalog/roll-table version.
pub const RARITY_V3_VERSION: u32 = 3;

/// Exactly one rarity per target tier, commonest first.
///
/// No `Legendary` exists in this version. Do not map legacy catalog values
/// to this enum without the original catalog version and an explicit migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum RarityV3 {
    /// Tier 1: public, base value.
    Common = 1,
    /// Tier 2: public.
    Uncommon = 2,
    /// Tier 3: public.
    Rare = 3,
    /// Tier 4: requires effective Special or Exclusive subscription.
    Special = 4,
    /// Tier 5: requires effective Exclusive subscription.
    Exclusive = 5,
}

impl RarityV3 {
    /// All current target rarities; there are no Common subtiers.
    pub const ALL: [Self; 5] = [
        Self::Common,
        Self::Uncommon,
        Self::Rare,
        Self::Special,
        Self::Exclusive,
    ];

    /// Reads a *v3* tier index, not a legacy find tier.
    #[must_use]
    pub const fn from_v3_tier(tier: u8) -> Option<Self> {
        match tier {
            1 => Some(Self::Common),
            2 => Some(Self::Uncommon),
            3 => Some(Self::Rare),
            4 => Some(Self::Special),
            5 => Some(Self::Exclusive),
            _ => None,
        }
    }

    /// Version-specific numeric tier, 1 through 5.
    #[must_use]
    pub const fn tier(self) -> u8 {
        self as u8
    }

    /// Binding-safe target code. Legacy `legendary` is never accepted.
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

    /// Parses a target code without silently promoting legacy rarity.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rarity| rarity.code() == code)
    }

    /// Exact versioned base pickup reward. The target is ~×3 between tiers;
    /// 810 is intentionally rounded to 800 at Exclusive.
    ///
    /// A server-owned entitlement multiplier is a *separate* award operation,
    /// applied once to this exact base, without further "nice" rounding.
    #[must_use]
    pub const fn base_find_xp(self) -> u32 {
        match self {
            Self::Common => 10,
            Self::Uncommon => 30,
            Self::Rare => 90,
            Self::Special => 270,
            Self::Exclusive => 800,
        }
    }

    /// Exact tier-relative price multiplier, independent of individual item ID.
    #[must_use]
    pub const fn price_multiplier(self) -> u64 {
        match self {
            Self::Common => 1,
            Self::Uncommon => 2,
            Self::Rare => 4,
            Self::Special => 8,
            Self::Exclusive => 16,
        }
    }

    /// Sale price for a rarity-priced found item at an approved common base.
    ///
    /// The item catalog never needs an individual "celebrity/title" sale price.
    /// The base value is *not chosen here*. Overflow must not wrap or saturate.
    #[must_use]
    pub fn find_sale_price(self, common_base_seeds: NonZeroU64) -> Option<u64> {
        common_base_seeds.get().checked_mul(self.price_multiplier())
    }
}

#[cfg(test)]
mod tests {
    use core::num::NonZeroU64;

    use super::RarityV3;

    #[test]
    fn one_distinct_rarity_per_tier_and_no_legacy_legendary() {
        for (position, rarity) in RarityV3::ALL.into_iter().enumerate() {
            assert_eq!(rarity.tier(), (position + 1) as u8);
            assert_eq!(RarityV3::from_v3_tier(rarity.tier()), Some(rarity));
            assert_ne!(rarity.code(), "legendary");
        }
        assert_eq!(RarityV3::from_v3_tier(0), None);
        assert_eq!(RarityV3::from_v3_tier(6), None);
    }

    #[test]
    fn exact_prices_double_per_rarity_without_titles() {
        let common_base = NonZeroU64::new(50).expect("positive base");
        let prices: Vec<_> = RarityV3::ALL
            .into_iter()
            .map(|rarity| rarity.find_sale_price(common_base))
            .collect();
        assert_eq!(prices, [Some(50), Some(100), Some(200), Some(400), Some(800)]);
        assert_eq!(RarityV3::Special.price_multiplier(), 8);
        assert_eq!(RarityV3::Exclusive.price_multiplier(), 16);
        assert_eq!(RarityV3::Exclusive.find_sale_price(NonZeroU64::MAX), None);
    }

    #[test]
    fn xp_is_rounded_to_a_friendly_800_at_the_top() {
        let xp: Vec<_> = RarityV3::ALL
            .into_iter()
            .map(RarityV3::base_find_xp)
            .collect();
        assert_eq!(xp, [10, 30, 90, 270, 800]);

        let rounded = |nominal: u32| {
            let step = if nominal < 500 { 10 } else { 100 };
            ((nominal + step / 2) / step) * step
        };
        for (n, rarity) in RarityV3::ALL.into_iter().enumerate() {
            let nominal = 10 * 3_u32.pow(n as u32);
            assert_eq!(rarity.base_find_xp(), rounded(nominal));
        }
        assert_eq!(rounded(810), 800);
    }
}
