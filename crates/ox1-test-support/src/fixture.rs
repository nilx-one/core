// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use ox1_contracts::{
    BondChainId, BondId, CommandEnvelope, ContractVersion, CoreError, DecimalU64,
    EffectRequestEnvelope, EventEnvelope, InvalidHistoryReason, MissingContextPort, OperationId,
    ProjectionEnvelope, Sha256Digest, TransitionOk, TransitionOutcome, canonical_json,
};
use ox1_kernel::bond_chain::{
    HistoryRelation, classify_history, require_active, validate_participants,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RECORD_DOMAIN: &[u8] = b"0x1:core-fixture-record:v0";

/// Uninhabited fixture context value; arrays of this type can only be empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmptyContextValue {}

/// Synthetic fixture authorization scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureAuthorizationScope {
    #[serde(rename = "fixture.initiate")]
    Initiate,
    #[serde(rename = "fixture.reciprocate")]
    Reciprocate,
}

impl FixtureAuthorizationScope {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Initiate => "fixture.initiate",
            Self::Reciprocate => "fixture.reciprocate",
        }
    }
}

/// Synthetic verified fixture authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureAuthorization {
    pub bond_id: BondId,
    pub scope: FixtureAuthorizationScope,
}

/// Explicit fixture context. No ambient input is permitted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureVerifiedContext {
    pub now_unix_ms: DecimalU64,
    pub generated_bch_id: Option<BondChainId>,
    pub authorizations: Vec<FixtureAuthorization>,
    pub entropy: Vec<EmptyContextValue>,
    pub verifications: Vec<EmptyContextValue>,
}

/// Fixture establishment state, independent of terminality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Establishment {
    Candidate,
    Established,
}

/// Closed synthetic fixture terminal outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureTerminalOutcome {
    Completed,
    Rejected,
    Expired,
    Cancelled,
}

impl FixtureTerminalOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Fixture lifecycle remains orthogonal to establishment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FixtureLifecycle {
    Active,
    Terminal { outcome: FixtureTerminalOutcome },
}

impl FixtureLifecycle {
    const fn is_terminal(self) -> bool {
        matches!(self, Self::Terminal { .. })
    }

    const fn outcome(self) -> Option<FixtureTerminalOutcome> {
        match self {
            Self::Active => None,
            Self::Terminal { outcome } => Some(outcome),
        }
    }
}

/// Complete fixture `BondChain` state used only by the synthetic contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureBondChain {
    pub bch_id: BondChainId,
    pub bond_0_id: BondId,
    pub bond_1_id: BondId,
    pub previous_bch_id: Option<BondChainId>,
    pub establishment: Establishment,
    pub lifecycle: FixtureLifecycle,
    pub expires_at_unix_ms: DecimalU64,
    pub cancellable: bool,
    pub history: Vec<FixtureRecord>,
}

/// Complete fixture state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureState {
    pub state_revision: DecimalU64,
    pub bond_chain: FixtureBondChain,
}

/// Synthetic record kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixtureRecordKind {
    #[serde(rename = "fixture.opened")]
    Opened,
    #[serde(rename = "fixture.accepted")]
    Accepted,
    #[serde(rename = "fixture.rejected")]
    Rejected,
    #[serde(rename = "fixture.expired")]
    Expired,
    #[serde(rename = "fixture.cancelled")]
    Cancelled,
}

/// Body of the opening synthetic record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureOpenedBody {
    pub bond_0_id: BondId,
    pub bond_1_id: BondId,
    pub previous_bch_id: Option<BondChainId>,
    pub expires_at_unix_ms: DecimalU64,
    pub cancellable: bool,
}

/// Exact empty object used by non-opening fixture records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureEmptyBody {}

/// Closed fixture record body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FixtureRecordBody {
    Opened(FixtureOpenedBody),
    Empty(FixtureEmptyBody),
}

/// Synthetic history record with deterministic test-only hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRecord {
    pub contract_version: ContractVersion,
    pub record_kind: FixtureRecordKind,
    pub bch_id: BondChainId,
    pub sequence: DecimalU64,
    pub previous_record_hash: Option<Sha256Digest>,
    pub actor_bond_id: Option<BondId>,
    pub observed_at_unix_ms: DecimalU64,
    pub body: FixtureRecordBody,
    pub record_hash: Sha256Digest,
}

#[derive(Serialize)]
struct HashableRecord<'a> {
    contract_version: ContractVersion,
    record_kind: FixtureRecordKind,
    bch_id: &'a BondChainId,
    sequence: DecimalU64,
    previous_record_hash: Option<&'a Sha256Digest>,
    actor_bond_id: Option<&'a BondId>,
    observed_at_unix_ms: DecimalU64,
    body: &'a FixtureRecordBody,
}

