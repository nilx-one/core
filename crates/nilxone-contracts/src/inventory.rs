// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! What a Bond and its Avaia carry, the Bond's Seeds, and the craft running
//! in the background.
//!
//! Two grids: the Bond's own and the Avaia's. Each finds and keeps its own
//! things; only the Bond sells and crafts, so what the Avaia carries is
//! handed over first. When a hand-over is allowed (the two met, say) is the
//! host's call.
//!
//! A craft is confirmed, then runs on the clock: the inputs and the Seeds are
//! taken when it starts, the thing it makes and its experience come when it
//! is finished. Time is milliseconds since the Unix epoch, given by the
//! caller; the service is the clock that counts.

use crate::backpack::{Backpack, Carry, Placed};
use crate::economy::{EconomyError, Place, Recipe, item_kind, recipe};
use crate::find_item::FindItem;

/// Whose grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Holder {
    /// The person's own.
    Bond,
    /// The Avaia's.
    Avaia,
}

impl Holder {
    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Bond => "bond",
            Self::Avaia => "avaia",
        }
    }
}

/// A craft running in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftJob {
    /// The recipe.
    pub recipe: &'static Recipe,
    /// When it was confirmed.
    pub started_ms: u64,
    /// When it is done.
    pub ready_ms: u64,
}

/// What a change paid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Seeds gained: a sale, or small change picked up.
    pub seeds_gained: u64,
    /// Seeds spent.
    pub seeds_spent: u64,
    /// Experience the Bond earned by it. A pick-up pays through the find's
    /// own award, so it is `0` here.
    pub experience: u32,
}

/// Everything a Bond and its Avaia hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    bond: Backpack,
    avaia: Backpack,
    seeds: u64,
    job: Option<CraftJob>,
}

impl Default for Inventory {
    /// Two backpacks, no Seeds, nothing crafting.
    fn default() -> Self {
        Self::new(Carry::Backpack, Carry::Backpack)
    }
}

impl Inventory {
    /// Empty grids of the given kinds.
    #[must_use]
    pub const fn new(bond: Carry, avaia: Carry) -> Self {
        Self {
            bond: Backpack::new(bond),
            avaia: Backpack::new(avaia),
            seeds: 0,
            job: None,
        }
    }

    /// An inventory as stored: two grids already checked and the Seeds. A
    /// running craft is restored with [`Self::resume_craft`].
    #[must_use]
    pub const fn restore(bond: Backpack, avaia: Backpack, seeds: u64) -> Self {
        Self {
            bond,
            avaia,
            seeds,
            job: None,
        }
    }

    /// One holder's grid.
    #[must_use]
    pub const fn backpack(&self, holder: Holder) -> &Backpack {
        match holder {
            Holder::Bond => &self.bond,
            Holder::Avaia => &self.avaia,
        }
    }

    fn backpack_mut(&mut self, holder: Holder) -> &mut Backpack {
        match holder {
            Holder::Bond => &mut self.bond,
            Holder::Avaia => &mut self.avaia,
        }
    }

    /// The Bond's Seeds. The Avaia holds none.
    #[must_use]
    pub const fn seeds(&self) -> u64 {
        self.seeds
    }

    /// The craft running, if any.
    #[must_use]
    pub const fn craft_job(&self) -> Option<&CraftJob> {
        self.job.as_ref()
    }

    /// Takes a picked-up find into `holder`'s grid. Money is credited to the
    /// Bond whoever picked it up; a thing goes to the first place it fits.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NoRoom`] when the thing fits nowhere: it is left
    /// where it lies. [`EconomyError::Overflow`] for an impossible balance.
    pub fn pick_up(&mut self, holder: Holder, found: &FindItem) -> Result<Outcome, EconomyError> {
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
        self.backpack_mut(holder).put(found.id)?;
        Ok(Outcome::default())
    }

    /// Moves a thing within `holder`'s grid, or puts it in at a cell.
    ///
    /// # Errors
    ///
    /// [`EconomyError::Missing`] when nothing starts at `from`, or
    /// [`EconomyError::NoRoom`] when it does not fit at `to`; either way
    /// nothing moves.
    pub fn rearrange(
        &mut self,
        holder: Holder,
        from: (u8, u8),
        to: (u8, u8),
    ) -> Result<Placed, EconomyError> {
        let backpack = self.backpack_mut(holder);
        let mut next = backpack.clone();
        let id = next.take_at(from.0, from.1)?;
        let placed = next.put_at(id, to.0, to.1)?;
        *backpack = next;
        Ok(placed)
    }

