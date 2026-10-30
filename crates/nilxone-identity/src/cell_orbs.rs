// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Experience Orbs from an opened cell (`docs/exp-orbs.md`).
//!
//! When a fog cell opens for a Bond, the device sends the cell; the service
//! answers its spill (a key, the centre and the landmarks) and keeps nothing.
//! The key is this service's keyed digest of the Bond and the cell, so it can
//! be recomputed, never chosen: a pick-up names its orb id and its cell, and
//! the service recomputes the key and prices the orb through Core's
//! `rated_orb_award`, which reads the orb's registry, landmark and number
//! from the id alone. Only a digest of a paid orb id is kept, to pay it once.
//!
//! No landmark source is wired yet, so a spill answers no landmarks: the
//! cell's own five orbs, Common, 10 each. An orb id that names a landmark is
//! refused until the service can check that the landmark lies in the cell.

use h3o::{CellIndex, LatLng, Resolution};
use nilxone_contracts::{LANDMARK_RARITY_REGISTRY_VERSION, rated_orb_award};
use sha2::{Digest as _, Sha256};

use crate::SecretDigester;

/// The fog's cell size: `PRESENCE_RESOLUTION` in `@nilx-one/presence-contract`.
pub const CELL_RESOLUTION: Resolution = Resolution::Nine;

const CELL_KEY_DOMAIN: &str = "nilx-one.orbs.cell-key.v1";
const E7: f64 = 10_000_000.0;

/// A fog cell as the device names it: an H3 index of the fog's resolution,
/// in canonical lowercase hex.
pub fn parse_cell(text: &str) -> Option<CellIndex> {
    let cell: CellIndex = text.parse().ok()?;
    (cell.resolution() == CELL_RESOLUTION && cell.to_string() == text).then_some(cell)
}

/// The cell key for `owner` and `cell`: 64 lowercase hex digits only this
/// service can make, and remake.
pub fn cell_key(digester: &SecretDigester, owner: &str, cell: CellIndex) -> String {
    let digest = digester.digest(CELL_KEY_DOMAIN, &format!("{owner}\u{0}{cell}"));
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The centre of a cell, in E7 degrees: `(longitude, latitude)`.
pub fn center_e7(cell: CellIndex) -> (i32, i32) {
    let center = LatLng::from(cell);
    // Both are within ±180° once scaled: they fit an i32.
    #[allow(clippy::cast_possible_truncation, reason = "a degree times 1e7 fits")]
    let to_e7 = |degrees: f64| (degrees * E7).round() as i32;
    (to_e7(center.lng()), to_e7(center.lat()))
}

/// One cell's spill, as the device hands it to Core's `orb_world`.
pub struct CellSpill {
    pub key: String,
    pub center: (i32, i32),
    pub registry: u32,
}

/// The spill of `cell` for `owner`. It names no landmark yet.
pub fn spill(digester: &SecretDigester, owner: &str, cell: CellIndex) -> CellSpill {
    CellSpill {
        key: cell_key(digester, owner, cell),
        center: center_e7(cell),
        registry: LANDMARK_RARITY_REGISTRY_VERSION,
    }
}

/// What a picked-up cell orb pays, or `None` when it is not one of this
/// Bond's orbs of this cell: a bad id, another key, an unpublished registry,
/// or a landmark the service cannot yet place in the cell.
pub fn price(digester: &SecretDigester, owner: &str, cell: CellIndex, orb_id: &str) -> Option<u64> {
    let key = cell_key(digester, owner, cell);
    let rest = orb_id.strip_prefix(&format!("orb:cell:{key}:"))?;
    let mut parts = rest.split(':');
    let (_registry, landmark) = (parts.next()?, parts.next()?);
    if landmark != "~" {
        return None;
    }
    let (_rarity, experience) = rated_orb_award(orb_id).ok()?;
    Some(u64::from(experience))
}

/// What the service keeps of a paid orb: a digest of its id, so it pays once.
pub fn orb_digest(orb_id: &str) -> String {
    Sha256::digest(orb_id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NativeAuthConfig;

    fn digester() -> SecretDigester {
        NativeAuthConfig::new(
            "test-auth-secret-that-is-at-least-thirty-two-bytes",
            "test-password-pepper-that-is-at-least-thirty-two-bytes",
        )
        .expect("valid native auth configuration")
        .secret_digester()
    }

    fn kyiv() -> CellIndex {
        LatLng::new(50.4501, 30.5234)
            .expect("Kyiv")
            .to_cell(CELL_RESOLUTION)
    }

    #[test]
    fn a_cell_is_a_canonical_resolution_nine_index() {
        let text = kyiv().to_string();
        assert_eq!(parse_cell(&text), Some(kyiv()));
        assert!(parse_cell(&text.to_uppercase()).is_none());
        let parent = kyiv().parent(Resolution::Eight).expect("parent");
        assert!(parse_cell(&parent.to_string()).is_none(), "resolution 8");
        assert!(parse_cell("nope").is_none());
        let (longitude, latitude) = center_e7(kyiv());
        assert!(
            (300_000_000..310_000_000).contains(&longitude),
            "{longitude}"
        );
        assert!((500_000_000..510_000_000).contains(&latitude), "{latitude}");
    }

    #[test]
    fn a_key_belongs_to_one_bond_and_one_cell() {
        let digester = digester();
        let key = cell_key(&digester, "0x0sky", kyiv());
        assert_eq!(key.len(), 64);
        assert!(
            key.bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        assert_eq!(key, cell_key(&digester, "0x0sky", kyiv()));
        assert_ne!(key, cell_key(&digester, "0x1sky", kyiv()));
        let neighbour = kyiv()
            .grid_disk::<Vec<_>>(1)
            .into_iter()
            .find(|cell| *cell != kyiv())
            .expect("a neighbour");
        assert_ne!(key, cell_key(&digester, "0x0sky", neighbour));
    }

    #[test]
    fn only_this_bonds_own_cell_orbs_are_priced() {
        let digester = digester();
        let key = cell_key(&digester, "0x0sky", kyiv());
        let own = format!("orb:cell:{key}:1:~:0");
        assert_eq!(price(&digester, "0x0sky", kyiv(), &own), Some(10));
        assert_eq!(price(&digester, "0x1sky", kyiv(), &own), None);
        assert_eq!(
            price(
                &digester,
                "0x0sky",
                kyiv(),
                &format!("orb:cell:{key}:1:~:5")
            ),
            None
        );
        assert_eq!(
            price(
                &digester,
                "0x0sky",
                kyiv(),
                &format!("orb:cell:{key}:2:~:0")
            ),
            None
        );
        // A landmark cannot be placed in the cell yet.
        assert_eq!(
            price(
                &digester,
                "0x0sky",
                kyiv(),
                &format!("orb:cell:{key}:1:n1:0")
            ),
            None
        );
        assert_ne!(
            orb_digest(&own),
            orb_digest(&format!("orb:cell:{key}:1:~:1"))
        );
    }
}