/// Synthetic fixture command registry. This registry is test-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum FixtureCommand {
    #[serde(rename = "fixture.open")]
    Open {
        bond_0_id: BondId,
        bond_1_id: BondId,
        previous_bch_id: Option<BondChainId>,
        expires_at_unix_ms: DecimalU64,
        cancellable: bool,
    },
    #[serde(rename = "fixture.accept")]
    Accept,
    #[serde(rename = "fixture.reject")]
    Reject,
    #[serde(rename = "fixture.expire")]
    Expire,
    #[serde(rename = "fixture.cancel")]
    Cancel,
    #[serde(rename = "fixture.synchronize")]
    Synchronize { candidate_history: Vec<FixtureRecord> },
}

impl FixtureCommand {
    const fn kind(&self) -> &'static str {
        match self {
            Self::Open { .. } => "fixture.open",
            Self::Accept => "fixture.accept",
            Self::Reject => "fixture.reject",
            Self::Expire => "fixture.expire",
            Self::Cancel => "fixture.cancel",
            Self::Synchronize { .. } => "fixture.synchronize",
        }
    }
}

/// Synthetic state-replacement event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum FixtureEvent {
    #[serde(rename = "fixture.state_replaced")]
    StateReplaced { state: FixtureState },
}

/// Synthetic effect request. Dispatch does not create protocol evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum FixtureEffect {
    #[serde(rename = "fixture.persist_record")]
    PersistRecord {
        bch_id: BondChainId,
        record_hash: Sha256Digest,
    },
    #[serde(rename = "fixture.transport_record")]
    TransportRecord {
        bch_id: BondChainId,
        record_hash: Sha256Digest,
    },
}

/// Binding-safe projection subset for the synthetic fixture chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureProjectionBondChain {
    pub bch_id: BondChainId,
    pub bond_0_id: BondId,
    pub bond_1_id: BondId,
    pub previous_bch_id: Option<BondChainId>,
    pub establishment: Establishment,
    pub lifecycle: FixtureLifecycle,
}

/// Synthetic fixture projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum FixtureProjection {
    #[serde(rename = "fixture.bond_chain")]
    BondChain { bond_chain: FixtureProjectionBondChain },
}

/// Complete typed outcome of one synthetic fixture transition.
pub type FixtureTransitionOutcome = TransitionOutcome<FixtureEvent, FixtureEffect, FixtureProjection>;

type FixtureEnvelope = CommandEnvelope<FixtureCommand, FixtureState, FixtureVerifiedContext>;

fn failure(error: CoreError) -> FixtureTransitionOutcome {
    TransitionOutcome::Error { error }
}

fn projection(chain: &FixtureBondChain) -> FixtureProjection {
    FixtureProjection::BondChain {
        bond_chain: FixtureProjectionBondChain {
            bch_id: chain.bch_id.clone(),
            bond_0_id: chain.bond_0_id.clone(),
            bond_1_id: chain.bond_1_id.clone(),
            previous_bch_id: chain.previous_bch_id.clone(),
            establishment: chain.establishment,
            lifecycle: chain.lifecycle,
        },
    }
}

fn next_revision(
    operation_id: &OperationId,
    revision: DecimalU64,
    command_kind: &str,
) -> Result<DecimalU64, CoreError> {
    revision.get().checked_add(1).map(DecimalU64::new).ok_or_else(|| {
        CoreError::invalid_transition(
            Some(operation_id.clone()),
            Some(command_kind.to_owned()),
            Some("state_revision_exhausted".to_owned()),
        )
    })
}

fn has_authorization(
    context: &FixtureVerifiedContext,
    bond_id: &BondId,
    scope: FixtureAuthorizationScope,
) -> bool {
    context
        .authorizations
        .iter()
        .any(|authorization| authorization.bond_id == *bond_id && authorization.scope == scope)
}

fn require_authorization(
    operation_id: &OperationId,
    context: &FixtureVerifiedContext,
    bond_id: &BondId,
    scope: FixtureAuthorizationScope,
) -> Result<(), CoreError> {
    if has_authorization(context, bond_id, scope) {
        Ok(())
    } else {
        Err(CoreError::unknown_authority(
            Some(operation_id.clone()),
            Some(bond_id.to_string()),
            Some(scope.as_str().to_owned()),
        ))
    }
}

fn hash_record(record: &HashableRecord<'_>) -> Result<Sha256Digest, CoreError> {
    let canonical = canonical_json(record).map_err(|_| {
        CoreError::invalid_history(None, None, None, InvalidHistoryReason::CanonicalBytes)
    })?;
    let mut hasher = Sha256::new();
    hasher.update(RECORD_DOMAIN);
    hasher.update([0]);
    hasher.update(canonical);
    Ok(Sha256Digest::from_bytes(hasher.finalize().into()))
}

