// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Binding-safe contract boundary for 0x1 Core.
//!
//! This crate represents the normative `0.1.0` client contract without defining a
//! production interaction registry. Product semantics remain owned by the canonical
//! `nilx-one/0x1` specification.

mod canonical;
mod envelope;
mod error;
mod identifier;
mod scalar;
mod version;

pub use canonical::{CanonicalJsonError, canonical_json};
pub use envelope::{
    CommandEnvelope, EffectRequestEnvelope, EventEnvelope, ProjectionEnvelope, TransitionOk,
    TransitionOutcome,
};
pub use error::{CoreError, ErrorCode, ErrorShapeError};
pub use identifier::{BondChainId, BondId, IdentifierError, OperationId, Sha256Digest};
pub use scalar::{DecimalU64, DecimalU64Error};
pub use version::{ContractVersion, VersionError};

/// Normative Core contract version implemented by this workspace.
pub const CONTRACT_VERSION: &str = "0.1.0";
/// Version of the synthetic cross-runtime fixture corpus.
pub const FIXTURE_CORPUS_VERSION: &str = "0.1.0";
/// Validated digest of the canonical synthetic fixture corpus.
pub const FIXTURE_CORPUS_DIGEST: &str =
    "sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009";
