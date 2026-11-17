// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Deterministic decorative placement on the world's H3 grid.

use h3o::{CellIndex, LatLng, Resolution};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const DOMAIN: &[u8] = b"nilx-one.procedural-map.v1";
const BASIS_POINTS: u16 = 10_000;

/// Placement weight per candidate site in a given habitat.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct PlacementRule {
    pub kind: u16,
    pub habitat: u16,
    pub weight_bps: u16,
}

/// A stable world-space spawn. Runtime ownership and movement are separate.
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
/// The density profile and habitat revision are world inputs shared by all runtimes.
/// The limit applies to the whole cell, across kinds.
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

    let mut candidates = Vec::new();
    for site in cell_index.children(Resolution::Twelve) {
        let mut digest = Sha256::new();
        digest.update(DOMAIN);
        digest.update(u64::from(site).to_be_bytes());
        digest.update(world_seed.as_bytes());
        let hash = digest.finalize();
        let draw = u32::from_be_bytes([hash[28], hash[29], hash[30], hash[31]]);
        let mut roll = draw % u32::from(BASIS_POINTS);
        for rule in &eligible {
            let weight = u32::from(rule.weight_bps);
            if roll < weight {
                candidates.push((hash, site, rule.kind));
                break;
            }
            roll -= weight;
        }
    }
    candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Ok(candidates
        .into_iter()
        .take(usize::from(limit))
        .map(|(hash, site, kind)| {
            let center = LatLng::from(site);
            Spawn {
                id: format!("map-v1:{site}:{kind}:{hash:x}"),
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
