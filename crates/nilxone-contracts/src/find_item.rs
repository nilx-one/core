// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! What a chance find is: the item a rolled find turns out to be.
//!
//! A host rolls a find with its tier (`rollSegment` in the web client, its port
//! in the identity service). This module answers the next question the same
//! way everywhere: which item of the catalog that find is, what picking it up
//! pays, and whether the Bond wants that rarity picked up at all. It is pure
//! arithmetic over the artifact id, so the client that shows the item and the
//! service that prices it can never disagree.

use core::fmt;

/// Version of [`CATALOG`] and of the pick. Any change to an item, a weight or
/// the hash changes which find is which item, so it raises this number.
pub const FIND_CATALOG_VERSION: u32 = 1;

/// Domain prefix hashed in front of the artifact id when picking an item.
const PICK_DOMAIN: &str = "nilx-one.find-item.v1:";

/// A find's tier, `1..=6`, as rolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FindTier(u8);

impl FindTier {
    /// Every tier, commonest first.
    pub const ALL: [Self; 6] = [Self(1), Self(2), Self(3), Self(4), Self(5), Self(6)];

    /// Returns the tier, or `None` outside `1..=6`.
    #[must_use]
    pub const fn new(value: u8) -> Option<Self> {
        if value >= 1 && value <= 6 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Returns the tier number.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The rarity this tier is shown as: tiers 1–3 are common, then one
    /// rarity per tier.
    #[must_use]
    pub const fn rarity(self) -> Rarity {
        match self.0 {
            4 => Rarity::Uncommon,
            5 => Rarity::Rare,
            6 => Rarity::Legendary,
            _ => Rarity::Common,
        }
    }
}

/// How rare a find is, as a person reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rarity {
    /// Tiers 1–3: bottles, cans and the everyday litter of a city.
    Common,
    /// Tier 4.
    Uncommon,
    /// Tier 5.
    Rare,
    /// Tier 6.
    Legendary,
}

impl Rarity {
    /// Every rarity, commonest first.
    pub const ALL: [Self; 4] = [Self::Common, Self::Uncommon, Self::Rare, Self::Legendary];

    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Legendary => "legendary",
        }
    }

    /// Parses a binding-safe code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rarity| rarity.code() == code)
    }

    const fn bit(self) -> u8 {
        match self {
            Self::Common => 1,
            Self::Uncommon => 2,
            Self::Rare => 4,
            Self::Legendary => 8,
        }
    }
}

/// Which rarities the Bond picks up. A find of a rarity left out is still
/// seen, and still pays its sighting; it is only not picked up.
///
/// The setting is the Bond's own choice in Settings: "1–3", "4", "5", "6".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PickupRarities(u8);

impl PickupRarities {
    /// Every rarity is picked up. The default.
    pub const ALL: Self = Self(0b1111);

    /// Nothing is picked up.
    pub const NONE: Self = Self(0);

    /// Returns these rarities with `rarity` included or left out.
    #[must_use]
    pub const fn with(self, rarity: Rarity, picked_up: bool) -> Self {
        if picked_up {
            Self(self.0 | rarity.bit())
        } else {
            Self(self.0 & !rarity.bit())
        }
    }

    /// Whether finds of `rarity` are picked up.
    #[must_use]
    pub const fn includes(self, rarity: Rarity) -> bool {
        self.0 & rarity.bit() != 0
    }

    /// Whether a find of `tier` is picked up.
    #[must_use]
    pub const fn picks_up(self, tier: FindTier) -> bool {
        self.includes(tier.rarity())
    }