    /// Hands the thing at `x`, `y` of one grid to the other, into the first
    /// place it fits there.
    ///
    /// # Errors
    ///
    /// [`EconomyError::Missing`] or [`EconomyError::NoRoom`]; either way
    /// nothing moves.
    pub fn hand_over(&mut self, from: Holder, x: u8, y: u8) -> Result<Placed, EconomyError> {
        let to = match from {
            Holder::Bond => Holder::Avaia,
            Holder::Avaia => Holder::Bond,
        };
        let mut giving = self.backpack(from).clone();
        let mut taking = self.backpack(to).clone();
        let id = giving.take_at(x, y)?;
        let placed = taking.put(id)?;
        *self.backpack_mut(from) = giving;
        *self.backpack_mut(to) = taking;
        Ok(placed)
    }

    /// Changes what `holder` carries things in.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NoRoom`] when the things do not fit the new grid.
    pub fn switch_carry(&mut self, holder: Holder, carry: Carry) -> Result<(), EconomyError> {
        self.backpack_mut(holder).switch_to(carry)
    }

    /// Sells `count` of `id` from the Bond's grid.
    ///
    /// # Errors
    ///
    /// [`EconomyError::UnknownItem`], [`EconomyError::NotForSale`],
    /// [`EconomyError::Missing`], or [`EconomyError::Overflow`].
    pub fn sell(&mut self, id: &str, count: u32) -> Result<Outcome, EconomyError> {
        let kind = item_kind(id).ok_or(EconomyError::UnknownItem)?;
        let price = kind.sell_price.ok_or(EconomyError::NotForSale)?;
        if count == 0 || self.bond.count(id) < count {
            return Err(EconomyError::Missing);
        }
        let paid = price
            .checked_mul(u64::from(count))
            .ok_or(EconomyError::Overflow)?;
        let seeds = self.seeds.checked_add(paid).ok_or(EconomyError::Overflow)?;
        self.bond.take(kind.id, count)?;
        self.seeds = seeds;
        Ok(Outcome {
            seeds_gained: paid,
            ..Outcome::default()
        })
    }

    /// Starts `recipe` at `place`, at `now_ms`, once the person confirmed it.
    /// The inputs and the Seeds are taken now; the tools stay. One craft at a
    /// time. All or nothing.
    ///
    /// # Errors
    ///
    /// [`EconomyError::Busy`], [`EconomyError::WrongPlace`],
    /// [`EconomyError::Missing`] for a missing thing or tool,
    /// [`EconomyError::NotEnoughSeeds`], or [`EconomyError::Overflow`].
    pub fn start_craft(
        &mut self,
        recipe: &'static Recipe,
        place: Place,
        now_ms: u64,
    ) -> Result<CraftJob, EconomyError> {
        if self.job.is_some() {
            return Err(EconomyError::Busy);
        }
        if recipe.place != Place::Anywhere && recipe.place != place {
            return Err(EconomyError::WrongPlace);
        }
        if recipe.tools.iter().any(|tool| self.bond.count(tool) == 0) {
            return Err(EconomyError::Missing);
        }
        if self.seeds < recipe.seeds {
            return Err(EconomyError::NotEnoughSeeds);
        }
        let mut bond = self.bond.clone();
        for (id, count) in recipe.consumes {
            bond.take(id, *count)?;
        }
        let ready_ms = u64::from(recipe.minutes)
            .checked_mul(60_000)
            .and_then(|duration| now_ms.checked_add(duration))
            .ok_or(EconomyError::Overflow)?;
        let job = CraftJob {
            recipe,
            started_ms: now_ms,
            ready_ms,
        };
        self.bond = bond;
        self.seeds -= recipe.seeds;
        self.job = Some(job);
        Ok(job)
    }

    /// Finishes the running craft at `now_ms`: the thing it makes goes into
    /// the Bond's grid and its experience is paid.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NoJob`], [`EconomyError::NotReady`] before its time,
    /// or [`EconomyError::NoRoom`]: the craft then stays done and waiting
    /// until there is room.
    pub fn finish_craft(&mut self, now_ms: u64) -> Result<Outcome, EconomyError> {
        let job = self.job.ok_or(EconomyError::NoJob)?;
        if now_ms < job.ready_ms {
            return Err(EconomyError::NotReady);
        }
        self.deliver(job)
    }

    /// Finishes a legendary craft at once, without its week. Only for real
    /// money: the host calls this after the service confirmed the payment,
    /// and Core cannot tell that it did.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NoJob`], [`EconomyError::NotLegendary`] for an
    /// everyday craft, or [`EconomyError::NoRoom`].
    pub fn finish_paid(&mut self) -> Result<Outcome, EconomyError> {
        let job = self.job.ok_or(EconomyError::NoJob)?;
        if !job.recipe.legendary {
            return Err(EconomyError::NotLegendary);
        }
        self.deliver(job)
    }

