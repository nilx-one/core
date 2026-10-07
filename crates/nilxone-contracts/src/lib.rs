// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Binding-safe contract boundary for 0x1 Core.
//!
//! This crate represents the normative `0.1.0` client contract without defining a
//! production interaction registry. Product semantics remain owned by the canonical
//! `nilx-one/0x1` specification.

mod avaia_life;
mod avaia_pub_dress;
mod backpack;
mod canonical;
mod economy;
mod envelope;
mod error;
mod find_item;
mod geography;
mod identifier;
mod inventory;
mod inventory_wire;
mod pub_dress;
mod pub_dress_label;
mod scalar;
mod spoken_line;
mod version;

pub use avaia_life::{AvaiaLife, LifeActivity, LifeIntent, apply_avaia_life};
pub use avaia_pub_dress::{AvaiaPubDress, AvaiaPubDressError};
pub use backpack::{Backpack, Carries, Carry, Placed, Size, size_of};
pub use canonical::{CanonicalJsonError, canonical_json};
pub use economy::{
    CRAFTED, ECONOMY_VERSION, EconomyError, ItemKind, Place, RECIPES, Recipe, SEED_CODE,
    SEED_EMBLEM, found_things, item_kind, recipe,
};
pub use envelope::{
    CommandEnvelope, EffectRequestEnvelope, EventEnvelope, ProjectionEnvelope, TransitionOk,
    TransitionOutcome,
};
pub use error::{CoreError, ErrorCode, ErrorShapeError, InvalidHistoryReason, MissingContextPort};
pub use find_item::{
    CATALOG as FIND_CATALOG, City, Condition, FIND_CATALOG_VERSION, FindItem, FindItemError,
    FindTier, PickupRarities, Rarity, find_item, item_for_find,
};
pub use geography::{
    BondLocation, BondLocationMode, GEO_COORDINATE_E7_SCALE, GeoCoordinate, GeoCoordinateError,
};
pub use identifier::{
    BondChainId, BondId, IdentifierError, OperationId, Sha256Digest, SpokenLineId,
};
pub use inventory::{CraftJob, GIFT_LEVEL, GIFT_POCKET_CELLS, Holder, Inventory, Outcome};
pub use inventory_wire::{
    apply_inventory_command, backpack_gift_due_wire, economy_catalog_json, find_item_wire,
    picks_up_wire,
};
pub use pub_dress::{PubDress, PubDressError};
pub use pub_dress_label::{
    PUB_DRESS_LABEL_MAX_OCTETS, PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH, PUB_DRESS_UNICODE_VERSION,
    PUB_DRESS_UTS46_IMPLEMENTATION, PubDressLabel, PubDressLabelError, PubDressStem,
};
pub use scalar::{DecimalU64, DecimalU64Error};
pub use spoken_line::{
    EARSHOT_MAX_METERS, EarshotRadius, EarshotRadiusError, SPOKEN_TEXT_MAX_SCALARS, SpokenLine,
    SpokenText, SpokenTextError, distance_meters, within_earshot,
};
pub use version::{ContractVersion, VersionError};

/// Normative Core contract version implemented by this workspace.
pub const CONTRACT_VERSION: &str = "0.1.0";
/// Version of the synthetic cross-runtime fixture corpus.
pub const FIXTURE_CORPUS_VERSION: &str = "0.1.0";
/// Validated digest of the canonical synthetic fixture corpus.
pub const FIXTURE_CORPUS_DIGEST: &str =
    "sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009";