fn make_record(
    operation_id: &OperationId,
    chain: &FixtureBondChain,
    record_kind: FixtureRecordKind,
    actor_bond_id: Option<BondId>,
    observed_at_unix_ms: DecimalU64,
    body: FixtureRecordBody,
) -> Result<FixtureRecord, CoreError> {
    let sequence = u64::try_from(chain.history.len()).map(DecimalU64::new).map_err(|_| {
        CoreError::invalid_history(
            Some(operation_id.clone()),
            Some(chain.bch_id.to_string()),
            None,
            InvalidHistoryReason::Sequence,
        )
    })?;
    let previous_record_hash = chain.history.last().map(|record| record.record_hash.clone());
    let hashable = HashableRecord {
        contract_version: ContractVersion::CURRENT,
        record_kind,
        bch_id: &chain.bch_id,
        sequence,
        previous_record_hash: previous_record_hash.as_ref(),
        actor_bond_id: actor_bond_id.as_ref(),
        observed_at_unix_ms,
        body: &body,
    };
    let record_hash = hash_record(&hashable)?;
    Ok(FixtureRecord {
        contract_version: ContractVersion::CURRENT,
        record_kind,
        bch_id: chain.bch_id.clone(),
        sequence,
        previous_record_hash,
        actor_bond_id,
        observed_at_unix_ms,
        body,
        record_hash,
    })
}

fn invalid_history(
    operation_id: &OperationId,
    bch_id: Option<&BondChainId>,
    sequence: Option<DecimalU64>,
    reason: InvalidHistoryReason,
) -> CoreError {
    CoreError::invalid_history(
        Some(operation_id.clone()),
        bch_id.map(ToString::to_string),
        sequence.map(|value| value.to_string()),
        reason,
    )
}

fn validate_record_hash(
    operation_id: &OperationId,
    record: &FixtureRecord,
) -> Result<(), CoreError> {
    let hashable = HashableRecord {
        contract_version: record.contract_version,
        record_kind: record.record_kind,
        bch_id: &record.bch_id,
        sequence: record.sequence,
        previous_record_hash: record.previous_record_hash.as_ref(),
        actor_bond_id: record.actor_bond_id.as_ref(),
        observed_at_unix_ms: record.observed_at_unix_ms,
        body: &record.body,
    };
    let computed = hash_record(&hashable).map_err(|_| {
        invalid_history(
            operation_id,
            Some(&record.bch_id),
            Some(record.sequence),
            InvalidHistoryReason::CanonicalBytes,
        )
    })?;
    if computed == record.record_hash {
        Ok(())
    } else {
        Err(invalid_history(
            operation_id,
            Some(&record.bch_id),
            Some(record.sequence),
            InvalidHistoryReason::RecordHash,
        ))
    }
}

