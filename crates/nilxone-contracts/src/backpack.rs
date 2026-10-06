// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! A grid to carry things in, as in S.T.A.L.K.E.R.: every thing takes a
//! rectangle of cells, and what is carried in decides how many cells there
//! are. A pocket holds 5, a backpack 40, a bag 120.

use crate::economy::EconomyError;

/// What things are carried in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Carry {
    /// Pockets: 5 × 1, 5 cells.
    Pocket,
    /// A backpack: 8 × 5, 40 cells.
    Backpack,
    /// A whole bag: 12 × 10, 120 cells.
    Bag,
}

impl Carry {
    /// Every way to carry, smallest first.
    pub const ALL: [Self; 3] = [Self::Pocket, Self::Backpack, Self::Bag];

    /// Returns the binding-safe code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Pocket => "pocket",
            Self::Backpack => "backpack",
            Self::Bag => "bag",
        }
    }

    /// Parses a binding-safe code.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|carry| carry.code() == code)
    }

    /// Columns and rows of the grid.
    #[must_use]
    pub const fn grid(self) -> Size {
        match self {
            Self::Pocket => Size {
                width: 5,
                height: 1,
            },
            Self::Backpack => Size {
                width: 8,
                height: 5,
            },
            Self::Bag => Size {
                width: 12,
                height: 10,
            },
        }
    }

    /// How many cells the grid has.
    #[must_use]
    pub const fn cells(self) -> u32 {
        let grid = self.grid();
        grid.width as u32 * grid.height as u32
    }
}

/// A rectangle of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    /// Columns.
    pub width: u8,
    /// Rows.
    pub height: u8,
}

/// The rectangle each thing takes. Things are not turned.
const SIZES: &[(&str, u8, u8)] = &[
    ("bottle", 1, 2),
    ("can", 1, 1),
    ("bottle_cap", 1, 1),
    ("flyer", 1, 1),
    ("metro_token", 1, 1),
    ("scratched_cd", 1, 1),
    ("blank_cassette", 1, 1),
    ("broken_headphones", 2, 1),
    ("cassette_podil_at_dawn", 1, 1),
    ("cassette_left_bank_echo", 1, 1),
    ("cassette_trukhaniv_summer", 1, 1),
    ("broken_cassette_player", 2, 2),
    ("broken_cd_player", 2, 1),
    ("broken_dictaphone", 1, 1),
    ("dictaphone", 1, 1),
    ("microphone", 1, 2),
    ("cassette_player", 2, 2),
    ("cd_player", 2, 1),
    ("cd_radio", 3, 2),
    ("reel_to_reel", 3, 3),
    ("test_pressing", 2, 2),
    ("headphones", 2, 1),
    ("album", 1, 1),
    ("kyiv_mixtape", 1, 1),
    ("field_recording", 1, 1),
    ("kyiv_anthology", 2, 2),
];

/// The rectangle a thing takes, or `None` for anything that is not a thing.
#[must_use]
pub fn size_of(id: &str) -> Option<Size> {
    SIZES
        .iter()
        .find(|(sized, _, _)| *sized == id)
        .map(|(_, width, height)| Size {
            width: *width,
            height: *height,
        })
}

/// A thing lying in a grid, its top-left cell at `x`, `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// The thing.
    pub id: &'static str,
    /// Column of its top-left cell.
    pub x: u8,
    /// Row of its top-left cell.
    pub y: u8,
}

impl Placed {
    fn size(self) -> Size {
        size_of(self.id).unwrap_or(Size {
            width: 1,
            height: 1,
        })
    }

    fn overlaps(self, other: Self) -> bool {
        let (a, b) = (self.size(), other.size());
        self.x < other.x + b.width
            && other.x < self.x + a.width
            && self.y < other.y + b.height
            && other.y < self.y + a.height
    }
}

/// One grid of things: a Bond's, or its Avaia's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backpack {
    carry: Carry,
    placed: Vec<Placed>,
}

impl Backpack {
    /// An empty grid of `carry`.
    #[must_use]
    pub const fn new(carry: Carry) -> Self {
        Self {
            carry,
            placed: Vec::new(),
        }
    }

    /// What it is carried in.
    #[must_use]
    pub const fn carry(&self) -> Carry {
        self.carry
    }

