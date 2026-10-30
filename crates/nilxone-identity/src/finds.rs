// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Chance finds as the service checks them (docs/avaia-outings.md, R3).
//!
//! A port of `rollSegment` from `packages/artifact-contract`: the same seed,
//! the same generator, the same table, so the service rolls exactly the find
//! every client rolls. It is what lets a claim name a real find instead of a
//! sha a client made up. The golden tests below are the TypeScript ones; a
//! change on either side has to change both and raise `ROLL_TABLE_VERSION`.
//!
//! It also prices awards (`award_amount`, the port of `awardAmount`), so a
//! request never carries an amount.

use sha2::{Digest, Sha256};

/// `ROLL_TABLE.version` in `artifact-contract`.
pub const ROLL_TABLE_VERSION: u32 = 1;

/// Tier, experience, and finds per kilometre walked: `ROLL_TABLE.tiers`.
const ROLL_TABLE: [(u8, u64, f64); 6] = [
    (1, 10, 0.25),
    (2, 25, 0.12),
    (3, 60, 0.06),
    (4, 150, 0.03),
    (5, 400, 0.018),
    (6, 1000, 0.01),
];

/// The side of a segment cell, and the length of walk one roll stands for.
const SEGMENT_METERS: f64 = 50.0;

/// A week, in milliseconds.
pub const EPOCH_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// The first Monday of 1970, 00:00 UTC, where epoch 0 starts.
const EPOCH_ORIGIN_MS: i64 = 4 * 24 * 60 * 60 * 1000;

/// Every `artifactSha` is the SHA-256 of this prefix and the `artifactId`.
pub const ARTIFACT_SHA_DOMAIN: &str = "nilx-one.artifact.v1:";

/// The commonest tier that is claimed.
pub const CLAIMED_MIN_TIER: u8 = 4;

/// The rarest tier an Avaia picks up itself.
pub const AVAIA_PICKUP_MAX_TIER: u8 = 3;

/// Seeing a find, any tier.
pub const FIND_SEEN_EXPERIENCE: u64 = 10;

/// The epoch, a week counted from the first Monday of 1970, at `now_ms`.
pub fn epoch_of(now_ms: i64) -> i64 {
    (now_ms - EPOCH_ORIGIN_MS).div_euclid(EPOCH_MS)
}

