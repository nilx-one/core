// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! How rare each landmark is: a versioned registry Core owns.
//!
//! A landmark's rarity belongs to that one landmark, keyed by the published
//! archive's stable feature id, not to its kind. A landmark absent from a
//! registry version is Common.
//!
//! A registry version also freezes the orb rules laid under it: the
//! experience each rarity pays, and the rarity roll with its hash domain.
//! Every version stays here, unchanged, once published: an orb id names the
//! version it was laid under, so the same id always rolls and prices the
//! same, whatever later versions change. A revision of a landmark's rarity
//! or of an orb rule is a new version appended below.
//!
//! The registry is product data, not evidence of history, heritage or any
//! visit.

use crate::rarity_v3::RarityV3;

/// The registry version the identity service answers new spills with.
pub const LANDMARK_RARITY_REGISTRY_VERSION: u32 = 1;

/// One published registry version: the landmarks it rates, and the orb
/// rules frozen with it.
#[derive(Clone, Copy)]
pub(crate) struct Registry {
    /// Landmarks rarer than Common, by archive id.
    pub landmarks: &'static [(&'static str, RarityV3)],
    /// What one orb of each rarity pays, Common first.
    pub experience: [u32; 5],
    /// The rarity a uniform draw rolls under a ceiling.
    pub roll: fn(u32, RarityV3) -> RarityV3,
    /// The hash domain the draw is taken in.
    pub roll_domain: &'static str,
}

/// Every published registry, version 1 first. Never edit a published one.
pub(crate) const REGISTRIES: [Registry; 1] = [
    // v1: nothing curated yet, so every landmark is Common. Orbs pay
    // 10 x multiplier and halve in chance per step.
    Registry {
        landmarks: &[],
        experience: [10, 20, 40, 80, 160],
        roll: roll_halving,
        roll_domain: "nilx-one.orbs.rarity.v1:",
    },
];

/// The v1 roll: at least rarity `r` with chance `1 / 2^(tier(r) - 1)`
/// (one in two Uncommon or rarer, one in sixteen Exclusive), never rarer
/// than the ceiling. Published with registry 1; never change it.
#[must_use]
pub fn roll_halving(draw: u32, ceiling: RarityV3) -> RarityV3 {
    let steps = u32::from(ceiling.tier() - 1);
    let step = u8::try_from(draw.leading_zeros().min(steps)).unwrap_or(0);
    RarityV3::from_v3_tier(step + 1).unwrap_or(RarityV3::Common)
}

/// The published registry `version`, if there is one.
pub(crate) fn registry_in(registries: &[Registry], version: u32) -> Option<&Registry> {
    registries.get(usize::try_from(version.checked_sub(1)?).ok()?)
}

/// What one orb of `rarity` laid under registry `version` pays, or `None`
/// for an unpublished version. This is the orb reward table alone; a find's
/// reward is [`RarityV3::base_find_xp`].
#[must_use]
pub fn orb_experience(version: u32, rarity: RarityV3) -> Option<u32> {
    let registry = registry_in(&REGISTRIES, version)?;
    registry
        .experience
        .get(usize::from(rarity.tier() - 1))
        .copied()
}

/// The longest landmark id, in bytes.
pub const LANDMARK_ID_MAX: usize = 64;

/// Whether `id` can name a landmark: 1 to 64 of `A-Z a-z 0-9 . _ -`.
#[must_use]
pub fn is_landmark_id(id: &str) -> bool {
    (1..=LANDMARK_ID_MAX).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Whether registry `version` was published.
#[must_use]
pub fn is_registry_version(version: u32) -> bool {
    (1..=LANDMARK_RARITY_REGISTRY_VERSION).contains(&version)
}

/// The rarity of landmark `id` under registry `version`: Common unless the
/// registry names it. `None` for a version that was never published or an id
/// that cannot name a landmark.
#[must_use]
pub fn landmark_rarity(version: u32, id: &str) -> Option<RarityV3> {
    rarity_in(&REGISTRIES, version, id)
}

pub(crate) fn rarity_in(registries: &[Registry], version: u32, id: &str) -> Option<RarityV3> {
    if !is_landmark_id(id) {
        return None;
    }
    let registry = registry_in(registries, version)?;
    Some(
        registry
            .landmarks
            .iter()
            .find(|(named, _)| *named == id)
            .map_or(RarityV3::Common, |(_, rarity)| *rarity),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_published_registry_names_each_landmark_once_by_a_valid_id() {
        for registry in REGISTRIES {
            let landmarks = registry.landmarks;
            for (position, (id, _)) in landmarks.iter().enumerate() {
                assert!(is_landmark_id(id), "{id}");
                assert!(
                    landmarks[..position].iter().all(|(other, _)| other != id),
                    "{id} twice"
                );
            }
        }
        assert_eq!(
            usize::try_from(LANDMARK_RARITY_REGISTRY_VERSION).expect("version"),
            REGISTRIES.len()
        );
    }

    #[test]
    fn an_unnamed_landmark_is_common_and_an_unknown_version_is_none() {
        assert_eq!(landmark_rarity(1, "n123456"), Some(RarityV3::Common));
        assert_eq!(landmark_rarity(0, "n123456"), None);
        assert_eq!(landmark_rarity(2, "n123456"), None);
        assert_eq!(landmark_rarity(1, ""), None);
        assert_eq!(landmark_rarity(1, "a:b"), None);
        assert_eq!(landmark_rarity(1, &"x".repeat(65)), None);
    }

    /// A later version that rates `lavra` and pays and rolls differently.
    pub(crate) fn revised() -> [Registry; 2] {
        fn always_common(_: u32, _: RarityV3) -> RarityV3 {
            RarityV3::Common
        }
        [
            REGISTRIES[0],
            Registry {
                landmarks: &[("lavra", RarityV3::Exclusive)],
                experience: [15, 30, 60, 120, 240],
                roll: always_common,
                roll_domain: "nilx-one.orbs.rarity.test:",
            },
        ]
    }

    #[test]
    fn a_later_version_never_changes_an_earlier_one() {
        let registries = revised();
        assert_eq!(rarity_in(&registries, 1, "lavra"), Some(RarityV3::Common));
        assert_eq!(
            rarity_in(&registries, 2, "lavra"),
            Some(RarityV3::Exclusive)
        );
        assert_eq!(rarity_in(&registries, 2, "sofia"), Some(RarityV3::Common));
        assert_eq!(
            registry_in(&registries, 1).map(|r| r.experience),
            Some([10, 20, 40, 80, 160])
        );
    }

    #[test]
    fn version_one_pays_ten_times_the_multiplier_and_halves_per_step() {
        let paid: Vec<Option<u32>> = RarityV3::ALL
            .into_iter()
            .map(|rarity| orb_experience(1, rarity))
            .collect();
        let expected: Vec<Option<u32>> = RarityV3::ALL
            .into_iter()
            .map(|rarity| u32::try_from(10 * rarity.price_multiplier()).ok())
            .collect();
        assert_eq!(paid, expected);
        assert_eq!(orb_experience(2, RarityV3::Common), None);
        assert_eq!(roll_halving(0, RarityV3::Exclusive), RarityV3::Exclusive);
        assert_eq!(
            roll_halving(u32::MAX, RarityV3::Exclusive),
            RarityV3::Common
        );
    }
}