fn validate_chain(operation_id: &OperationId, chain: &FixtureBondChain) -> Result<(), CoreError> {
    validate_participants(&chain.bond_0_id, &chain.bond_1_id).map_err(|_| {
        CoreError::invalid_participants(
            Some(operation_id.clone()),
            Some(chain.bond_0_id.to_string()),
            Some(chain.bond_1_id.to_string()),
        )
    })?;

    let Some(first) = chain.history.first() else {
        return Err(invalid_history(
            operation_id,
            Some(&chain.bch_id),
            None,
            InvalidHistoryReason::Sequence,
        ));
    };
    if first.record_kind != FixtureRecordKind::Opened {
        return Err(invalid_history(
            operation_id,
            Some(&chain.bch_id),
            Some(first.sequence),
            InvalidHistoryReason::Sequence,
        ));
    }
    let FixtureRecordBody::Opened(opened) = &first.body else {
        return Err(invalid_history(
            operation_id,
            Some(&chain.bch_id),
            Some(first.sequence),
            InvalidHistoryReason::CanonicalBytes,
        ));
    };
    if first.sequence != DecimalU64::new(0) || first.previous_record_hash.is_some() {
        return Err(invalid_history(
            operation_id,
            Some(&chain.bch_id),
            Some(first.sequence),
            InvalidHistoryReason::Sequence,
        ));
    }
    if first.contract_version != ContractVersion::CURRENT
        || first.bch_id != chain.bch_id
        || first.actor_bond_id.as_ref() != Some(&chain.bond_0_id)
        || opened.bond_0_id != chain.bond_0_id
        || opened.bond_1_id != chain.bond_1_id
        || opened.previous_bch_id != chain.previous_bch_id
        || opened.expires_at_unix_ms != chain.expires_at_unix_ms
        || opened.cancellable != chain.cancellable
    {
        return Err(invalid_history(
            operation_id,
            Some(&chain.bch_id),
            Some(first.sequence),
            InvalidHistoryReason::Participants,
        ));
    }
    validate_record_hash(operation_id, first)?;

    let mut establishment = Establishment::Candidate;
    let mut lifecycle = FixtureLifecycle::Active;
    let mut previous_hash = first.record_hash.clone();

    for (index, record) in chain.history.iter().enumerate().skip(1) {
        let sequence = u64::try_from(index).map(DecimalU64::new).map_err(|_| {
            invalid_history(
                operation_id,
                Some(&chain.bch_id),
                None,
                InvalidHistoryReason::Sequence,
            )
        })?;
        if lifecycle.is_terminal() {
            return Err(invalid_history(
                operation_id,
                Some(&chain.bch_id),
                Some(record.sequence),
                InvalidHistoryReason::Sequence,
            ));
        }
        if record.contract_version != ContractVersion::CURRENT
            || record.bch_id != chain.bch_id
            || record.sequence != sequence
        {
            return Err(invalid_history(
                operation_id,
                Some(&chain.bch_id),
                Some(record.sequence),
                InvalidHistoryReason::Sequence,
            ));
        }
        if record.previous_record_hash.as_ref() != Some(&previous_hash) {
            return Err(invalid_history(
                operation_id,
                Some(&chain.bch_id),
                Some(record.sequence),
                InvalidHistoryReason::PreviousHash,
            ));
        }
        if !matches!(record.body, FixtureRecordBody::Empty(_)) {
            return Err(invalid_history(
                operation_id,
                Some(&chain.bch_id),
                Some(record.sequence),
                InvalidHistoryReason::CanonicalBytes,
            ));
        }

        match record.record_kind {
            FixtureRecordKind::Opened => {
                return Err(invalid_history(
                    operation_id,
                    Some(&chain.bch_id),
                    Some(record.sequence),
                    InvalidHistoryReason::Sequence,
                ));
            }
            FixtureRecordKind::Accepted => {
                if record.actor_bond_id.as_ref() != Some(&chain.bond_1_id)
                    || record.observed_at_unix_ms >= chain.expires_at_unix_ms
                {
                    return Err(invalid_history(
                        operation_id,
                        Some(&chain.bch_id),
                        Some(record.sequence),
                        InvalidHistoryReason::Participants,
                    ));
                }
                establishment = Establishment::Established;
                lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Completed,
                };
            }
            FixtureRecordKind::Rejected => {
                if record.actor_bond_id.as_ref() != Some(&chain.bond_1_id)
                    || record.observed_at_unix_ms >= chain.expires_at_unix_ms
                {
                    return Err(invalid_history(
                        operation_id,
                        Some(&chain.bch_id),
                        Some(record.sequence),
                        InvalidHistoryReason::Participants,
                    ));
                }
                lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Rejected,
                };
            }
            FixtureRecordKind::Expired => {
                if record.actor_bond_id.is_some()
                    || record.observed_at_unix_ms < chain.expires_at_unix_ms
                {
                    return Err(invalid_history(
                        operation_id,
                        Some(&chain.bch_id),
                        Some(record.sequence),
                        InvalidHistoryReason::Participants,
                    ));
                }
                lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Expired,
                };
            }
            FixtureRecordKind::Cancelled => {
                if record.actor_bond_id.as_ref() != Some(&chain.bond_0_id)
                    || record.observed_at_unix_ms >= chain.expires_at_unix_ms
                    || !chain.cancellable
                {
                    return Err(invalid_history(
                        operation_id,
                        Some(&chain.bch_id),
                        Some(record.sequence),
                        InvalidHistoryReason::Participants,
                    ));
                }
                lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Cancelled,
                };
            }
        }
        validate_record_hash(operation_id, record)?;
        previous_hash = record.record_hash.clone();
    }

    if chain.establishment != establishment || chain.lifecycle != lifecycle {
        return Err(invalid_history(
            operation_id,
            Some(&chain.bch_id),
            None,
            InvalidHistoryReason::CanonicalBytes,
        ));
    }
    Ok(())
}

fn require_revision(
    operation_id: &OperationId,
    expected: Option<DecimalU64>,
    state: Option<&FixtureState>,
) -> Result<(), CoreError> {
    let actual = state.map(|value| value.state_revision);
    if expected == actual {
        Ok(())
    } else {
        Err(CoreError::state_revision_mismatch(
            Some(operation_id.clone()),
            expected.map(|value| value.to_string()),
            actual.map(|value| value.to_string()),
        ))
    }
}

fn terminal_error(operation_id: &OperationId, chain: &FixtureBondChain) -> CoreError {
    CoreError::terminal_bond_chain(
        Some(operation_id.clone()),
        Some(chain.bch_id.to_string()),
        chain.lifecycle.outcome().map(|outcome| outcome.as_str().to_owned()),
    )
}

fn success(
    operation_id: OperationId,
    state: FixtureState,
    effects: Vec<FixtureEffect>,
) -> FixtureTransitionOutcome {
    let event = EventEnvelope {
        contract_version: ContractVersion::CURRENT,
        operation_id: operation_id.clone(),
        sequence: DecimalU64::new(0),
        event: FixtureEvent::StateReplaced {
            state: state.clone(),
        },
    };
    let effect_requests = effects
        .into_iter()
        .enumerate()
        .map(|(index, effect)| EffectRequestEnvelope {
            contract_version: ContractVersion::CURRENT,
            operation_id: operation_id.clone(),
            sequence: DecimalU64::new(u64::try_from(index).unwrap_or(u64::MAX)),
            effect,
        })
        .collect();
    let client_projection = ProjectionEnvelope {
        contract_version: ContractVersion::CURRENT,
        operation_id: operation_id.clone(),
        state_revision: state.state_revision,
        projection: projection(&state.bond_chain),
    };
    TransitionOutcome::Ok {
        ok: TransitionOk {
            contract_version: ContractVersion::CURRENT,
            operation_id,
            state_revision: state.state_revision,
            events: vec![event],
            effect_requests,
            client_projection,
        },
    }
}

