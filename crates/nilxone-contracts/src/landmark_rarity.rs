// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! How rare each landmark is: a versioned registry Core owns.
//!
//! A landmark's rarity belongs to that one landmark, keyed by the published
//! archive's stable feature id, not to its kind. A landmark absent from a
//! registry version is Common. Every version stays here, unchanged, once
//! published: an orb id names the version it was laid under, so the same id
//! always prices the same. A revision is a new version appended below.
//!
//! The registry is product data, not evidence of history, heritage or any
//! visit.

use crate::rarity_v3::RarityV3;

/// The registry version the identity service answers new spills with.
pub const LANDMARK_RARITY_REGISTRY_VERSION: u32 = 1;

/// Every published registry, version 1 first. Never edit a published one.
const REGISTRIES: [&[(&str, RarityV3)]; 1] = [
    // v1: nothing curated yet, so every landmark is Common.
    &[],
];

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

pub(crate) fn rarity_in(
    registries: &[&[(&str, RarityV3)]],
    version: u32,
    id: &str,
) -> Option<RarityV3> {
    if !is_landmark_id(id) {
        return None;
    }
    let index = usize::try_from(version.checked_sub(1)?).ok()?;
    let registry = registries.get(index)?;
    Some(
        registry
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
            for (position, (id, _)) in registry.iter().enumerate() {
                assert!(is_landmark_id(id), "{id}");
                assert!(
                    registry[..position].iter().all(|(other, _)| other != id),
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

    #[test]
    fn a_later_version_never_changes_an_earlier_one() {
        let registries: [&[(&str, RarityV3)]; 2] = [&[], &[("lavra", RarityV3::Exclusive)]];
        assert_eq!(rarity_in(&registries, 1, "lavra"), Some(RarityV3::Common));
        assert_eq!(
            rarity_in(&registries, 2, "lavra"),
            Some(RarityV3::Exclusive)
        );
        assert_eq!(rarity_in(&registries, 2, "sofia"), Some(RarityV3::Common));
    }
}