    /// Everything in it, in the order it was put in.
    #[must_use]
    pub fn placed(&self) -> &[Placed] {
        &self.placed
    }

    /// How many of `id` it holds.
    #[must_use]
    pub fn count(&self, id: &str) -> u32 {
        let count = self.placed.iter().filter(|placed| placed.id == id).count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    /// Cells taken.
    #[must_use]
    pub fn used_cells(&self) -> u32 {
        self.placed
            .iter()
            .map(|placed| {
                let size = placed.size();
                u32::from(size.width) * u32::from(size.height)
            })
            .sum()
    }

    /// Whether `id` could go with its top-left cell at `x`, `y`.
    #[must_use]
    pub fn fits_at(&self, id: &'static str, x: u8, y: u8) -> bool {
        let (Some(size), grid) = (size_of(id), self.carry.grid()) else {
            return false;
        };
        let candidate = Placed { id, x, y };
        u16::from(x) + u16::from(size.width) <= u16::from(grid.width)
            && u16::from(y) + u16::from(size.height) <= u16::from(grid.height)
            && !self.placed.iter().any(|placed| placed.overlaps(candidate))
    }

    /// Puts `id` with its top-left cell at `x`, `y`.
    ///
    /// # Errors
    ///
    /// [`EconomyError::UnknownItem`] for something that is not a thing,
    /// [`EconomyError::NoRoom`] when it does not fit there.
    pub fn put_at(&mut self, id: &str, x: u8, y: u8) -> Result<Placed, EconomyError> {
        let id = known(id)?;
        if !self.fits_at(id, x, y) {
            return Err(EconomyError::NoRoom);
        }
        let placed = Placed { id, x, y };
        self.placed.push(placed);
        Ok(placed)
    }

    /// Puts `id` in the first place it fits: row by row, then column by
    /// column, from the top left.
    ///
    /// # Errors
    ///
    /// [`EconomyError::UnknownItem`], or [`EconomyError::NoRoom`] when it fits
    /// nowhere.
    pub fn put(&mut self, id: &str) -> Result<Placed, EconomyError> {
        let id = known(id)?;
        let grid = self.carry.grid();
        for y in 0..grid.height {
            for x in 0..grid.width {
                if self.fits_at(id, x, y) {
                    return self.put_at(id, x, y);
                }
            }
        }
        Err(EconomyError::NoRoom)
    }

    /// Takes out the thing whose top-left cell is at `x`, `y`.
    ///
    /// # Errors
    ///
    /// [`EconomyError::Missing`] when nothing starts there.
    pub fn take_at(&mut self, x: u8, y: u8) -> Result<&'static str, EconomyError> {
        let index = self
            .placed
            .iter()
            .position(|placed| placed.x == x && placed.y == y)
            .ok_or(EconomyError::Missing)?;
        Ok(self.placed.remove(index).id)
    }

    /// Takes out `count` of `id`, the last put in first. All or nothing.
    ///
    /// # Errors
    ///
    /// [`EconomyError::Missing`] when fewer are held.
    pub fn take(&mut self, id: &str, count: u32) -> Result<(), EconomyError> {
        if count == 0 || self.count(id) < count {
            return Err(EconomyError::Missing);
        }
        for _ in 0..count {
            if let Some(index) = self.placed.iter().rposition(|placed| placed.id == id) {
                self.placed.remove(index);
            }
        }
        Ok(())
    }

    /// Moves everything into a grid of `carry`. Where everything still fits
    /// as it lies, it stays; otherwise it is repacked, largest first. All or
    /// nothing.
    ///
    /// # Errors
    ///
    /// [`EconomyError::NoRoom`] when the things do not fit the new grid.
    pub fn switch_to(&mut self, carry: Carry) -> Result<(), EconomyError> {
        let mut kept = Self::new(carry);
        if self
            .placed
            .iter()
            .all(|placed| kept.put_at(placed.id, placed.x, placed.y).is_ok())
        {
            *self = kept;
            return Ok(());
        }
        let mut repacked = Self::new(carry);
        let mut things: Vec<_> = self.placed.iter().map(|placed| placed.id).collect();
        things.sort_by_key(|id| {
            let size = size_of(id).unwrap_or(Size {
                width: 1,
                height: 1,
            });
            core::cmp::Reverse(u16::from(size.width) * u16::from(size.height))
        });
        for id in things {
            repacked.put(id)?;
        }
        *self = repacked;
        Ok(())
    }
}