fn semantic_success(
    operation_id: OperationId,
    mut state: FixtureState,
    record: FixtureRecord,
    establishment: Establishment,
    lifecycle: FixtureLifecycle,
    command_kind: &str,
) -> FixtureTransitionOutcome {
    let revision = match next_revision(&operation_id, state.state_revision, command_kind) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    let bch_id = state.bond_chain.bch_id.clone();
    let record_hash = record.record_hash.clone();
    state.state_revision = revision;
    state.bond_chain.establishment = establishment;
    state.bond_chain.lifecycle = lifecycle;
    state.bond_chain.history.push(record);
    success(
        operation_id,
        state,
        vec![
            FixtureEffect::PersistRecord {
                bch_id: bch_id.clone(),
                record_hash: record_hash.clone(),
            },
            FixtureEffect::TransportRecord {
                bch_id,
                record_hash,
            },
        ],
    )
}

fn run_open(envelope: FixtureEnvelope) -> FixtureTransitionOutcome {
    let FixtureCommand::Open {
        bond_0_id,
        bond_1_id,
        previous_bch_id,
        expires_at_unix_ms,
        cancellable,
    } = envelope.command
    else {
        return failure(CoreError::invalid_transition(
            Some(envelope.operation_id),
            None,
            None,
        ));
    };
    if let Err(error) = require_revision(
        &envelope.operation_id,
        envelope.expected_state_revision,
        envelope.state.as_ref(),
    ) {
        return failure(error);
    }
    if envelope.state.is_some() {
        return failure(CoreError::invalid_transition(
            Some(envelope.operation_id),
            Some("fixture.open".to_owned()),
            Some("existing".to_owned()),
        ));
    }
    if let Err(_error) = validate_participants(&bond_0_id, &bond_1_id) {
        return failure(CoreError::invalid_participants(
            Some(envelope.operation_id),
            Some(bond_0_id.to_string()),
            Some(bond_1_id.to_string()),
        ));
    }
    if expires_at_unix_ms <= envelope.verified_context.now_unix_ms {
        return failure(CoreError::invalid_transition(
            Some(envelope.operation_id),
            Some("fixture.open".to_owned()),
            Some("expired".to_owned()),
        ));
    }
    if let Err(error) = require_authorization(
        &envelope.operation_id,
        &envelope.verified_context,
        &bond_0_id,
        FixtureAuthorizationScope::Initiate,
    ) {
        return failure(error);
    }
    let Some(bch_id) = envelope.verified_context.generated_bch_id.clone() else {
        return failure(CoreError::missing_context(
            Some(envelope.operation_id),
            MissingContextPort::IdentifierGeneration,
        ));
    };
    if previous_bch_id.as_ref() == Some(&bch_id) {
        return failure(CoreError::invalid_transition(
            Some(envelope.operation_id),
            Some("fixture.open".to_owned()),
            Some("self_reference".to_owned()),
        ));
    }

    let mut chain = FixtureBondChain {
        bch_id,
        bond_0_id,
        bond_1_id,
        previous_bch_id,
        establishment: Establishment::Candidate,
        lifecycle: FixtureLifecycle::Active,
        expires_at_unix_ms,
        cancellable,
        history: Vec::new(),
    };
    let body = FixtureRecordBody::Opened(FixtureOpenedBody {
        bond_0_id: chain.bond_0_id.clone(),
        bond_1_id: chain.bond_1_id.clone(),
        previous_bch_id: chain.previous_bch_id.clone(),
        expires_at_unix_ms: chain.expires_at_unix_ms,
        cancellable: chain.cancellable,
    });
    let record = match make_record(
        &envelope.operation_id,
        &chain,
        FixtureRecordKind::Opened,
        Some(chain.bond_0_id.clone()),
        envelope.verified_context.now_unix_ms,
        body,
    ) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    let record_hash = record.record_hash.clone();
    let bch_id = chain.bch_id.clone();
    chain.history.push(record);
    let state = FixtureState {
        state_revision: DecimalU64::new(1),
        bond_chain: chain,
    };
    success(
        envelope.operation_id,
        state,
        vec![
            FixtureEffect::PersistRecord {
                bch_id: bch_id.clone(),
                record_hash: record_hash.clone(),
            },
            FixtureEffect::TransportRecord {
                bch_id,
                record_hash,
            },
        ],
    )
}