    /// The canonical wire form: the included rarity codes, commonest first,
    /// joined by `,`. Nothing included is the empty string.
    #[must_use]
    pub fn to_wire(self) -> String {
        Rarity::ALL
            .into_iter()
            .filter(|rarity| self.includes(*rarity))
            .map(Rarity::code)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parses the wire form. Order does not matter; an unknown or repeated
    /// code is refused rather than ignored.
    ///
    /// # Errors
    ///
    /// Returns [`FindItemError::PickupRarities`] for an unknown or repeated code.
    pub fn from_wire(value: &str) -> Result<Self, FindItemError> {
        let mut rarities = Self::NONE;
        if value.is_empty() {
            return Ok(rarities);
        }
        for code in value.split(',') {
            let rarity = Rarity::from_code(code).ok_or(FindItemError::PickupRarities)?;
            if rarities.includes(rarity) {
                return Err(FindItemError::PickupRarities);
            }
            rarities = rarities.with(rarity, true);
        }
        Ok(rarities)
    }
}

impl Default for PickupRarities {
    fn default() -> Self {
        Self::ALL
    }
}

/// A city with its own share of the catalog. Elsewhere only the items every
/// city shares turn up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum City {
    /// Kyiv. The archive, and the one city with its own items today.
    Kyiv,
    /// Saint Petersburg. Reserved: no items of its own yet.
    SaintPetersburg,
    /// Barcelona. Reserved: no items of its own yet.
    Barcelona,
}

impl City {
    /// Every city, in a fixed order.
    pub const ALL: [Self; 3] = [Self::Kyiv, Self::SaintPetersburg, Self::Barcelona];

    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Kyiv => "kyiv",
            Self::SaintPetersburg => "saint_petersburg",
            Self::Barcelona => "barcelona",
        }
    }

    /// The inclusive segment rows and columns the city covers.
    ///
    /// They are cells of the find segment grid (`segmentAt` in the web
    /// client: rows of `50 / 111195` degrees of latitude from -90, columns
    /// `1.5698` times wider from -180), so a city is decided from the
    /// artifact id alone, with no coordinate and no floating point.
    const fn segment_bounds(self) -> SegmentBounds {
        match self {
            // 50.21..50.59 N, 30.24..30.83 E.
            Self::Kyiv => SegmentBounds {
                rows: (311_813, 312_658),
                columns: (297_842, 298_678),
            },
            // 59.63..60.25 N, 29.43..30.76 E.
            Self::SaintPetersburg => SegmentBounds {
                rows: (332_762, 334_140),
                columns: (296_694, 298_578),
            },
            // 41.32..41.47 N, 2.05..2.23 E.
            Self::Barcelona => SegmentBounds {
                rows: (292_042, 292_376),
                columns: (257_906, 258_161),
            },
        }
    }

    /// The city a find segment lies in, if any.
    #[must_use]
    pub fn of_segment(row: i64, column: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|city| {
            let bounds = city.segment_bounds();
            (bounds.rows.0..=bounds.rows.1).contains(&row)
                && (bounds.columns.0..=bounds.columns.1).contains(&column)
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct SegmentBounds {
    rows: (i64, i64),
    columns: (i64, i64),
}

/// Whether an item works as found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Condition {
    /// It works, or it is not the kind of thing that breaks.
    Intact,
    /// It is broken. Some broken items can be repaired into their intact kind.
    Broken,
}

impl Condition {
    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Intact => "intact",
            Self::Broken => "broken",
        }
    }
}

/// One kind of thing a find can be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindItem {
    /// Stable, binding-safe id. Hosts name it in their own languages.
    pub id: &'static str,
    /// The tier that rolls it: how often it turns up.
    pub tier: FindTier,
    /// What picking it up pays. Deliberately not the tier's: a rare thing can
    /// be worth little on its own.
    pub experience: u32,
    /// In-game money it is worth on the spot, `0` for most items.
    pub coins: u32,
    /// Whether it works as found.
    pub condition: Condition,
    /// The one city it turns up in, or `None` for every city.
    pub city: Option<City>,
    /// Relative weight among the items its tier can be in one place.
    pub weight: u32,
}

const fn item(
    id: &'static str,
    tier: u8,
    experience: u32,
    condition: Condition,
    city: Option<City>,
) -> FindItem {
    FindItem {
        id,
        tier: FindTier(tier),
        experience,
        coins: 0,
        condition,
        city,
        weight: 1,
    }
}

const fn weighted(mut found: FindItem, weight: u32) -> FindItem {
    found.weight = weight;
    found
}

const fn worth(mut found: FindItem, coins: u32) -> FindItem {
    found.coins = coins;
    found
}

use City::Kyiv;
use Condition::{Broken, Intact};