    fn deliver(&mut self, job: CraftJob) -> Result<Outcome, EconomyError> {
        self.bond.put(job.recipe.makes)?;
        self.job = None;
        Ok(Outcome {
            seeds_spent: job.recipe.seeds,
            experience: job.recipe.experience,
            ..Outcome::default()
        })
    }

    /// Restores a running craft, as stored by the host, onto this inventory.
    ///
    /// # Errors
    ///
    /// [`EconomyError::Busy`] when one is already running, or
    /// [`EconomyError::UnknownItem`] for an unknown recipe.
    pub fn resume_craft(
        &mut self,
        recipe_id: &str,
        started_ms: u64,
        ready_ms: u64,
    ) -> Result<(), EconomyError> {
        if self.job.is_some() {
            return Err(EconomyError::Busy);
        }
        let recipe = recipe(recipe_id).ok_or(EconomyError::UnknownItem)?;
        self.job = Some(CraftJob {
            recipe,
            started_ms,
            ready_ms,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Holder, Inventory};
    use crate::backpack::Carry;
    use crate::economy::{EconomyError, Place, recipe};
    use crate::find_item::find_item;

    const MINUTE: u64 = 60_000;

    fn bond_holding(things: &[(&str, u32)], seeds: u64) -> Inventory {
        let mut inventory = Inventory::new(Carry::Bag, Carry::Backpack);
        for (id, count) in things {
            for _ in 0..*count {
                inventory.bond.put(id).unwrap();
            }
        }
        inventory.seeds = seeds;
        inventory
    }

    #[test]
    fn each_finds_into_its_own_grid() {
        let mut inventory = Inventory::default();
        inventory
            .pick_up(Holder::Avaia, find_item("can").unwrap())
            .unwrap();
        assert_eq!(inventory.backpack(Holder::Avaia).count("can"), 1);
        assert_eq!(inventory.backpack(Holder::Bond).count("can"), 0);

        let change = inventory
            .pick_up(Holder::Avaia, find_item("small_change").unwrap())
            .unwrap();
        assert_eq!((change.seeds_gained, inventory.seeds()), (15, 15));
    }

    #[test]
    fn a_full_pocket_leaves_the_find_where_it_lies() {
        let mut inventory = Inventory::new(Carry::Pocket, Carry::Pocket);
        assert_eq!(
            inventory.pick_up(Holder::Bond, find_item("reel_to_reel").unwrap()),
            Err(EconomyError::NoRoom)
        );
    }

    #[test]
    fn the_avaia_hands_over_and_the_bond_sells() {
        let mut inventory = Inventory::default();
        inventory
            .pick_up(Holder::Avaia, find_item("bottle").unwrap())
            .unwrap();
        assert_eq!(inventory.sell("bottle", 1), Err(EconomyError::Missing));
        inventory.hand_over(Holder::Avaia, 0, 0).unwrap();
        assert_eq!(inventory.sell("bottle", 1).unwrap().seeds_gained, 5);
        assert_eq!(inventory.backpack(Holder::Avaia).used_cells(), 0);
        assert_eq!(inventory.sell("flyer", 1), Err(EconomyError::NotForSale));
    }

    #[test]
    fn rearranging_is_all_or_nothing() {
        let mut inventory = Inventory::default();
        inventory
            .pick_up(Holder::Bond, find_item("can").unwrap())
            .unwrap();
        inventory
            .pick_up(Holder::Bond, find_item("can").unwrap())
            .unwrap();
        assert_eq!(
            inventory.rearrange(Holder::Bond, (0, 0), (1, 0)),
            Err(EconomyError::NoRoom)
        );
        let moved = inventory.rearrange(Holder::Bond, (0, 0), (7, 4)).unwrap();
        assert_eq!((moved.x, moved.y), (7, 4));
    }

    #[test]
    fn the_cd_player_repair_takes_45_minutes() {
        let repair = recipe("repair_cd_player").unwrap();
        let mut inventory = bond_holding(&[("broken_cd_player", 1)], 250);
        assert_eq!(
            inventory.start_craft(repair, Place::Anywhere, 0),
            Err(EconomyError::WrongPlace)
        );
        let job = inventory
            .start_craft(repair, Place::RepairWorkshop, 1_000)
            .unwrap();
        assert_eq!(job.ready_ms, 1_000 + 45 * MINUTE);
        assert_eq!(inventory.seeds(), 50);
        assert_eq!(
            inventory.backpack(Holder::Bond).count("broken_cd_player"),
            0
        );

        assert_eq!(
            inventory.start_craft(repair, Place::RepairWorkshop, 2_000),
            Err(EconomyError::Busy)
        );
        assert_eq!(
            inventory.finish_craft(job.ready_ms - 1),
            Err(EconomyError::NotReady)
        );
        assert_eq!(inventory.finish_paid(), Err(EconomyError::NotLegendary));
        let outcome = inventory.finish_craft(job.ready_ms).unwrap();
        assert_eq!((outcome.seeds_spent, outcome.experience), (200, 50));
        assert_eq!(inventory.backpack(Holder::Bond).count("cd_player"), 1);
        assert_eq!(inventory.craft_job(), None);
        assert_eq!(
            inventory.finish_craft(job.ready_ms),
            Err(EconomyError::NoJob)
        );
    }

    #[test]
    fn a_refused_start_changes_nothing() {
        let repair = recipe("repair_cd_player").unwrap();
        let mut poor = bond_holding(&[("broken_cd_player", 1)], 199);
        let before = poor.clone();
        assert_eq!(
            poor.start_craft(repair, Place::RepairWorkshop, 0),
            Err(EconomyError::NotEnoughSeeds)
        );
        assert_eq!(poor, before);

        let album = recipe("craft_album").unwrap();
        let mut no_player = bond_holding(&[("scratched_cd", 3)], 0);
        assert_eq!(
            no_player.start_craft(album, Place::Anywhere, 0),
            Err(EconomyError::Missing)
        );
        let mut two_discs = bond_holding(&[("scratched_cd", 2), ("cd_player", 1)], 0);
        let before = two_discs.clone();
        assert_eq!(
            two_discs.start_craft(album, Place::Anywhere, 0),
            Err(EconomyError::Missing)
        );
        assert_eq!(two_discs, before);
    }

    #[test]
    fn a_legendary_craft_takes_a_week_or_real_money() {
        let anthology = recipe("craft_kyiv_anthology").unwrap();
        let mut inventory = Inventory::new(Carry::Bag, Carry::Backpack);
        for id in ["album", "kyiv_mixtape", "field_recording", "reel_to_reel"] {
            inventory.bond.put(id).unwrap();
        }
        let mut waiting = inventory.clone();

        let job = inventory
            .start_craft(anthology, Place::Anywhere, 0)
            .unwrap();
        assert_eq!(job.ready_ms, 7 * 24 * 60 * MINUTE);
        assert_eq!(inventory.finish_paid().unwrap().experience, 500);
        assert_eq!(inventory.backpack(Holder::Bond).count("kyiv_anthology"), 1);
        assert_eq!(inventory.backpack(Holder::Bond).count("reel_to_reel"), 1);

        let job = waiting.start_craft(anthology, Place::Anywhere, 0).unwrap();
        assert_eq!(
            waiting.finish_craft(job.ready_ms - 1),
            Err(EconomyError::NotReady)
        );
        assert!(waiting.finish_craft(job.ready_ms).is_ok());
    }

    #[test]
    fn a_done_craft_waits_for_room() {
        let headphones = recipe("repair_headphones").unwrap();
        let mut inventory = Inventory::new(Carry::Pocket, Carry::Pocket);
        inventory
            .pick_up(Holder::Bond, find_item("broken_headphones").unwrap())
            .unwrap();
        inventory.seeds = 20;
        let job = inventory
            .start_craft(headphones, Place::Anywhere, 0)
            .unwrap();
        for _ in 0..5 {
            inventory
                .pick_up(Holder::Bond, find_item("can").unwrap())
                .unwrap();
        }
        assert_eq!(
            inventory.finish_craft(job.ready_ms),
            Err(EconomyError::NoRoom)
        );
        assert_eq!(inventory.craft_job(), Some(&job));
        inventory.sell("can", 2).unwrap();
        assert!(inventory.finish_craft(job.ready_ms).is_ok());
    }

    #[test]
    fn a_stored_craft_resumes() {
        let mut inventory = Inventory::default();
        inventory
            .resume_craft("repair_headphones", 0, 15 * MINUTE)
            .unwrap();
        assert_eq!(
            inventory.resume_craft("repair_headphones", 0, 1),
            Err(EconomyError::Busy)
        );
        assert_eq!(
            Inventory::default().resume_craft("nothing", 0, 1),
            Err(EconomyError::UnknownItem)
        );
    }
}