fn run_existing(envelope: FixtureEnvelope) -> FixtureTransitionOutcome {
    let operation_id = envelope.operation_id;
    let command_kind = envelope.command.kind();
    let Some(mut state) = envelope.state else {
        return failure(CoreError::state_revision_mismatch(
            Some(operation_id),
            envelope.expected_state_revision.map(|value| value.to_string()),
            None,
        ));
    };
    if let Err(error) = require_revision(
        &operation_id,
        envelope.expected_state_revision,
        Some(&state),
    ) {
        return failure(error);
    }
    if let Err(error) = validate_chain(&operation_id, &state.bond_chain) {
        return failure(error);
    }

    if let FixtureCommand::Synchronize { candidate_history } = envelope.command {
        return run_synchronize(operation_id, state, candidate_history);
    }
    if require_active(state.bond_chain.lifecycle.is_terminal()).is_err() {
        return failure(terminal_error(&operation_id, &state.bond_chain));
    }

    let now = envelope.verified_context.now_unix_ms;
    let (record_kind, actor, establishment, lifecycle) = match envelope.command {
        FixtureCommand::Accept => {
            if now >= state.bond_chain.expires_at_unix_ms {
                return failure(CoreError::invalid_transition(
                    Some(operation_id),
                    Some(command_kind.to_owned()),
                    Some("expired".to_owned()),
                ));
            }
            if let Err(error) = require_authorization(
                &operation_id,
                &envelope.verified_context,
                &state.bond_chain.bond_1_id,
                FixtureAuthorizationScope::Reciprocate,
            ) {
                return failure(error);
            }
            (
                FixtureRecordKind::Accepted,
                Some(state.bond_chain.bond_1_id.clone()),
                Establishment::Established,
                FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Completed,
                },
            )
        }
        FixtureCommand::Reject => {
            if now >= state.bond_chain.expires_at_unix_ms {
                return failure(CoreError::invalid_transition(
                    Some(operation_id),
                    Some(command_kind.to_owned()),
                    Some("expired".to_owned()),
                ));
            }
            if let Err(error) = require_authorization(
                &operation_id,
                &envelope.verified_context,
                &state.bond_chain.bond_1_id,
                FixtureAuthorizationScope::Reciprocate,
            ) {
                return failure(error);
            }
            (
                FixtureRecordKind::Rejected,
                Some(state.bond_chain.bond_1_id.clone()),
                Establishment::Candidate,
                FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Rejected,
                },
            )
        }
        FixtureCommand::Expire => {
            if now < state.bond_chain.expires_at_unix_ms {
                return failure(CoreError::invalid_transition(
                    Some(operation_id),
                    Some(command_kind.to_owned()),
                    Some("active".to_owned()),
                ));
            }
            (
                FixtureRecordKind::Expired,
                None,
                Establishment::Candidate,
                FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Expired,
                },
            )
        }
        FixtureCommand::Cancel => {
            if now >= state.bond_chain.expires_at_unix_ms || !state.bond_chain.cancellable {
                return failure(CoreError::invalid_transition(
                    Some(operation_id),
                    Some(command_kind.to_owned()),
                    Some("not_cancellable".to_owned()),
                ));
            }
            if let Err(error) = require_authorization(
                &operation_id,
                &envelope.verified_context,
                &state.bond_chain.bond_0_id,
                FixtureAuthorizationScope::Initiate,
            ) {
                return failure(error);
            }
            (
                FixtureRecordKind::Cancelled,
                Some(state.bond_chain.bond_0_id.clone()),
                Establishment::Candidate,
                FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Cancelled,
                },
            )
        }
        FixtureCommand::Open { .. } | FixtureCommand::Synchronize { .. } => {
            return failure(CoreError::invalid_transition(
                Some(operation_id),
                Some(command_kind.to_owned()),
                Some("existing".to_owned()),
            ));
        }
    };

    let record = match make_record(
        &operation_id,
        &state.bond_chain,
        record_kind,
        actor,
        now,
        FixtureRecordBody::Empty(FixtureEmptyBody {}),
    ) {
        Ok(value) => value,
        Err(error) => return failure(error),
    };
    semantic_success(
        operation_id,
        state,
        record,
        establishment,
        lifecycle,
        command_kind,
    )
}