/// Every item a find can be, version [`FIND_CATALOG_VERSION`].
///
/// Music is the thread: cassettes of local bands, players broken and working,
/// the things a person may later repair and put together. The bands are
/// invented, as the rest of the pack is: no real artist or brand.
pub const CATALOG: &[FindItem] = &[
    // Tier 1, common: what any street holds.
    weighted(item("bottle", 1, 10, Intact, None), 3),
    weighted(item("can", 1, 10, Intact, None), 3),
    weighted(item("bottle_cap", 1, 10, Intact, None), 2),
    item("flyer", 1, 10, Intact, None),
    worth(item("small_change", 1, 10, Intact, None), 5),
    item("metro_token", 1, 10, Intact, Some(Kyiv)),
    // Tier 2, common.
    weighted(item("scratched_cd", 2, 25, Intact, None), 2),
    item("blank_cassette", 2, 25, Intact, None),
    item("broken_headphones", 2, 25, Broken, None),
    item("cassette_podil_at_dawn", 2, 25, Intact, Some(Kyiv)),
    item("cassette_left_bank_echo", 2, 25, Intact, Some(Kyiv)),
    item("cassette_trukhaniv_summer", 2, 25, Intact, Some(Kyiv)),
    // Tier 3, common: broken players, the start of a repair.
    item("broken_cassette_player", 3, 60, Broken, None),
    item("broken_cd_player", 3, 60, Broken, None),
    item("broken_dictaphone", 3, 60, Broken, None),
    // Tier 4, uncommon.
    item("dictaphone", 4, 150, Intact, None),
    item("microphone", 4, 150, Intact, None),
    item("cassette_player", 4, 150, Intact, None),
    // Tier 5, rare. A boombox is rare to come across and worth little by
    // itself: what it plays is the point.
    item("cd_player", 5, 400, Intact, None),
    item("cd_boombox", 5, 25, Intact, None),
    // Tier 6, legendary.
    item("reel_to_reel", 6, 1000, Intact, None),
    item("test_pressing", 6, 1000, Intact, Some(Kyiv)),
];

/// Looks an item up by its id.
#[must_use]
pub fn find_item(id: &str) -> Option<&'static FindItem> {
    CATALOG.iter().find(|candidate| candidate.id == id)
}

/// Which item the find `artifact_id`, rolled at `tier`, is.
///
/// The artifact id is `art:seg:<row>:<column>:e<epoch>:<packVersion>:<slot>`.
/// Its segment decides the city; a 32-bit FNV-1a hash of
/// `nilx-one.find-item.v1:` and the id picks among the items of that tier
/// there, by weight.
///
/// # Errors
///
/// Returns [`FindItemError::ArtifactId`] when the id is not canonical, and
/// [`FindItemError::NoItem`] if the catalog had no item for the tier there,
/// which its tests rule out.
pub fn item_for_find(
    artifact_id: &str,
    tier: FindTier,
) -> Result<&'static FindItem, FindItemError> {
    let (row, column) = artifact_segment(artifact_id).ok_or(FindItemError::ArtifactId)?;
    let city = City::of_segment(row, column);
    let pool = || {
        CATALOG
            .iter()
            .filter(move |candidate| candidate.tier == tier)
            .filter(move |candidate| candidate.city.is_none() || candidate.city == city)
    };
    let total: u32 = pool().map(|candidate| candidate.weight).sum();
    let mut point = fnv1a32(PICK_DOMAIN, artifact_id)
        .checked_rem(total)
        .ok_or(FindItemError::NoItem)?;
    for candidate in pool() {
        if point < candidate.weight {
            return Ok(candidate);
        }
        point -= candidate.weight;
    }
    Err(FindItemError::NoItem)
}

/// FNV-1a over the UTF-8 of `prefix` then `value`, 32 bits.
fn fnv1a32(prefix: &str, value: &str) -> u32 {
    prefix
        .bytes()
        .chain(value.bytes())
        .fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        })
}