/// A find as the service rolled it: what a claim may name.
#[derive(Debug, Clone, PartialEq)]
pub struct FindRoll {
    pub artifact_id: String,
    pub epoch: i64,
    pub pack_version: u32,
    pub tier: u8,
    pub experience: u64,
    pub along: f64,
    pub across: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FindError {
    #[error("the pack id must be non-empty and free of ':'")]
    PackId,
    #[error("the pack version must be a positive integer")]
    PackVersion,
    #[error("not an artifact id this service rolls")]
    ArtifactId,
}

/// What `rollSegment` gives for one segment and epoch, or `None` for nothing.
pub fn roll_segment(
    pack_id: &str,
    pack_version: u32,
    epoch: i64,
    row: i64,
    column: i64,
) -> Result<Option<FindRoll>, FindError> {
    if pack_id.is_empty() || pack_id.contains(':') {
        return Err(FindError::PackId);
    }
    if pack_version < 1 {
        return Err(FindError::PackVersion);
    }
    let segment = format!("seg:{row}:{column}");
    let mut random = Mulberry32(xmur3(&format!(
        "{pack_id}:{pack_version}:e{epoch}:{segment}"
    )));
    let draw = random.next();
    let km = SEGMENT_METERS / 1000.0;
    let mut threshold = 0.0;
    for &(tier, experience, per_km) in ROLL_TABLE.iter().rev() {
        threshold += per_km * km;
        if draw < threshold {
            let slot = 0;
            let along = random.next();
            let across = random.next() * 2.0 - 1.0;
            return Ok(Some(FindRoll {
                artifact_id: format!("art:{segment}:e{epoch}:{pack_version}:{slot}"),
                epoch,
                pack_version,
                tier,
                experience,
                along,
                across,
            }));
        }
    }
    Ok(None)
}

/// Rolls the find an `artifactId` names, and answers it only when the roll
/// gives exactly that id: a real find of this pack, not one a client made up.
pub fn roll_artifact(pack_id: &str, artifact_id: &str) -> Result<FindRoll, FindError> {
    let parsed = parse_artifact_id(artifact_id).ok_or(FindError::ArtifactId)?;
    let roll = roll_segment(
        pack_id,
        parsed.pack_version,
        parsed.epoch,
        parsed.row,
        parsed.column,
    )?
    .ok_or(FindError::ArtifactId)?;
    if roll.artifact_id != artifact_id {
        return Err(FindError::ArtifactId);
    }
    Ok(roll)
}

/// The public name of a find: 64 lowercase hex digits.
pub fn artifact_sha(artifact_id: &str) -> String {
    let digest = Sha256::digest(format!("{ARTIFACT_SHA_DOMAIN}{artifact_id}").as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

struct ParsedArtifactId {
    row: i64,
    column: i64,
    epoch: i64,
    pack_version: u32,
}

/// `art:seg:<row>:<column>:e<epoch>:<packVersion>:<slot>`, canonical numbers
/// only, so one find has exactly one id.
fn parse_artifact_id(id: &str) -> Option<ParsedArtifactId> {
    let rest = id.strip_prefix("art:seg:")?;
    let mut parts = rest.split(':');
    let row = canonical_number(parts.next()?)?;
    let column = canonical_number(parts.next()?)?;
    let epoch = canonical_number(parts.next()?.strip_prefix('e')?)?;
    let pack_version = u32::try_from(canonical_number(parts.next()?)?).ok()?;
    let slot = canonical_number(parts.next()?)?;
    if parts.next().is_some() || slot != 0 {
        return None;
    }
    Some(ParsedArtifactId {
        row,
        column,
        epoch,
        pack_version,
    })
}

fn canonical_number(text: &str) -> Option<i64> {
    let value: i64 = text.parse().ok()?;
    (value.to_string() == text).then_some(value)
}

/// What an award is for: `AwardKind` in `progression/commitment.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwardKind {
    ZoneRevealed,
    ZoneWalked,
    LandmarkStudied,
    LandmarkNoticed,
    FindSeen,
    FindPickedUp,
    /// A repair or a craft finished (`docs/economy.md` in core): the Bond's
    /// own doing, priced by its recipe.
    CraftFinished,
    /// One orb of a spill, picked up by whoever reached it first.
    OrbPickedUp,
    /// One orb of a cell that opened for the Bond (`docs/exp-orbs.md`),
    /// priced by its orb id: `crate::cell_orbs::price`.
    CellOrbPickedUp,
}

impl AwardKind {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "zone_revealed" => Self::ZoneRevealed,
            "zone_walked" => Self::ZoneWalked,
            "landmark_studied" => Self::LandmarkStudied,
            "landmark_noticed" => Self::LandmarkNoticed,
            "find_seen" => Self::FindSeen,
            "find_picked_up" => Self::FindPickedUp,
            "craft_finished" => Self::CraftFinished,
            "orb_picked_up" => Self::OrbPickedUp,
            "cell_orb_picked_up" => Self::CellOrbPickedUp,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZoneRevealed => "zone_revealed",
            Self::ZoneWalked => "zone_walked",
            Self::LandmarkStudied => "landmark_studied",
            Self::LandmarkNoticed => "landmark_noticed",
            Self::FindSeen => "find_seen",
            Self::FindPickedUp => "find_picked_up",
            Self::CraftFinished => "craft_finished",
            Self::OrbPickedUp => "orb_picked_up",
            Self::CellOrbPickedUp => "cell_orb_picked_up",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Earner {
    Bond,
    Avaia,
}

/// `EXPERIENCE_UNIT` in `progression.ts`; the prices below are its multiples.
const EXPERIENCE_UNIT: u64 = 10;

/// What an award pays, or `None` when the earner cannot earn that kind: the
/// port of `awardAmount`. A request never names the amount.
pub fn award_amount(kind: AwardKind, earner: Earner, tier: Option<u8>) -> Option<u64> {
    if kind != AwardKind::FindPickedUp && tier.is_some() {
        return None;
    }
    match (kind, earner) {
        (AwardKind::ZoneRevealed, Earner::Avaia) => Some(EXPERIENCE_UNIT),
        (AwardKind::ZoneWalked, Earner::Bond) => Some(EXPERIENCE_UNIT * 3),
        (AwardKind::LandmarkStudied, Earner::Avaia) => Some(EXPERIENCE_UNIT * 9 / 2),
        (AwardKind::LandmarkNoticed, Earner::Bond) => Some(EXPERIENCE_UNIT * 2),
        (AwardKind::FindSeen, _) => Some(FIND_SEEN_EXPERIENCE),
        (AwardKind::OrbPickedUp, _) => Some(ORB_EXPERIENCE),
        // Priced by its recipe: `craft_award`; by its orb id: `cell_orbs`.
        (AwardKind::CraftFinished | AwardKind::CellOrbPickedUp, _) => None,
        (AwardKind::FindPickedUp, earner) => {
            let tier = tier?;
            if earner == Earner::Avaia && tier > AVAIA_PICKUP_MAX_TIER {
                return None;
            }
            ROLL_TABLE
                .iter()
                .find(|(t, _, _)| *t == tier)
                .map(|(_, experience, _)| *experience)
        }
        _ => None,
    }
}

/// What picking up a find of `tier` pays, by the item it is (Core's
/// catalog, `docs/find-items.md` in `nilx-one/core`).
///
/// A claimed pick-up (tier 4 and up) names its find, so the item is Core's
/// pick for that artifact: a rare CD radio pays 25, a rare CD player 400. A
/// common pick-up names nothing, and needs nothing: every item a common tier
/// can be pays that tier's amount, which a test here holds Core's catalog
/// to. The tier amount is `award_amount`'s.
pub fn pick_up_amount(artifact_id: Option<&str>, tier: u8) -> Option<u64> {
    match artifact_id {
        Some(artifact_id) => {
            let tier = nilxone_contracts::FindTier::new(tier)?;
            nilxone_contracts::item_for_find(artifact_id, tier)
                .ok()
                .map(|item| u64::from(item.experience))
        }
        None => ROLL_TABLE
            .iter()
            .find(|(t, _, _)| *t == tier)
            .map(|(_, experience, _)| *experience),
    }
}

/// What one orb pays: Core's `ORB_EXPERIENCE`.
pub const ORB_EXPERIENCE: u64 = nilxone_contracts::ORB_EXPERIENCE as u64;

/// How long a spill lies on the ground: Core's `ORB_LIFETIME_MS`.
pub const ORB_LIFETIME_MS: i64 = nilxone_contracts::ORB_LIFETIME_MS as i64;

/// How many orbs a find spills, as Core counts them, or `None` for an id
/// Core does not read as a find.
pub fn orb_count(artifact_id: &str) -> Option<u8> {
    nilxone_contracts::orb_count(artifact_id).ok()
}

/// The pack the service rolls claimed finds with. A claim cannot choose its
/// pack: if it could, any segment could be made to roll a rare find. It has
/// to equal the pack id the client rolls with once finds ship.
pub const FIND_PACK_ID: &str = "nilx-one.finds";

/// How long into a new week a pick-up of last week's find is still taken:
/// one that was pending offline when the week turned.
pub const CLAIM_GRACE_MS: i64 = 24 * 60 * 60 * 1000;

/// The most awards of one kind (and, for a pick-up, one tier) a Bond may have
/// accepted in a week. A bound on a client inventing awards that follow the
/// rules, set at two to three times what the most a person and an Avaia can
/// walk in a week (some 150 km and 210 km) rolls. Not proof of play.
pub fn weekly_cap(kind: AwardKind, tier: Option<u8>) -> u32 {
    match (kind, tier) {
        (AwardKind::ZoneRevealed | AwardKind::ZoneWalked, _) => 2_000,
        (AwardKind::LandmarkStudied, _) => 300,
        (AwardKind::LandmarkNoticed, _) => 500,
        (AwardKind::FindSeen, _) => 400,
        (AwardKind::FindPickedUp, Some(1)) => 200,
        (AwardKind::FindPickedUp, Some(2)) => 100,
        (AwardKind::FindPickedUp, Some(3)) => 50,
        (AwardKind::FindPickedUp, Some(4)) => 15,
        (AwardKind::FindPickedUp, Some(5)) => 10,
        (AwardKind::FindPickedUp, Some(6)) => 6,
        (AwardKind::FindPickedUp, _) => 0,
        // Everyday crafts take 15 to 45 minutes, one at a time: a week holds
        // under 700. A legendary one takes a week, or real money; its bucket
        // (see `craft_cap_bucket`) allows a few.
        (AwardKind::CraftFinished, Some(LEGENDARY_CRAFT_BUCKET)) => 3,
        (AwardKind::CraftFinished, _) => 300,
        // Thirty at most per find, and a find spills once a week.
        (AwardKind::OrbPickedUp, _) => 1_500,
        // Experience, not money: no weekly cap. Each orb id pays once.
        (AwardKind::CellOrbPickedUp, _) => u32::MAX,
    }
}

/// The weekly-cap bucket a legendary craft counts in. Crafts carry no tier;
/// the bucket only keeps a legendary craft from sharing the everyday cap.
pub const LEGENDARY_CRAFT_BUCKET: u8 = 6;

/// What a finished craft pays and the cap bucket it counts in, by Core's
/// recipe, or `None` for a recipe Core does not have.
pub fn craft_award(recipe: &str) -> Option<(u64, u8)> {
    let recipe = nilxone_contracts::recipe(recipe)?;
    let bucket = if recipe.legendary {
        LEGENDARY_CRAFT_BUCKET
    } else {
        0
    };
    Some((u64::from(recipe.experience), bucket))
}

/// Whether a pick-up of a find rolled in `find_epoch` may still be claimed at
/// `now_ms`: this week's, or last week's during the grace.
pub fn claimable_epoch(find_epoch: i64, now_ms: i64) -> bool {
    let current = epoch_of(now_ms);
    find_epoch == current
        || (find_epoch == current - 1 && epoch_of(now_ms - CLAIM_GRACE_MS) == find_epoch)
}

/// `xmur3` in `artifact-contract`: a string to a 32-bit seed. It hashes UTF-16
/// code units, as `charCodeAt` does.
fn xmur3(text: &str) -> u32 {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut h: u32 = 1_779_033_703 ^ (units.len() as u32);
    for unit in units {
        h = (h ^ u32::from(unit)).wrapping_mul(3_432_918_353);
        h = h.rotate_left(13);
    }
    h = (h ^ (h >> 16)).wrapping_mul(2_246_822_507);
    h = (h ^ (h >> 13)).wrapping_mul(3_266_489_909);
    h ^ (h >> 16)
}

/// `mulberry32` in `artifact-contract`, uniform in [0, 1).
struct Mulberry32(u32);

impl Mulberry32 {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_craft_pays_its_recipe() {
        assert_eq!(craft_award("repair_cd_player"), Some((50, 0)));
        assert_eq!(
            craft_award("craft_kyiv_anthology"),
            Some((500, LEGENDARY_CRAFT_BUCKET))
        );
        assert_eq!(craft_award("nothing"), None);
        assert_eq!(
            award_amount(AwardKind::CraftFinished, Earner::Bond, None),
            None
        );
        assert!(weekly_cap(AwardKind::CraftFinished, Some(LEGENDARY_CRAFT_BUCKET)) < 10);
    }

    #[test]
    fn common_tiers_pay_the_same_for_every_item() {
        for item in nilxone_contracts::FIND_CATALOG {
            let tier = item.tier.get();
            if tier < CLAIMED_MIN_TIER {
                assert_eq!(
                    pick_up_amount(None, tier),
                    Some(u64::from(item.experience)),
                    "{} would need its artifact id to be priced",
                    item.id
                );
            }
        }
    }

    #[test]
    fn a_claimed_pick_up_pays_by_its_item() {
        // Core's golden picks: a Kyiv find at tier 5 is a CD radio, at tier 6
        // a test pressing.
        let kyiv = "art:seg:312346:298243:e2908:1:0";
        assert_eq!(pick_up_amount(Some(kyiv), 5), Some(25));
        assert_eq!(pick_up_amount(Some(kyiv), 6), Some(1000));
        assert_eq!(pick_up_amount(Some("art:nope"), 5), None);
        assert_eq!(pick_up_amount(None, 5), Some(400));
    }

    // Core's golden counts (`orb_spill.rs`): the service claims against them.
    #[test]
    fn counts_orbs_as_core_does() {
        let counts: Vec<Option<u8>> = (0..5)
            .map(|row| orb_count(&format!("art:seg:{}:298243:e2908:1:0", 312_346 + row)))
            .collect();
        assert_eq!(counts, [Some(20), Some(23), Some(14), Some(7), Some(27)]);
        assert_eq!(
            award_amount(AwardKind::OrbPickedUp, Earner::Avaia, None),
            Some(ORB_EXPERIENCE)
        );
        assert_eq!(
            award_amount(AwardKind::OrbPickedUp, Earner::Bond, Some(1)),
            None
        );
    }

    // The golden snapshot from `artifact-contract`'s index.test.ts.
    #[test]
    fn matches_the_typescript_golden_rolls() {
        let mut found = Vec::new();
        for r in 0..4000 {
            if found.len() == 6 {
                break;
            }
            if let Some(roll) = roll_segment("golden", 1, 2900, 3_000_000 + r, 7000).unwrap() {
                found.push(roll);
            }
        }
        let expected: [(&str, u8, u64, f64, f64); 6] = [
            (
                "art:seg:3000042:7000:e2900:1:0",
                1,
                10,
                0.9053420010022819,
                -0.7218878073617816,
            ),
            (
                "art:seg:3000106:7000:e2900:1:0",
                1,
                10,
                0.575726603390649,
                -0.4965639994479716,
            ),
            (
                "art:seg:3000134:7000:e2900:1:0",
                5,
                400,
                0.062402204843237996,
                -0.9192713284865022,
            ),
            (
                "art:seg:3000139:7000:e2900:1:0",
                1,
                10,
                0.6837211933452636,
                0.9136116416193545,
            ),
            (
                "art:seg:3000156:7000:e2900:1:0",
                1,
                10,
                0.009048925479874015,
                0.24597822595387697,
            ),
            (
                "art:seg:3000269:7000:e2900:1:0",
                1,
                10,
                0.4986262572929263,
                -0.797026711050421,
            ),
        ];
        assert_eq!(ROLL_TABLE_VERSION, 1);
        assert_eq!(found.len(), expected.len());
        for (roll, (id, tier, experience, along, across)) in found.iter().zip(expected) {
            assert_eq!(roll.artifact_id, id);
            assert_eq!(roll.tier, tier);
            assert_eq!(roll.experience, experience);
            assert_eq!(roll.along, along);
            assert_eq!(roll.across, across);
        }
    }

    // Tier counts over 20,000 segments, computed by the TypeScript roll.
    #[test]
    fn agrees_with_the_typescript_roll_over_many_segments() {
        let mut counts = [0u32; 7];
        let mut rare = Vec::new();
        for r in 0..20_000 {
            match roll_segment("golden", 1, 2961, 312_000 + r, 298_243).unwrap() {
                None => counts[0] += 1,
                Some(roll) => {
                    counts[usize::from(roll.tier)] += 1;
                    if roll.tier >= CLAIMED_MIN_TIER && rare.len() < 4 {
                        rare.push((roll.artifact_id, roll.tier));
                    }
                }
            }
        }
        assert_eq!(counts, [19542, 243, 114, 50, 29, 15, 7]);
        assert_eq!(
            rare,
            [
                ("art:seg:313424:298243:e2961:1:0".to_owned(), 4),
                ("art:seg:313806:298243:e2961:1:0".to_owned(), 5),
                ("art:seg:314071:298243:e2961:1:0".to_owned(), 4),
                ("art:seg:314134:298243:e2961:1:0".to_owned(), 4),
            ]
        );
    }

    #[test]
    fn rolls_a_named_find_only_when_it_exists() {
        let roll = roll_artifact("golden", "art:seg:313806:298243:e2961:1:0").unwrap();
        assert_eq!((roll.tier, roll.experience, roll.epoch), (5, 400, 2961));
        // A segment that rolls nothing, another week, a slot that is not
        // there, and ids that are not canonical.
        for id in [
            "art:seg:313807:298243:e2961:1:0",
            "art:seg:313806:298243:e2962:1:0",
            "art:seg:313806:298243:e2961:1:1",
            "art:seg:0313806:298243:e2961:1:0",
            "art:seg:+313806:298243:e2961:1:0",
            "art:seg:313806:298243:e2961:1:0:0",
            "art:seg:313806:298243:2961:1:0",
            "seg:313806:298243",
        ] {
            assert_eq!(
                roll_artifact("golden", id),
                Err(FindError::ArtifactId),
                "{id}"
            );
        }
        assert_eq!(
            roll_artifact("other", "art:seg:313806:298243:e2961:1:0").map(|roll| roll.tier),
            roll_segment("other", 1, 2961, 313_806, 298_243)
                .unwrap()
                .filter(|roll| roll.artifact_id == "art:seg:313806:298243:e2961:1:0")
                .map(|roll| roll.tier)
                .ok_or(FindError::ArtifactId)
        );
    }

    #[test]
    fn refuses_a_pack_it_cannot_seed() {
        assert_eq!(roll_segment("", 1, 1, 1, 1), Err(FindError::PackId));
        assert_eq!(roll_segment("a:b", 1, 1, 1, 1), Err(FindError::PackId));
        assert_eq!(roll_segment("a", 0, 1, 1, 1), Err(FindError::PackVersion));
    }

    #[test]
    fn names_a_find_as_every_client_does() {
        // The golden sha from `artifact-contract`'s claim.test.ts.
        assert_eq!(
            artifact_sha("art:seg:312346:298243:e2961:1:0"),
            "3366f9fe9be8b666ca9f8bd76b66db632526f4ec5b394e791ee1411d8053d3a6"
        );
    }

    #[test]
    fn counts_weeks_from_the_first_monday_of_1970() {
        // 2026-10-05 is a Monday: epoch 2961 starts at its midnight, UTC.
        let monday = 1_791_158_400_000;
        assert_eq!(epoch_of(monday), 2961);
        assert_eq!(epoch_of(monday - 1), 2960);
        assert_eq!(epoch_of(monday + EPOCH_MS - 1), 2961);
        assert_eq!(epoch_of(0), -1);
    }

    #[test]
    fn prices_awards_as_the_client_does() {
        use AwardKind::*;
        use Earner::*;
        assert_eq!(award_amount(ZoneRevealed, Avaia, None), Some(10));
        assert_eq!(award_amount(ZoneWalked, Bond, None), Some(30));
        assert_eq!(award_amount(LandmarkStudied, Avaia, None), Some(45));
        assert_eq!(award_amount(LandmarkNoticed, Bond, None), Some(20));
        assert_eq!(award_amount(FindSeen, Avaia, None), Some(10));
        assert_eq!(award_amount(FindSeen, Bond, None), Some(10));
        assert_eq!(award_amount(FindPickedUp, Bond, Some(6)), Some(1000));
        assert_eq!(award_amount(FindPickedUp, Avaia, Some(3)), Some(60));

        assert_eq!(award_amount(ZoneRevealed, Bond, None), None);
        assert_eq!(award_amount(ZoneWalked, Avaia, None), None);
        assert_eq!(award_amount(LandmarkStudied, Bond, None), None);
        assert_eq!(award_amount(LandmarkNoticed, Avaia, None), None);
        assert_eq!(award_amount(FindPickedUp, Avaia, Some(4)), None);
        assert_eq!(award_amount(FindPickedUp, Bond, None), None);
        assert_eq!(award_amount(FindPickedUp, Bond, Some(7)), None);
        assert_eq!(award_amount(ZoneWalked, Bond, Some(2)), None);
    }

    #[test]
    fn takes_last_weeks_find_only_on_the_first_day() {
        let monday = 1_791_158_400_000;
        assert!(claimable_epoch(2961, monday));
        assert!(claimable_epoch(2960, monday));
        assert!(claimable_epoch(2960, monday + CLAIM_GRACE_MS - 1));
        assert!(!claimable_epoch(2960, monday + CLAIM_GRACE_MS));
        assert!(!claimable_epoch(2962, monday));
        assert!(!claimable_epoch(2959, monday));
    }

    #[test]
    fn caps_rare_pick_ups_tighter_than_common_ones() {
        let caps: Vec<u32> = (1..=6)
            .map(|tier| weekly_cap(AwardKind::FindPickedUp, Some(tier)))
            .collect();
        assert!(caps.windows(2).all(|pair| pair[0] > pair[1]));
        assert_eq!(weekly_cap(AwardKind::FindPickedUp, None), 0);
    }

    #[test]
    fn reads_back_every_kind_it_writes() {
        for kind in [
            AwardKind::ZoneRevealed,
            AwardKind::ZoneWalked,
            AwardKind::LandmarkStudied,
            AwardKind::LandmarkNoticed,
            AwardKind::FindSeen,
            AwardKind::FindPickedUp,
        ] {
            assert_eq!(AwardKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(AwardKind::parse("amount"), None);
    }
}
