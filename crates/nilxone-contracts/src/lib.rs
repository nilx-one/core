// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Binding-safe contract boundary for 0x1 Core.
//!
//! This crate represents the normative `0.1.0` client contract without defining a
//! production interaction registry. Product semantics remain owned by the canonical
//! `nilx-one/0x1` specification.

mod avaia_drive;
mod avaia_life;
mod avaia_proximity;
mod avaia_pub_dress;
mod backpack;
mod business_point;
mod business_station;
mod canonical;
mod courier_destination;
mod economy;
mod envelope;
mod error;
mod error_report;
mod find_item;
mod geography;
mod identifier;
mod inventory;
mod inventory_wire;
mod orb_spill;
mod pub_dress;
mod pub_dress_label;
mod rarity_v2;
mod rarity_v3;
mod scalar;
mod spoken_line;
mod version;
mod xping_engine;

pub use avaia_drive::{
    AVAIA_DRIVE_VERSION, Action, Activity, Choice, Curious, Describing, Detour, DriveCommand,
    DriveError, DriveInput, DriveKind, DriveRef, DriveState, Entry, Feeling, Group, Home, Line,
    MenuOption, Obstacle, Passing, Pending, Presentation, Purpose, Reach, Resolve, Resume, Stand,
    Target, WalkMemory, avaia_drive_step_wire, next_due, outing_budget_meters, restless_ms,
    restlessness, step as step_drive,
};
pub use avaia_life::{AvaiaLife, LifeActivity, LifeIntent, apply_avaia_life};
pub use avaia_proximity::{
    AvaiaProximity, MAX_COUNTED_ARTIFACTS, MIN_DURATION_DISTANCE_METERS, NEAR_DISTANCE_METERS,
    ProximityLevel, RED_DISTANCE_METERS, REVEAL_DISTANCE_METERS, avaia_proximity,
    avaia_proximity_wire,
};
pub use avaia_pub_dress::{AvaiaPubDress, AvaiaPubDressError};
pub use backpack::{Backpack, Carries, Carry, Placed, Size, size_of};
pub use business_point::{
    BUSINESS_POINT_PENDING_SECONDS, BusinessPoint, BusinessPointAddress, BusinessPointDecision,
    BusinessPointError, BusinessPointIdentity, BusinessPointLocation, BusinessPointStatus,
    BusinessPointVerification, BusinessPointVisibility, apply_business_point_command,
    business_point_view,
};
pub use business_station::{
    MAX_STATION_CAPTION_SCALARS, StationCaption, StationPubDress, StationPubDressError,
    normalize_station_caption,
};
pub use canonical::{CanonicalJsonError, canonical_json};
pub use courier_destination::{
    COURIER_DESTINATION_VERSION, CourierCandidate, CourierDestinationError,
    CourierDestinationRequest, MAX_COURIER_CANDIDATES, MAX_COURIER_POINT_ID_SCALARS,
    courier_destination, courier_destination_wire,
};
pub use economy::{
    CRAFTED, ECONOMY_VERSION, EconomyError, ItemKind, Place, RECIPES, Recipe, SEED_CODE,
    SEED_EMBLEM, found_things, item_kind, recipe,
};
pub use envelope::{
    CommandEnvelope, EffectRequestEnvelope, EventEnvelope, ProjectionEnvelope, TransitionOk,
    TransitionOutcome,
};
pub use error::{CoreError, ErrorCode, ErrorShapeError, InvalidHistoryReason, MissingContextPort};
pub use error_report::{
    ERRORS_PROJECT, ERRORS_PROTOCOL_VERSION, ReportError, Reportable, error_id, errors_v1_event,
    failure_report,
};
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
pub use inventory::{CraftJob, GIFT_LEVEL, Holder, Inventory, Outcome, STARTER_SEEDS};
pub use inventory_wire::{
    apply_inventory_command, backpack_gift_due_wire, economy_catalog_json, find_item_wire,
    picks_up_wire, xping_repair_wire,
};
pub use orb_spill::{
    CELL_ORBS, CellLandmark, CellOrbSpot, CellSpill, DrawnOrb, KnownSpill, LANDMARK_ORBS,
    MIN_TRAIL_METERS, ORB_EXPERIENCE, ORB_LIFETIME_MS, ORB_MAX, ORB_MIN, ORB_PICKUP_METERS,
    ORB_SPILL_VERSION, ORB_STAGGER_MS, OrbError, OrbKind, OrbSpot, OrbView, OrbWorld,
    cell_orb_count, cell_orb_id, cell_orbs, orb_award, orb_count, orb_id, orb_trail, orb_trail_via,
    orb_world, orb_world_wire, trail_start,
};
pub use pub_dress::{PubDress, PubDressError};
pub use pub_dress_label::{
    PUB_DRESS_LABEL_MAX_OCTETS, PUB_DRESS_LABEL_SUFFIX_MAX_LENGTH, PUB_DRESS_UNICODE_VERSION,
    PUB_DRESS_UTS46_IMPLEMENTATION, PubDressLabel, PubDressLabelError, PubDressStem,
};
pub use rarity_v2::RarityV2;
pub use rarity_v3::{RARITY_V3_VERSION, RarityV3};
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
pub use xping_engine::{
    XPING_BREAK_AFTER_EMPTY_MS, XPING_BROKEN_MS, XPING_ENGINE_VERSION, XPING_FORCE_FULL_MS,
    XPING_FULL_ENERGY, XPING_FULL_REPAIR_SEEDS, XPING_RECHARGE_FULL_MS, XpingEngine,
    XpingEngineError, XpingEngineView, xping_engine_command_wire, xping_engine_view,
    xping_engine_view_wire,
};