/// The segment row and column of a canonical artifact id.
fn artifact_segment(id: &str) -> Option<(i64, i64)> {
    let mut parts = id.strip_prefix("art:seg:")?.split(':');
    let row = canonical_integer(parts.next()?)?;
    let column = canonical_integer(parts.next()?)?;
    let epoch = parts.next()?.strip_prefix('e')?;
    canonical_integer(epoch)?;
    let pack_version = canonical_integer(parts.next()?)?;
    let slot = canonical_integer(parts.next()?)?;
    if parts.next().is_some() || pack_version < 1 || slot < 0 {
        return None;
    }
    Some((row, column))
}

/// A decimal integer as `String(n)` writes it: no `+`, no leading zero, no
/// `-0`.
fn canonical_integer(text: &str) -> Option<i64> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty()
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || (digits.len() > 1 && digits.starts_with('0'))
        || text == "-0"
    {
        return None;
    }
    text.parse().ok()
}

/// Stable failure classification for find items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindItemError {
    /// The artifact id is not canonical.
    ArtifactId,
    /// The pick-up rarities are not a canonical list of rarity codes.
    PickupRarities,
    /// The catalog has no item of the tier for the place.
    NoItem,
}

impl FindItemError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ArtifactId => "artifact_id",
            Self::PickupRarities => "pickup_rarities",
            Self::NoItem => "no_item",
        }
    }
}

impl fmt::Display for FindItemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for FindItemError {}

#[cfg(test)]
mod tests {
    use super::{
        CATALOG, City, Condition, FindItemError, FindTier, PickupRarities, Rarity, find_item,
        fnv1a32, item_for_find,
    };
    use std::collections::{BTreeMap, HashSet};

    const KYIV: &str = "art:seg:312346:298243:e2908:1:0";
    const ELSEWHERE: &str = "art:seg:100:200:e2908:1:0";

    #[test]
    fn tiers_read_as_rarities() {
        let rarities: Vec<_> = FindTier::ALL.iter().map(|tier| tier.rarity()).collect();
        assert_eq!(
            rarities,
            [
                Rarity::Common,
                Rarity::Common,
                Rarity::Common,
                Rarity::Uncommon,
                Rarity::Rare,
                Rarity::Legendary
            ]
        );
        assert_eq!(FindTier::new(0), None);
        assert_eq!(FindTier::new(7), None);
    }

    #[test]
    fn ids_are_unique_and_binding_safe() {
        let mut seen = HashSet::new();
        for found in CATALOG {
            assert!(seen.insert(found.id), "duplicate {}", found.id);
            assert!(
                found
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{}",
                found.id
            );
            assert!(found.weight > 0, "{}", found.id);
            assert!(found.experience > 0, "{}", found.id);
        }
        assert_eq!(
            find_item("cd_boombox").map(|found| found.experience),
            Some(25)
        );
        assert_eq!(find_item("nothing"), None);
    }

    #[test]
    fn every_tier_has_an_item_everywhere() {
        for tier in FindTier::ALL {
            for city in City::ALL.map(Some).into_iter().chain([None]) {
                assert!(
                    CATALOG
                        .iter()
                        .any(|found| found.tier == tier
                            && (found.city.is_none() || found.city == city)),
                    "tier {} in {city:?}",
                    tier.get()
                );
            }
        }
    }

    #[test]
    fn broken_players_are_common_and_working_ones_are_not() {
        for id in [
            "broken_cassette_player",
            "broken_cd_player",
            "broken_dictaphone",
        ] {
            let found = find_item(id).unwrap();
            assert_eq!(found.condition, Condition::Broken);
            assert_eq!(found.tier.rarity(), Rarity::Common);
        }
        assert_eq!(find_item("cd_player").unwrap().tier.rarity(), Rarity::Rare);
    }

    #[test]
    fn cities_are_read_from_the_segment() {
        assert_eq!(City::of_segment(312_346, 298_243), Some(City::Kyiv));
        assert_eq!(City::of_segment(100, 200), None);
        assert_eq!(City::of_segment(292_200, 258_000), Some(City::Barcelona));
        assert_eq!(
            City::of_segment(333_500, 297_500),
            Some(City::SaintPetersburg)
        );
    }

