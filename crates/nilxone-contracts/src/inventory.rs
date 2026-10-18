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

use crate::backpack::{Backpack, Carries, Carry, Placed};
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
    /// Whether this command gave xSasha's backpack gift.
    pub gifted: bool,
    /// Why a picked-up thing stayed where it lay although the command
    /// succeeded: the gift was given, but the thing does not fit even then.
    pub left_behind: Option<EconomyError>,
}

/// Everything a Bond and its Avaia hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    bond: Backpack,
    avaia: Backpack,
    seeds: u64,
    job: Option<CraftJob>,
    owned: [Carries; 2],
    gifted: bool,
    starter: bool,
}

/// The Seeds ₴€£ every Bond is given once, to start with.
pub const STARTER_SEEDS: u64 = 10_000;

/// The Bond level by which the backpack gift is due, even if nothing was
/// ever too much for the pockets.
pub const GIFT_LEVEL: u32 = 3;

impl Default for Inventory {
    /// Pockets for both, no Seeds, nothing crafting, no gift yet.
    fn default() -> Self {
        Self::new(Carry::Pocket, Carry::Pocket)
    }
}

impl Inventory {
    /// Empty grids of the given kinds, each owned.
    #[must_use]
    pub const fn new(bond: Carry, avaia: Carry) -> Self {
        Self {
            bond: Backpack::new(bond),
            avaia: Backpack::new(avaia),
            seeds: 0,
            job: None,
            owned: [Carries::POCKETS.with(bond), Carries::POCKETS.with(avaia)],
            gifted: false,
            starter: false,
        }
    }

    /// An inventory as stored: two grids already checked, the Seeds, what
    /// each owns and whether the gift was given. Each owns at least what it
    /// carries in. A running craft is restored with [`Self::resume_craft`].
    #[must_use]
    pub const fn restore(
        bond: Backpack,
        avaia: Backpack,
        seeds: u64,
        owned: [Carries; 2],
        gifted: bool,
    ) -> Self {
        let owned = [owned[0].with(bond.carry()), owned[1].with(avaia.carry())];
        Self {
            bond,
            avaia,
            seeds,
            job: None,
            owned,
            gifted,
            starter: false,
        }
    }

    /// The same inventory, recording whether the starter Seeds were given.
    #[must_use]
    pub const fn with_starter(mut self, given: bool) -> Self {
        self.starter = given;
        self
    }

    /// Whether the starter Seeds were given.
    #[must_use]
    pub const fn starter_given(&self) -> bool {
        self.starter
    }

    /// Gives [`STARTER_SEEDS`], once.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NotDue`] when already given, and
    /// [`EconomyError::Overflow`] if the balance cannot hold them.
    pub fn grant_starter_seeds(&mut self) -> Result<Outcome, EconomyError> {
        if self.starter {
            return Err(EconomyError::NotDue);
        }
        self.seeds = self
            .seeds
            .checked_add(STARTER_SEEDS)
            .ok_or(EconomyError::Overflow)?;
        self.starter = true;
        Ok(Outcome {
            seeds_gained: STARTER_SEEDS,
            ..Outcome::default()
        })
    }

    /// Pays for a repair of xPing's engine with `missing_permille` of its
    /// energy missing: [`crate::repair_seeds_for`] Seeds. The engine's own
    /// state is the host's to update ([`crate::XpingEngine::repaired`]).
    ///
    /// # Errors
    ///
    /// [`EconomyError::NotDue`] when nothing is missing, and
    /// [`EconomyError::NotEnoughSeeds`].
    pub fn pay_xping_repair(&mut self, missing_permille: u16) -> Result<Outcome, EconomyError> {
        let price = crate::repair_seeds_for(missing_permille);
        if price == 0 {
            return Err(EconomyError::NotDue);
        }
        if self.seeds < price {
            return Err(EconomyError::NotEnoughSeeds);
        }
        self.seeds -= price;
        Ok(Outcome {
            seeds_spent: price,
            ..Outcome::default()
        })
    }

    /// What `holder` owns to carry things in.
    #[must_use]
    pub const fn owned(&self, holder: Holder) -> Carries {
        match holder {
            Holder::Bond => self.owned[0],
            Holder::Avaia => self.owned[1],
        }
    }

    fn own(&mut self, holder: Holder, carry: Carry) {
        let index = match holder {
            Holder::Bond => 0,
            Holder::Avaia => 1,
        };
        self.owned[index] = self.owned[index].with(carry);
    }

    /// Whether the backpack gift was given.
    #[must_use]
    pub const fn gifted(&self) -> bool {
        self.gifted
    }