fn run_synchronize(
    operation_id: OperationId,
    mut state: FixtureState,
    candidate_history: Vec<FixtureRecord>,
) -> FixtureTransitionOutcome {
    let local_head = state
        .bond_chain
        .history
        .last()
        .map(|record| record.record_hash.to_string());
    let candidate_head = candidate_history
        .last()
        .map(|record| record.record_hash.to_string());
    match classify_history(&state.bond_chain.history, &candidate_history) {
        HistoryRelation::Equal => TransitionOutcome::Ok {
            ok: TransitionOk {
                contract_version: ContractVersion::CURRENT,
                operation_id: operation_id.clone(),
                state_revision: state.state_revision,
                events: Vec::new(),
                effect_requests: Vec::new(),
                client_projection: ProjectionEnvelope {
                    contract_version: ContractVersion::CURRENT,
                    operation_id,
                    state_revision: state.state_revision,
                    projection: projection(&state.bond_chain),
                },
            },
        },
        HistoryRelation::Rollback => failure(CoreError::history_rollback(
            Some(operation_id),
            Some(state.bond_chain.bch_id.to_string()),
            local_head,
            candidate_head,
        )),
        HistoryRelation::Divergence => failure(CoreError::history_divergence(
            Some(operation_id),
            Some(state.bond_chain.bch_id.to_string()),
            local_head,
            candidate_head,
        )),
        HistoryRelation::FastForward { first_new_index } => {
            let mut advanced = state.bond_chain.clone();
            advanced.history = candidate_history.clone();
            let derived = derive_status_from_history(&operation_id, &advanced);
            let (establishment, lifecycle) = match derived {
                Ok(value) => value,
                Err(error) => return failure(error),
            };
            advanced.establishment = establishment;
            advanced.lifecycle = lifecycle;
            if let Err(error) = validate_chain(&operation_id, &advanced) {
                return failure(error);
            }
            let revision = match next_revision(
                &operation_id,
                state.state_revision,
                "fixture.synchronize",
            ) {
                Ok(value) => value,
                Err(error) => return failure(error),
            };
            let effects = candidate_history[first_new_index..]
                .iter()
                .map(|record| FixtureEffect::PersistRecord {
                    bch_id: advanced.bch_id.clone(),
                    record_hash: record.record_hash.clone(),
                })
                .collect();
            state.state_revision = revision;
            state.bond_chain = advanced;
            success(operation_id, state, effects)
        }
    }
}

fn derive_status_from_history(
    operation_id: &OperationId,
    chain: &FixtureBondChain,
) -> Result<(Establishment, FixtureLifecycle), CoreError> {
    let mut probe = chain.clone();
    probe.establishment = Establishment::Candidate;
    probe.lifecycle = FixtureLifecycle::Active;
    if probe.history.len() > 1 {
        let last = probe.history.last().expect("history length checked");
        match last.record_kind {
            FixtureRecordKind::Accepted => {
                probe.establishment = Establishment::Established;
                probe.lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Completed,
                };
            }
            FixtureRecordKind::Rejected => {
                probe.lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Rejected,
                };
            }
            FixtureRecordKind::Expired => {
                probe.lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Expired,
                };
            }
            FixtureRecordKind::Cancelled => {
                probe.lifecycle = FixtureLifecycle::Terminal {
                    outcome: FixtureTerminalOutcome::Cancelled,
                };
            }
            FixtureRecordKind::Opened => {}
        }
    }
    let result = (probe.establishment, probe.lifecycle);
    validate_chain(operation_id, &probe)?;
    Ok(result)
}

