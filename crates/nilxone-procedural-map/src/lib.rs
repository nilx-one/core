// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Deterministic decorative placement on the world's H3 grid.

pub mod simulation;

use h3o::{CellIndex, LatLng, Resolution};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const DOMAIN: &[u8] = b"nilx-one.procedural-map.v1";
const BASIS_POINTS: u16 = 10_000;

/// Placement weight per candidate site in a given habitat.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementRule {
    pub kind: u16,
    pub habitat: u16,
    pub weight_bps: u16,
}

/// A stable world-space spawn. Match across runtimes by ID.
/// Coordinates are floating-point and may differ slightly across targets.
#[derive(Debug, PartialEq, Serialize)]
pub struct Spawn {
    pub id: String,
    pub kind: u16,
    pub longitude: f64,
    pub latitude: f64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PlacementError {
    InvalidCell,
    InvalidRules,
}

/// Places entities at resolution-12 child centers of one resolution-9 world cell.
/// All runtimes must use the same habitat and density profile. Each kind draws
/// independently. Competing hits at one site and the shared population cap can
/// still displace placements. Surviving placements keep their IDs.
///
/// # Errors
///
/// Returns an error for a noncanonical cell or for duplicate eligible kinds or
/// eligible weights whose sum exceeds 10,000 basis points.
pub fn generate(
    world_seed: &str,
    cell: &str,
    habitat: u16,
    rules: &[PlacementRule],
    limit: u16,
) -> Result<Vec<Spawn>, PlacementError> {
    let cell_index = cell
        .parse::<CellIndex>()
        .ok()
        .filter(|index| index.resolution() == Resolution::Nine && index.to_string() == cell)
        .ok_or(PlacementError::InvalidCell)?;

    let mut eligible: Vec<_> = rules
        .iter()
        .filter(|rule| rule.habitat == habitat)
        .collect();
    eligible.sort_unstable_by_key(|rule| rule.kind);
    if eligible.windows(2).any(|pair| pair[0].kind == pair[1].kind)
        || eligible
            .iter()
            .try_fold(0_u16, |sum, rule| sum.checked_add(rule.weight_bps))
            .is_none_or(|sum| sum > BASIS_POINTS)
    {
        return Err(PlacementError::InvalidRules);
    }
    if eligible.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }

    let mut seed_digest = Sha256::new();
    seed_digest.update(DOMAIN);
    seed_digest.update(world_seed.as_bytes());
    let seed_hash = seed_digest.finalize();
    let world_id = u128::from_be_bytes(core::array::from_fn(|index| seed_hash[index]));
    let mut candidates = Vec::new();
    for site in cell_index.children(Resolution::Twelve) {
        for rule in &eligible {
            let mut digest = Sha256::new();
            digest.update(DOMAIN);
            digest.update(u64::from(site).to_be_bytes());
            digest.update(rule.kind.to_be_bytes());
            digest.update(seed_hash);
            let hash = digest.finalize();
            let draw = u32::from_be_bytes([hash[28], hash[29], hash[30], hash[31]]);
            if draw % u32::from(BASIS_POINTS) < u32::from(rule.weight_bps) {
                candidates.push((hash, site, rule.kind));
                break;
            }
        }
    }
    candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Ok(candidates
        .into_iter()
        .take(usize::from(limit))
        .map(|(_, site, kind)| {
            let center = LatLng::from(site);
            Spawn {
                id: format!("map-v1:{world_id:032x}:{site}:{kind}"),
                kind,
                longitude: center.lng(),
                latitude: center.lat(),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_inputs_and_pins_spawn_id() {
        let cell = "8928308280fffff";
        let rule = PlacementRule {
            kind: 1,
            habitat: 7,
            weight_bps: 10_000,
        };
        assert!(generate("world", cell, 7, &[rule], 0).unwrap().is_empty());
        let other = PlacementRule {
            kind: 2,
            weight_bps: 1,
            ..rule
        };
        assert_eq!(
            generate("world", cell, 7, &[rule, other], 1),
            Err(PlacementError::InvalidRules)
        );
        for invalid in ["8928308280FFFFF", "8828308281fffff"] {
            assert_eq!(
                generate("world", invalid, 7, &[rule], 1),
                Err(PlacementError::InvalidCell)
            );
        }
        assert_eq!(
            generate("world", cell, 7, &[rule], 1).unwrap()[0].id,
            "map-v1:28091f7e86085b64e281cfbbb07f0d3d:8c28308280d07ff:1"
        );
    }

    #[test]
    fn placement_is_stable_across_rule_order_and_bounded_per_cell() {
        let rules = [
            PlacementRule {
                kind: 1,
                habitat: 7,
                weight_bps: 5000,
            },
            PlacementRule {
                kind: 2,
                habitat: 7,
                weight_bps: 5000,
            },
        ];
        let cell = "8928308280fffff";
        let spawns = generate("world", cell, 7, &rules, 5).unwrap();
        assert_eq!(spawns.len(), 5);
        assert_eq!(
            spawns,
            generate("world", cell, 7, &[rules[1], rules[0]], 5).unwrap()
        );
        assert_ne!(
            spawns,
            generate("another world", cell, 7, &rules, 5).unwrap()
        );
        assert!(generate("world", cell, 8, &rules, 5).unwrap().is_empty());
        assert_eq!(
            generate("world", cell, 7, &[rules[0], rules[0]], 5),
            Err(PlacementError::InvalidRules)
        );
    }
}
