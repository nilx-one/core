// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Seeds, the in-game money, and what a Bond does with the things it finds:
//! sell them, repair them, put them together.
//!
//! Everything here is a pure rule over an [`Inventory`] the host holds. Core
//! does not store an inventory, does not decide where a Bond stands, and does
//! not attest that a workshop was visited: the host answers that, and the
//! service, as with experience, holds the totals against a commitment.

use std::collections::BTreeMap;

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
    },
    Recipe {
        id: "repair_cassette_player",
        consumes: &[("broken_cassette_player", 1)],
        tools: &[],
        seeds: 100,
        makes: "cassette_player",
        experience: 40,
        place: Place::RepairWorkshop,
    },
    Recipe {
        id: "repair_dictaphone",
        consumes: &[("broken_dictaphone", 1)],
        tools: &[],
        seeds: 80,
        makes: "dictaphone",
        experience: 40,
        place: Place::RepairWorkshop,
    },
    Recipe {
        id: "repair_headphones",
        consumes: &[("broken_headphones", 1)],
        tools: &[],
        seeds: 20,
        makes: "headphones",
        experience: 15,
        place: Place::Anywhere,
    },
    Recipe {
        id: "craft_album",
        consumes: &[("scratched_cd", 3)],
        tools: &["cd_player"],
        seeds: 0,
        makes: "album",
        experience: 75,
        place: Place::Anywhere,
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
    },
    Recipe {
        id: "craft_field_recording",
        consumes: &[("blank_cassette", 1)],
        tools: &["dictaphone", "microphone"],
        seeds: 0,
        makes: "field_recording",
        experience: 60,
        place: Place::Anywhere,
    },
];

/// Looks a recipe up by its id.
#[must_use]
pub fn recipe(id: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|candidate| candidate.id == id)
}

/// What a Bond holds: things by kind, and seeds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    things: BTreeMap<&'static str, u32>,
    seeds: u64,
}

/// What a change to an inventory paid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Seeds gained: a sale, or small change picked up.
    pub seeds_gained: u64,
    /// Seeds spent.
    pub seeds_spent: u64,
    /// Experience the Bond earned by it. Picking up pays through the find's
    /// own award, so it is `0` here.
    pub experience: u32,
}

impl Inventory {
    /// An empty inventory.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many of `id` are held.
    #[must_use]
    pub fn count(&self, id: &str) -> u32 {
        self.things.get(id).copied().unwrap_or(0)
    }

    /// Seeds held.
    #[must_use]
    pub const fn seeds(&self) -> u64 {
        self.seeds
    }

