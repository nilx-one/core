// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Seeds, the in-game money, and what a Bond does with the things it finds:
//! sell them, repair them, put them together.
//!
//! Everything here is a pure rule; the inventory that follows them is
//! `inventory`, and the host holds it. Core
//! does not store an inventory, does not decide where a Bond stands, and does
//! not attest that a workshop was visited: the host answers that, and the
//! service, as with experience, holds the totals against a commitment.

use core::fmt;

use crate::find_item::{CATALOG, FindItem, find_item};

/// Version of the prices and recipes. Any change to a price, a recipe or a
/// crafted item raises it.
pub const ECONOMY_VERSION: u32 = 1;

/// The in-game money's binding-safe code. One currency everywhere.
pub const SEED_CODE: &str = "seed";

/// The emblem of the one currency: one mark, not three currencies.
pub const SEED_EMBLEM: &str = "₴€£";

/// A thing a Bond can hold: found in the world, or made from found things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemKind {
    /// Stable, binding-safe id.
    pub id: &'static str,
    /// Seeds a sale pays, or `None` for a thing nobody buys.
    pub sell_price: Option<u64>,
}

/// What found things sell for. A find left out is not bought; money itself
/// (small change) is never held as a thing and has no price.
const FOUND_PRICES: &[(&str, u64)] = &[
    ("bottle", 5),
    ("can", 4),
    ("bottle_cap", 2),
    ("metro_token", 8),
    ("scratched_cd", 10),
    ("blank_cassette", 15),
    ("broken_headphones", 10),
    ("cassette_podil_at_dawn", 20),
    ("cassette_left_bank_echo", 20),
    ("cassette_trukhaniv_summer", 20),
    ("broken_cassette_player", 50),
    ("broken_cd_player", 50),
    ("broken_dictaphone", 50),
    ("dictaphone", 200),
    ("microphone", 200),
    ("cassette_player", 200),
    ("cd_player", 600),
    ("cd_radio", 80),
    ("reel_to_reel", 2000),
    ("test_pressing", 2500),
];

/// Things that are never found, only made, and what they sell for.
pub const CRAFTED: &[ItemKind] = &[
    ItemKind {
        id: "headphones",
        sell_price: Some(40),
    },
    ItemKind {
        id: "album",
        sell_price: Some(350),
    },
    ItemKind {
        id: "kyiv_mixtape",
        sell_price: Some(500),
    },
    ItemKind {
        id: "field_recording",
        sell_price: Some(150),
    },
    ItemKind {
        id: "kyiv_anthology",
        sell_price: Some(5000),
    },
];

/// Looks up anything a Bond can hold: a found thing or a crafted one.
/// Money (an item worth seeds on the spot) is not a thing and is `None`.
#[must_use]
pub fn item_kind(id: &str) -> Option<ItemKind> {
    if let Some(found) = find_item(id) {
        if found.seeds > 0 {
            return None;
        }
        return Some(ItemKind {
            id: found.id,
            sell_price: FOUND_PRICES
                .iter()
                .find(|(priced, _)| *priced == id)
                .map(|(_, price)| *price),
        });
    }
    CRAFTED.iter().copied().find(|crafted| crafted.id == id)
}

/// Where a recipe can be done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    /// Anywhere: at home, on a bench.
    Anywhere,
    /// At a real electronics repair workshop the Bond walked to. Which places
    /// count (a radio market stall, a repair shop on the map) is the host's.
    RepairWorkshop,
}

impl Place {
    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Anywhere => "anywhere",
            Self::RepairWorkshop => "repair_workshop",
        }
    }
}

/// A repair or a craft: things in, a thing out, experience for the Bond.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    /// Stable, binding-safe id.
    pub id: &'static str,
    /// Things used up, with how many of each.
    pub consumes: &'static [(&'static str, u32)],
    /// Things needed but kept, such as the player an album is played on.
    pub tools: &'static [&'static str],
    /// Seeds it costs.
    pub seeds: u64,
    /// The thing it makes.
    pub makes: &'static str,
    /// Experience it pays the Bond. A repair or a craft is the person's own
    /// doing, so it never pays the Avaia.
    pub experience: u32,
    /// Where it can be done.
    pub place: Place,
    /// How long it takes, in minutes, running in the background once
    /// confirmed: 15 to 45 for everyday work, a week for a legendary one.
    pub minutes: u32,
    /// A legendary craft: a week of waiting, or done at once for real money.
    pub legendary: bool,
}