fn known(id: &str) -> Result<&'static str, EconomyError> {
    SIZES
        .iter()
        .find(|(sized, _, _)| *sized == id)
        .map(|(sized, _, _)| *sized)
        .ok_or(EconomyError::UnknownItem)
}

#[cfg(test)]
mod tests {
    use super::{Backpack, Carry, SIZES, size_of};
    use crate::CRAFTED;
    use crate::economy::{EconomyError, found_things, item_kind};

    #[test]
    fn grids_hold_5_40_and_120_cells() {
        let cells: Vec<_> = Carry::ALL.iter().map(|carry| carry.cells()).collect();
        assert_eq!(cells, [5, 40, 120]);
        assert_eq!(Carry::from_code("bag"), Some(Carry::Bag));
    }

    #[test]
    fn every_thing_has_a_size_and_fits_a_backpack() {
        for id in found_things()
            .map(|found| found.id)
            .chain(CRAFTED.iter().map(|kind| kind.id))
        {
            let size = size_of(id).unwrap_or_else(|| panic!("{id} has no size"));
            assert!(Backpack::new(Carry::Backpack).fits_at(id, 0, 0), "{id}");
            assert!(size.width > 0 && size.height > 0, "{id}");
        }
        for (id, _, _) in SIZES {
            assert!(item_kind(id).is_some(), "{id} is not a thing");
        }
        assert_eq!(size_of("small_change"), None);
    }

    #[test]
    fn things_do_not_overlap() {
        let mut backpack = Backpack::new(Carry::Backpack);
        backpack.put_at("reel_to_reel", 0, 0).unwrap();
        assert_eq!(backpack.put_at("can", 2, 2), Err(EconomyError::NoRoom));
        assert_eq!(backpack.put_at("can", 3, 0).map(|placed| placed.x), Ok(3));
        assert_eq!(backpack.put_at("cd_radio", 6, 0), Err(EconomyError::NoRoom));
        assert_eq!(backpack.used_cells(), 10);
    }

    #[test]
    fn a_pocket_fills_up() {
        let mut pocket = Backpack::new(Carry::Pocket);
        for _ in 0..5 {
            pocket.put("can").unwrap();
        }
        assert_eq!(pocket.put("can"), Err(EconomyError::NoRoom));
        assert_eq!(
            Backpack::new(Carry::Pocket).put("bottle"),
            Err(EconomyError::NoRoom),
            "a bottle stands two cells tall"
        );
    }

    #[test]
    fn first_fit_goes_row_by_row() {
        let mut backpack = Backpack::new(Carry::Backpack);
        let first = backpack.put("cassette_player").unwrap();
        let second = backpack.put("cassette_player").unwrap();
        assert_eq!((first.x, first.y, second.x, second.y), (0, 0, 2, 0));
    }

    #[test]
    fn taking_out_frees_the_cells() {
        let mut backpack = Backpack::new(Carry::Pocket);
        backpack.put("can").unwrap();
        backpack.put("can").unwrap();
        assert_eq!(backpack.take_at(0, 0), Ok("can"));
        assert_eq!(backpack.take_at(0, 0), Err(EconomyError::Missing));
        assert_eq!(backpack.take("can", 2), Err(EconomyError::Missing));
        backpack.take("can", 1).unwrap();
        assert_eq!(backpack.used_cells(), 0);
    }

    #[test]
    fn switching_down_repacks_or_refuses() {
        let mut backpack = Backpack::new(Carry::Bag);
        backpack.put_at("can", 11, 9).unwrap();
        backpack.put_at("can", 0, 0).unwrap();
        backpack.switch_to(Carry::Pocket).unwrap();
        assert_eq!(backpack.carry(), Carry::Pocket);
        assert_eq!(backpack.count("can"), 2);

        let mut full = Backpack::new(Carry::Backpack);
        full.put("reel_to_reel").unwrap();
        let before = full.clone();
        assert_eq!(full.switch_to(Carry::Pocket), Err(EconomyError::NoRoom));
        assert_eq!(full, before);
    }
}