/// Executes one typed synthetic fixture transition.
///
/// This function is test support only. It does not register a production command,
/// event, effect, projection, identity, consent, relationship, or authority.
#[must_use]
pub fn run_fixture_transition(envelope: FixtureEnvelope) -> FixtureTransitionOutcome {
    if !envelope.contract_version.accepts_provider(ContractVersion::CURRENT) {
        return failure(CoreError::unsupported_contract_version(
            Some(envelope.operation_id),
            Some(envelope.contract_version.to_string()),
            ContractVersion::CURRENT.to_string(),
        ));
    }
    if matches!(envelope.command, FixtureCommand::Open { .. }) {
        run_open(envelope)
    } else {
        run_existing(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ox1_contracts::{ErrorCode, canonical_json};

    fn bond(hex: char) -> BondId {
        format!("bond_{}", hex.to_string().repeat(64))
            .parse()
            .expect("fixture Bond id")
    }

    fn bch(hex: char) -> BondChainId {
        format!("bch_{}", hex.to_string().repeat(64))
            .parse()
            .expect("fixture BondChain id")
    }

    fn operation(value: u8) -> OperationId {
        format!("op_{value:032x}").parse().expect("fixture operation id")
    }

    fn context(now: u64, generated: Option<BondChainId>, auth: FixtureAuthorization) -> FixtureVerifiedContext {
        FixtureVerifiedContext {
            now_unix_ms: DecimalU64::new(now),
            generated_bch_id: generated,
            authorizations: vec![auth],
            entropy: Vec::new(),
            verifications: Vec::new(),
        }
    }

    fn open_outcome() -> FixtureTransitionOutcome {
        let bond_0 = bond('1');
        let bond_1 = bond('2');
        let chain_id: BondChainId = "bch_000000000000000000000000000000000000000000000000000000000000000a"
            .parse()
            .expect("canonical corpus id");
        run_fixture_transition(CommandEnvelope {
            contract_version: ContractVersion::CURRENT,
            operation_id: operation(1),
            expected_state_revision: None,
            command: FixtureCommand::Open {
                bond_0_id: bond_0.clone(),
                bond_1_id: bond_1,
                previous_bch_id: None,
                expires_at_unix_ms: DecimalU64::new(2000),
                cancellable: true,
            },
            state: None,
            verified_context: context(
                1000,
                Some(chain_id),
                FixtureAuthorization {
                    bond_id: bond_0,
                    scope: FixtureAuthorizationScope::Initiate,
                },
            ),
        })
    }

    fn opened_state() -> FixtureState {
        let TransitionOutcome::Ok { ok } = open_outcome() else {
            panic!("open fixture must succeed");
        };
        let FixtureEvent::StateReplaced { state } = &ok.events[0].event;
        state.clone()
    }

    #[test]
    fn opening_is_unilateral_candidate_with_canonical_record_hash() {
        let state = opened_state();
        assert_eq!(state.bond_chain.establishment, Establishment::Candidate);
        assert_eq!(state.bond_chain.lifecycle, FixtureLifecycle::Active);
        assert_eq!(
            state.bond_chain.history[0].record_hash.as_str(),
            "sha256_3bce5e808f4af30b7b53824fe7b807e483f59d6a5621f0436409b0a4f9f8ba56"
        );
    }

    #[test]
    fn reciprocal_acceptance_establishes_and_completes_atomically() {
        let state = opened_state();
        let bond_1 = state.bond_chain.bond_1_id.clone();
        let outcome = run_fixture_transition(CommandEnvelope {
            contract_version: ContractVersion::CURRENT,
            operation_id: operation(2),
            expected_state_revision: Some(DecimalU64::new(1)),
            command: FixtureCommand::Accept,
            state: Some(state),
            verified_context: context(
                1100,
                None,
                FixtureAuthorization {
                    bond_id: bond_1,
                    scope: FixtureAuthorizationScope::Reciprocate,
                },
            ),
        });
        let TransitionOutcome::Ok { ok } = outcome else {
            panic!("accept fixture must succeed");
        };
        let FixtureEvent::StateReplaced { state } = &ok.events[0].event;
        assert_eq!(state.bond_chain.establishment, Establishment::Established);
        assert_eq!(
            state.bond_chain.lifecycle,
            FixtureLifecycle::Terminal {
                outcome: FixtureTerminalOutcome::Completed
            }
        );
        assert_eq!(
            state.bond_chain.history[1].record_hash.as_str(),
            "sha256_57d134da8d5ade3f05345933555761b01a8c60dbe1175141d84c1734c0ea387d"
        );
    }

    #[test]
    fn terminal_chain_rejects_later_semantic_command() {
        let state = opened_state();
        let bond_1 = state.bond_chain.bond_1_id.clone();
        let accepted = run_fixture_transition(CommandEnvelope {
            contract_version: ContractVersion::CURRENT,
            operation_id: operation(2),
            expected_state_revision: Some(DecimalU64::new(1)),
            command: FixtureCommand::Accept,
            state: Some(state),
            verified_context: context(
                1100,
                None,
                FixtureAuthorization {
                    bond_id: bond_1,
                    scope: FixtureAuthorizationScope::Reciprocate,
                },
            ),
        });
        let TransitionOutcome::Ok { ok } = accepted else {
            panic!("accept fixture must succeed");
        };
        let FixtureEvent::StateReplaced { state } = &ok.events[0].event;
        let bond_0 = state.bond_chain.bond_0_id.clone();
        let rejected = run_fixture_transition(CommandEnvelope {
            contract_version: ContractVersion::CURRENT,
            operation_id: operation(3),
            expected_state_revision: Some(DecimalU64::new(2)),
            command: FixtureCommand::Cancel,
            state: Some(state.clone()),
            verified_context: context(
                1200,
                None,
                FixtureAuthorization {
                    bond_id: bond_0,
                    scope: FixtureAuthorizationScope::Initiate,
                },
            ),
        });
        let TransitionOutcome::Error { error } = rejected else {
            panic!("terminal append must fail");
        };
        assert_eq!(error.code(), ErrorCode::TerminalBondChain);
    }

    #[test]
    fn identical_participants_are_rejected() {
        let same = bond('1');
        let outcome = run_fixture_transition(CommandEnvelope {
            contract_version: ContractVersion::CURRENT,
            operation_id: operation(5),
            expected_state_revision: None,
            command: FixtureCommand::Open {
                bond_0_id: same.clone(),
                bond_1_id: same.clone(),
                previous_bch_id: None,
                expires_at_unix_ms: DecimalU64::new(2000),
                cancellable: true,
            },
            state: None,
            verified_context: context(
                1000,
                Some(bch('a')),
                FixtureAuthorization {
                    bond_id: same,
                    scope: FixtureAuthorizationScope::Initiate,
                },
            ),
        });
        let TransitionOutcome::Error { error } = outcome else {
            panic!("same participants must fail");
        };
        assert_eq!(error.code(), ErrorCode::InvalidParticipants);
    }

    #[test]
    fn deterministic_replay_produces_byte_equivalent_output() {
        let first = canonical_json(&open_outcome()).expect("canonical outcome");
        let second = canonical_json(&open_outcome()).expect("canonical outcome");
        assert_eq!(first, second);
    }
}