/// Every repair and craft, version [`ECONOMY_VERSION`].
pub const RECIPES: &[Recipe] = &[
    Recipe {
        id: "repair_cd_player",
        consumes: &[("broken_cd_player", 1)],
        tools: &[],
        seeds: 200,
        makes: "cd_player",
        experience: 50,
        place: Place::RepairWorkshop,
        minutes: 45,
        legendary: false,
    },
    Recipe {
        id: "repair_cassette_player",
        consumes: &[("broken_cassette_player", 1)],
        tools: &[],
        seeds: 100,
        makes: "cassette_player",
        experience: 40,
        place: Place::RepairWorkshop,
        minutes: 30,
        legendary: false,
    },
    Recipe {
        id: "repair_dictaphone",
        consumes: &[("broken_dictaphone", 1)],
        tools: &[],
        seeds: 80,
        makes: "dictaphone",
        experience: 40,
        place: Place::RepairWorkshop,
        minutes: 30,
        legendary: false,
    },
    Recipe {
        id: "repair_headphones",
        consumes: &[("broken_headphones", 1)],
        tools: &[],
        seeds: 20,
        makes: "headphones",
        experience: 15,
        place: Place::Anywhere,
        minutes: 15,
        legendary: false,
    },
    Recipe {
        id: "craft_album",
        consumes: &[("scratched_cd", 3)],
        tools: &["cd_player"],
        seeds: 0,
        makes: "album",
        experience: 75,
        place: Place::Anywhere,
        minutes: 45,
        legendary: false,
    },
    Recipe {
        id: "craft_kyiv_mixtape",
        consumes: &[
            ("blank_cassette", 1),
            ("cassette_podil_at_dawn", 1),
            ("cassette_left_bank_echo", 1),
            ("cassette_trukhaniv_summer", 1),
        ],
        tools: &["cassette_player"],
        seeds: 0,
        makes: "kyiv_mixtape",
        experience: 120,
        place: Place::Anywhere,
        minutes: 45,
        legendary: false,
    },
    Recipe {
        id: "craft_field_recording",
        consumes: &[("blank_cassette", 1)],
        tools: &["dictaphone", "microphone"],
        seeds: 0,
        makes: "field_recording",
        experience: 60,
        place: Place::Anywhere,
        minutes: 30,
        legendary: false,
    },
    Recipe {
        id: "craft_kyiv_anthology",
        consumes: &[("album", 1), ("kyiv_mixtape", 1), ("field_recording", 1)],
        tools: &["reel_to_reel"],
        seeds: 0,
        makes: "kyiv_anthology",
        experience: 500,
        place: Place::Anywhere,
        minutes: 7 * 24 * 60,
        legendary: true,
    },
];

/// Looks a recipe up by its id.
#[must_use]
pub fn recipe(id: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|candidate| candidate.id == id)
}

/// Every found thing that can be held (money is not), for hosts that list
/// them.
pub fn found_things() -> impl Iterator<Item = &'static FindItem> {
    CATALOG.iter().filter(|found| found.seeds == 0)
}

/// Stable failure classification for the economy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EconomyError {
    /// No such thing.
    UnknownItem,
    /// Nobody buys it.
    NotForSale,
    /// Fewer are held than needed, or a tool is missing.
    Missing,
    /// The balance does not cover the cost.
    NotEnoughSeeds,
    /// The recipe needs a place the Bond is not at.
    WrongPlace,
    /// A count or the balance would overflow.
    Overflow,
    /// The thing does not fit the grid there.
    NoRoom,
    /// A craft is already running.
    Busy,
    /// No craft is running.
    NoJob,
    /// The craft is not done yet.
    NotReady,
    /// Only a legendary craft can be finished at once.
    NotLegendary,
}

impl EconomyError {
    /// Returns the binding-safe failure code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnknownItem => "unknown_item",
            Self::NotForSale => "not_for_sale",
            Self::Missing => "missing",
            Self::NotEnoughSeeds => "not_enough_seeds",
            Self::WrongPlace => "wrong_place",
            Self::Overflow => "overflow",
            Self::NoRoom => "no_room",
            Self::Busy => "busy",
            Self::NoJob => "no_job",
            Self::NotReady => "not_ready",
            Self::NotLegendary => "not_legendary",
        }
    }
}

impl fmt::Display for EconomyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for EconomyError {}

#[cfg(test)]
mod tests {
    use super::{CRAFTED, FOUND_PRICES, RECIPES, found_things, item_kind};
    use crate::find_item::{CATALOG, find_item};
    use std::collections::HashSet;

    #[test]
    fn every_id_is_known_once() {
        let mut seen = HashSet::new();
        for id in CATALOG
            .iter()
            .map(|found| found.id)
            .chain(CRAFTED.iter().map(|kind| kind.id))
        {
            assert!(seen.insert(id), "duplicate {id}");
        }
        for (id, price) in FOUND_PRICES {
            let found = find_item(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(found.seeds, 0, "money has no price: {id}");
            assert!(*price > 0, "{id}");
        }
    }

    #[test]
    fn small_change_is_money_not_a_thing() {
        assert_eq!(item_kind("small_change"), None);
        assert!(found_things().all(|found| found.id != "small_change"));
        assert_eq!(item_kind("flyer").map(|kind| kind.sell_price), Some(None));
    }

    #[test]
    fn every_recipe_names_real_things() {
        let mut seen = HashSet::new();
        for recipe in RECIPES {
            assert!(seen.insert(recipe.id), "duplicate {}", recipe.id);
            assert!(item_kind(recipe.makes).is_some(), "{}", recipe.id);
            for (id, count) in recipe.consumes {
                assert!(item_kind(id).is_some(), "{}: {id}", recipe.id);
                assert!(*count > 0, "{}: {id}", recipe.id);
            }
            for tool in recipe.tools {
                assert!(item_kind(tool).is_some(), "{}: {tool}", recipe.id);
            }
            assert!(recipe.experience > 0, "{}", recipe.id);
        }
        for crafted in CRAFTED {
            assert!(
                RECIPES.iter().any(|recipe| recipe.makes == crafted.id),
                "{} is never made",
                crafted.id
            );
        }
    }

    #[test]
    fn crafting_takes_its_time() {
        for recipe in RECIPES {
            if recipe.legendary {
                assert_eq!(recipe.minutes, 7 * 24 * 60, "{}", recipe.id);
            } else {
                assert!((15..=45).contains(&recipe.minutes), "{}", recipe.id);
            }
        }
        assert!(RECIPES.iter().any(|recipe| recipe.legendary));
    }
}