    /// Every thing held, by id, with its count. Ids in a fixed order.
    pub fn things(&self) -> impl Iterator<Item = (&'static str, u32)> + '_ {
        self.things.iter().map(|(id, count)| (*id, *count))
    }

    /// Takes a picked-up find in: money is credited, a thing is kept.
    ///
    /// # Errors
    ///
    /// Returns [`EconomyError::Overflow`] if a count or the balance would
    /// overflow.
    pub fn pick_up(&mut self, found: &FindItem) -> Result<Outcome, EconomyError> {
        if found.seeds > 0 {
            self.seeds = self
                .seeds
                .checked_add(found.seeds)
                .ok_or(EconomyError::Overflow)?;
            return Ok(Outcome {
                seeds_gained: found.seeds,
                ..Outcome::default()
            });
        }
        self.add(found.id, 1)?;
        Ok(Outcome::default())
    }

    /// Sells `count` of `id`.
    ///
    /// # Errors
    ///
    /// [`EconomyError::UnknownItem`], [`EconomyError::NotForSale`],
    /// [`EconomyError::Missing`] when fewer are held, or
    /// [`EconomyError::Overflow`].
    pub fn sell(&mut self, id: &str, count: u32) -> Result<Outcome, EconomyError> {
        let kind = item_kind(id).ok_or(EconomyError::UnknownItem)?;
        let price = kind.sell_price.ok_or(EconomyError::NotForSale)?;
        if count == 0 || self.count(id) < count {
            return Err(EconomyError::Missing);
        }
        let paid = price
            .checked_mul(u64::from(count))
            .ok_or(EconomyError::Overflow)?;
        let seeds = self.seeds.checked_add(paid).ok_or(EconomyError::Overflow)?;
        self.remove(kind.id, count);
        self.seeds = seeds;
        Ok(Outcome {
            seeds_gained: paid,
            ..Outcome::default()
        })
    }

    /// Does `recipe` at `place`. All or nothing: a refused recipe leaves the
    /// inventory as it was.
    ///
    /// # Errors
    ///
    /// [`EconomyError::WrongPlace`], [`EconomyError::Missing`] for a missing
    /// thing or tool, [`EconomyError::NotEnoughSeeds`], or
    /// [`EconomyError::Overflow`].
    pub fn craft(&mut self, recipe: &Recipe, place: Place) -> Result<Outcome, EconomyError> {
        if recipe.place != Place::Anywhere && recipe.place != place {
            return Err(EconomyError::WrongPlace);
        }
        for (id, count) in recipe.consumes {
            if self.count(id) < *count {
                return Err(EconomyError::Missing);
            }
        }
        for tool in recipe.tools {
            if self.count(tool) == 0 {
                return Err(EconomyError::Missing);
            }
        }
        if self.seeds < recipe.seeds {
            return Err(EconomyError::NotEnoughSeeds);
        }
        let made = item_kind(recipe.makes).ok_or(EconomyError::UnknownItem)?;
        let mut next = self.clone();
        for (id, count) in recipe.consumes {
            next.remove(id, *count);
        }
        next.seeds -= recipe.seeds;
        next.add(made.id, 1)?;
        *self = next;
        Ok(Outcome {
            seeds_spent: recipe.seeds,
            experience: recipe.experience,
            ..Outcome::default()
        })
    }

    fn add(&mut self, id: &'static str, count: u32) -> Result<(), EconomyError> {
        let held = self.things.entry(id).or_insert(0);
        *held = held.checked_add(count).ok_or(EconomyError::Overflow)?;
        Ok(())
    }

    fn remove(&mut self, id: &str, count: u32) {
        if let Some(held) = self.things.get_mut(id) {
            *held -= count;
            if *held == 0 {
                self.things.remove(id);
            }
        }
    }
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
    use super::{
        CRAFTED, EconomyError, FOUND_PRICES, Inventory, Place, RECIPES, found_things, item_kind,
        recipe,
    };
    use crate::find_item::{CATALOG, find_item};
    use std::collections::HashSet;

    fn holding(things: &[(&str, u32)], seeds: u64) -> Inventory {
        let mut inventory = Inventory::new();
        for (id, count) in things {
            for _ in 0..*count {
                inventory.pick_up(find_item(id).unwrap()).unwrap();
            }
        }
        inventory.seeds = seeds;
        inventory
    }

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
    fn small_change_is_money_not_a_thing() {
        let mut inventory = Inventory::new();
        let outcome = inventory
            .pick_up(find_item("small_change").unwrap())
            .unwrap();
        assert_eq!(outcome.seeds_gained, 15);
        assert_eq!(inventory.seeds(), 15);
        assert_eq!(inventory.count("small_change"), 0);
        assert_eq!(item_kind("small_change"), None);
        assert!(found_things().all(|found| found.id != "small_change"));
    }

    #[test]
    fn selling_pays_by_the_price() {
        let mut inventory = holding(&[("bottle", 3)], 0);
        assert_eq!(inventory.sell("bottle", 2).unwrap().seeds_gained, 10);
        assert_eq!(inventory.count("bottle"), 1);
        assert_eq!(inventory.seeds(), 10);
        assert_eq!(inventory.sell("bottle", 2), Err(EconomyError::Missing));
        assert_eq!(inventory.sell("bottle", 0), Err(EconomyError::Missing));
        assert_eq!(inventory.sell("nothing", 1), Err(EconomyError::UnknownItem));
        let mut flyers = holding(&[("flyer", 1)], 0);
        assert_eq!(flyers.sell("flyer", 1), Err(EconomyError::NotForSale));
        assert_eq!(flyers.count("flyer"), 1);
    }

    #[test]
    fn the_cd_player_is_repaired_at_a_workshop() {
        // A broken player found for 60, repaired for 200 seeds and 50
        // experience into a player that sells for 600.
        let repair = recipe("repair_cd_player").unwrap();
        let mut inventory = holding(&[("broken_cd_player", 1)], 250);

        assert_eq!(
            inventory.craft(repair, Place::Anywhere),
            Err(EconomyError::WrongPlace)
        );
        let outcome = inventory.craft(repair, Place::RepairWorkshop).unwrap();
        assert_eq!((outcome.seeds_spent, outcome.experience), (200, 50));
        assert_eq!(inventory.seeds(), 50);
        assert_eq!(inventory.count("broken_cd_player"), 0);
        assert_eq!(inventory.count("cd_player"), 1);
        assert_eq!(item_kind("cd_player").unwrap().sell_price, Some(600));
    }

    #[test]
    fn a_refused_craft_changes_nothing() {
        let repair = recipe("repair_cd_player").unwrap();
        let mut poor = holding(&[("broken_cd_player", 1)], 199);
        let before = poor.clone();
        assert_eq!(
            poor.craft(repair, Place::RepairWorkshop),
            Err(EconomyError::NotEnoughSeeds)
        );
        assert_eq!(poor, before);

        let album = recipe("craft_album").unwrap();
        let mut no_player = holding(&[("scratched_cd", 3)], 0);
        assert_eq!(
            no_player.craft(album, Place::Anywhere),
            Err(EconomyError::Missing)
        );
        assert_eq!(no_player.count("scratched_cd"), 3);
    }

    #[test]
    fn tools_are_kept_and_inputs_used_up() {
        let album = recipe("craft_album").unwrap();
        let mut inventory = holding(&[("scratched_cd", 4), ("cd_player", 1)], 0);
        assert_eq!(
            inventory.craft(album, Place::Anywhere).unwrap().experience,
            75
        );
        assert_eq!(inventory.count("scratched_cd"), 1);
        assert_eq!(inventory.count("cd_player"), 1);
        assert_eq!(inventory.count("album"), 1);
        assert_eq!(
            inventory.things().collect::<Vec<_>>(),
            [("album", 1), ("cd_player", 1), ("scratched_cd", 1)]
        );
    }

    #[test]
    fn workshops_also_do_what_needs_no_place() {
        let headphones = recipe("repair_headphones").unwrap();
        let mut inventory = holding(&[("broken_headphones", 1)], 20);
        assert!(inventory.craft(headphones, Place::RepairWorkshop).is_ok());
        assert_eq!(inventory.count("headphones"), 1);
    }
}