    /// Whether xSasha's backpack gift is due by level: not given yet, and the
    /// Bond is level 3. The level is the host's to say. The gift also comes,
    /// earlier, from a pick-up that does not fit the pockets (`pick_up`).
    #[must_use]
    pub const fn backpack_gift_due(&self, bond_level: u32) -> bool {
        !self.gifted && bond_level >= GIFT_LEVEL
    }

    /// Gives both the Bond and the Avaia a backpack, once, when it is due:
    /// each moves into it from pockets.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NotDue`] when it is not due, or already given.
    pub fn gift_backpacks(&mut self, bond_level: u32) -> Result<(), EconomyError> {
        if !self.backpack_gift_due(bond_level) {
            return Err(EconomyError::NotDue);
        }
        self.give_backpacks()
    }

    fn give_backpacks(&mut self) -> Result<(), EconomyError> {
        for holder in [Holder::Bond, Holder::Avaia] {
            self.own(holder, Carry::Backpack);
            if self.backpack(holder).carry() == Carry::Pocket {
                // A backpack holds whatever pockets hold.
                self.backpack_mut(holder).switch_to(Carry::Backpack)?;
            }
        }
        self.gifted = true;
        Ok(())
    }

    /// Buys `carry` for `holder` with the Bond's Seeds and moves into it when
    /// it is bigger than what `holder` carries now.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NotForSale`] for pockets, [`EconomyError::AlreadyOwned`],
    /// or [`EconomyError::NotEnoughSeeds`].
    pub fn buy_carry(&mut self, holder: Holder, carry: Carry) -> Result<Outcome, EconomyError> {
        let price = carry.price().ok_or(EconomyError::NotForSale)?;
        if self.owned(holder).has(carry) {
            return Err(EconomyError::AlreadyOwned);
        }
        if self.seeds < price {
            return Err(EconomyError::NotEnoughSeeds);
        }
        let current = self.backpack(holder).carry();
        if carry.cells() > current.cells() {
            self.backpack_mut(holder).switch_to(carry)?;
        }
        self.seeds -= price;
        self.own(holder, carry);
        Ok(Outcome {
            seeds_spent: price,
            ..Outcome::default()
        })
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
        match self.backpack_mut(holder).put(found.id) {
            Ok(_) => Ok(Outcome::default()),
            // A thing that does not fit the pockets, before the gift, is
            // what brings it: both get a backpack, and it goes into one.
            Err(EconomyError::NoRoom | EconomyError::TooBig)
                if !self.gifted && self.backpack(holder).carry() == Carry::Pocket =>
            {
                self.give_backpacks()?;
                let left_behind = self.backpack_mut(holder).put(found.id).err();
                Ok(Outcome {
                    gifted: true,
                    left_behind,
                    ..Outcome::default()
                })
            }
            Err(error) => Err(error),
        }
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

    /// Changes what `holder` carries things in, to something it owns.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NotOwned`], or [`EconomyError::NoRoom`] when the
    /// things do not fit the new grid.
    pub fn switch_carry(&mut self, holder: Holder, carry: Carry) -> Result<(), EconomyError> {
        if !self.owned(holder).has(carry) {
            return Err(EconomyError::NotOwned);
        }
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
    fn after_the_gift_pockets_worn_again_leave_a_big_find_where_it_lies() {
        let mut inventory = Inventory::default();
        inventory.gift_backpacks(3).unwrap();
        inventory.switch_carry(Holder::Bond, Carry::Pocket).unwrap();
        let before = inventory.clone();
        assert_eq!(
            inventory.pick_up(Holder::Bond, find_item("reel_to_reel").unwrap()),
            Err(EconomyError::TooBig)
        );
        assert_eq!(inventory, before);
    }

    #[test]
    fn the_avaia_hands_over_and_the_bond_sells() {
        let mut inventory = Inventory::new(Carry::Backpack, Carry::Backpack);
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
        let mut inventory = Inventory::new(Carry::Backpack, Carry::Backpack);
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
        let moved = inventory.rearrange(Holder::Bond, (0, 0), (2, 3)).unwrap();
        assert_eq!((moved.x, moved.y), (2, 3));
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
        let mut inventory = Inventory::new(Carry::Backpack, Carry::Pocket);
        inventory
            .pick_up(Holder::Bond, find_item("broken_headphones").unwrap())
            .unwrap();
        inventory.seeds = 20;
        let job = inventory
            .start_craft(headphones, Place::Anywhere, 0)
            .unwrap();
        for _ in 0..12 {
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

    #[test]
    fn both_start_in_pockets_and_own_nothing_else() {
        let mut inventory = Inventory::default();
        for holder in [Holder::Bond, Holder::Avaia] {
            assert_eq!(inventory.backpack(holder).carry(), Carry::Pocket);
            assert!(!inventory.owned(holder).has(Carry::Backpack));
        }
        assert_eq!(
            inventory.switch_carry(Holder::Bond, Carry::Bag),
            Err(EconomyError::NotOwned)
        );
    }

    #[test]
    fn a_full_pocket_alone_does_not_bring_the_gift() {
        let mut inventory = Inventory::default();
        for _ in 0..3 {
            let outcome = inventory
                .pick_up(Holder::Bond, find_item("can").unwrap())
                .unwrap();
            assert!(!outcome.gifted);
        }
        assert!(!inventory.backpack_gift_due(1));
        assert_eq!(inventory.gift_backpacks(1), Err(EconomyError::NotDue));
        assert_eq!(inventory.backpack(Holder::Bond).carry(), Carry::Pocket);
    }

    #[test]
    fn a_thing_the_pockets_cannot_hold_brings_the_gift_and_goes_in() {
        let mut inventory = Inventory::default();
        for _ in 0..3 {
            inventory
                .pick_up(Holder::Bond, find_item("can").unwrap())
                .unwrap();
        }
        let outcome = inventory
            .pick_up(Holder::Bond, find_item("can").unwrap())
            .unwrap();
        assert!(outcome.gifted);
        assert_eq!(outcome.left_behind, None);
        for holder in [Holder::Bond, Holder::Avaia] {
            assert_eq!(inventory.backpack(holder).carry(), Carry::Backpack);
            assert!(inventory.owned(holder).has(Carry::Backpack));
        }
        assert_eq!(inventory.backpack(Holder::Bond).count("can"), 4);
        assert!(inventory.gifted());
        assert_eq!(inventory.gift_backpacks(5), Err(EconomyError::NotDue));
    }

    #[test]
    fn a_thing_too_wide_for_empty_pockets_brings_the_gift_too() {
        let mut inventory = Inventory::default();
        let outcome = inventory
            .pick_up(Holder::Avaia, find_item("reel_to_reel").unwrap())
            .unwrap();
        assert!(outcome.gifted);
        assert_eq!(outcome.left_behind, None);
        assert_eq!(inventory.backpack(Holder::Avaia).count("reel_to_reel"), 1);
        assert_eq!(inventory.backpack(Holder::Bond).carry(), Carry::Backpack);
    }

    #[test]
    fn after_the_gift_a_thing_with_no_room_stays_where_it_lay() {
        let mut inventory = Inventory::default();
        inventory.gift_backpacks(3).unwrap();
        for _ in 0..12 {
            inventory
                .pick_up(Holder::Bond, find_item("can").unwrap())
                .unwrap();
        }
        let before = inventory.clone();
        assert_eq!(
            inventory.pick_up(Holder::Bond, find_item("can").unwrap()),
            Err(EconomyError::NoRoom)
        );
        assert_eq!(inventory, before);
    }

    #[test]
    fn the_gift_comes_by_level_three_however_empty() {
        let inventory = Inventory::default();
        assert!(!inventory.backpack_gift_due(2));
        assert!(inventory.backpack_gift_due(3));
    }

    #[test]
    fn carries_are_bought_with_the_bonds_seeds() {
        let mut inventory = Inventory {
            seeds: 6_000,
            ..Inventory::default()
        };
        assert_eq!(
            inventory.buy_carry(Holder::Bond, Carry::Pocket),
            Err(EconomyError::NotForSale)
        );
        let bag = inventory.buy_carry(Holder::Avaia, Carry::Bag).unwrap();
        assert_eq!(bag.seeds_spent, 5_000);
        assert_eq!(inventory.backpack(Holder::Avaia).carry(), Carry::Bag);
        assert_eq!(inventory.seeds(), 1_000);
        assert_eq!(
            inventory.buy_carry(Holder::Bond, Carry::Backpack),
            Err(EconomyError::NotEnoughSeeds)
        );
        assert_eq!(
            inventory.buy_carry(Holder::Avaia, Carry::Bag),
            Err(EconomyError::AlreadyOwned)
        );

        // A smaller carry bought later is owned, not worn.
        inventory.seeds = 1_500;
        inventory.buy_carry(Holder::Avaia, Carry::Backpack).unwrap();
        assert_eq!(inventory.backpack(Holder::Avaia).carry(), Carry::Bag);
        inventory
            .switch_carry(Holder::Avaia, Carry::Backpack)
            .unwrap();
        assert_eq!(inventory.backpack(Holder::Avaia).carry(), Carry::Backpack);
    }
}