    #[test]
    fn the_hash_is_fnv1a() {
        // Published FNV-1a 32 vectors.
        assert_eq!(fnv1a32("", ""), 0x811c_9dc5);
        assert_eq!(fnv1a32("", "a"), 0xe40c_292c);
        assert_eq!(fnv1a32("foo", "bar"), 0xbf9c_f968);
    }

    #[test]
    fn a_find_is_always_the_same_item() {
        let tier = FindTier::new(2).unwrap();
        assert_eq!(item_for_find(KYIV, tier), item_for_find(KYIV, tier));
    }

    #[test]
    fn local_items_stay_in_their_city() {
        for tier in FindTier::ALL {
            for epoch in 0..400 {
                let id = format!("art:seg:100:{epoch}:e2908:1:0");
                let found = item_for_find(&id, tier).unwrap();
                assert_eq!(found.city, None, "{}", found.id);
            }
        }
    }

    #[test]
    fn picks_follow_the_weights() {
        let tier = FindTier::new(1).unwrap();
        let mut counts = BTreeMap::new();
        for column in 0..20_000 {
            let id = format!("art:seg:7:{column}:e2908:1:0");
            *counts
                .entry(item_for_find(&id, tier).unwrap().id)
                .or_insert(0_u32) += 1;
        }
        // Outside a city the tier 1 weights are 3, 3, 2, 1, 1: a tenth is 2000.
        for (id, weight) in [("bottle", 3), ("can", 3), ("bottle_cap", 2), ("flyer", 1)] {
            let expected = 2_000 * weight;
            let got = counts[id];
            assert!(
                got.abs_diff(expected) < expected / 10,
                "{id}: {got} vs {expected}"
            );
        }
        assert!(!counts.contains_key("metro_token"));
    }

    #[test]
    fn golden_picks() {
        // Changing any of these means a find is now a different item: raise
        // FIND_CATALOG_VERSION and update the web client and the service.
        let picks: Vec<_> = FindTier::ALL
            .iter()
            .flat_map(|tier| [KYIV, ELSEWHERE].map(|id| item_for_find(id, *tier).unwrap().id))
            .collect();
        assert_eq!(picks, GOLDEN);
    }

    const GOLDEN: [&str; 12] = [
        "bottle_cap",
        "bottle_cap",
        "scratched_cd",
        "broken_headphones",
        "broken_cassette_player",
        "broken_cassette_player",
        "dictaphone",
        "dictaphone",
        "cd_boombox",
        "cd_boombox",
        "test_pressing",
        "reel_to_reel",
    ];

    #[test]
    fn non_canonical_ids_are_refused() {
        let tier = FindTier::new(1).unwrap();
        for id in [
            "",
            "art:seg:1:2:e3:1",
            "art:seg:01:2:e3:1:0",
            "art:seg:1:2:e3:0:0",
            "art:seg:1:2:e3:1:0:9",
            "art:seg:-0:2:e3:1:0",
            "art:seg:+1:2:e3:1:0",
            "art:seg:1:2:3:1:0",
        ] {
            assert_eq!(
                item_for_find(id, tier),
                Err(FindItemError::ArtifactId),
                "{id}"
            );
        }
        assert!(item_for_find("art:seg:-5:2:e-1:1:0", tier).is_ok());
    }

    #[test]
    fn pickup_rarities_round_trip() {
        assert_eq!(
            PickupRarities::default().to_wire(),
            "common,uncommon,rare,legendary"
        );
        let only_rare = PickupRarities::NONE.with(Rarity::Rare, true);
        assert_eq!(only_rare.to_wire(), "rare");
        assert!(only_rare.picks_up(FindTier::new(5).unwrap()));
        assert!(!only_rare.picks_up(FindTier::new(1).unwrap()));
        assert_eq!(
            PickupRarities::from_wire("legendary,common")
                .unwrap()
                .to_wire(),
            "common,legendary"
        );
        assert_eq!(PickupRarities::from_wire(""), Ok(PickupRarities::NONE));
        for bad in ["rare,rare", "epic", "common,", " common"] {
            assert_eq!(
                PickupRarities::from_wire(bad),
                Err(FindItemError::PickupRarities),
                "{bad}"
            );
        }
    }
}
