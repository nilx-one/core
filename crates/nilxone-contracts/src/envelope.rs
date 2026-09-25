// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use crate::{ContractVersion, CoreError, DecimalU64, OperationId};
use serde::{Deserialize, Serialize};

/// Closed command envelope shared by registered transition contracts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope<C, S, X> {
    pub contract_version: ContractVersion,
    pub operation_id: OperationId,
    pub expected_state_revision: Option<DecimalU64>,
    pub command: C,
    pub state: Option<S>,
    pub verified_context: X,
}

/// Closed event envelope emitted by one accepted transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope<E> {
    pub contract_version: ContractVersion,
    pub operation_id: OperationId,
    pub sequence: DecimalU64,
    pub event: E,
}

/// Closed effect-request envelope. Dispatch is not completion evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRequestEnvelope<F> {
    pub contract_version: ContractVersion,
    pub operation_id: OperationId,
    pub sequence: DecimalU64,
    pub effect: F,
}

/// Closed deterministic client projection envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionEnvelope<P> {
    pub contract_version: ContractVersion,
    pub operation_id: OperationId,
    pub state_revision: DecimalU64,
    pub projection: P,
}

/// Successful transition result before the outer `ok` envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionOk<E, F, P> {
    pub contract_version: ContractVersion,
    pub operation_id: OperationId,
    pub state_revision: DecimalU64,
    pub events: Vec<EventEnvelope<E>>,
    pub effect_requests: Vec<EffectRequestEnvelope<F>>,
    pub client_projection: ProjectionEnvelope<P>,
}

/// Closed transition result: exactly one `ok` or `error` member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TransitionOutcome<E, F, P> {
    Ok { ok: TransitionOk<E, F, P> },
    Error { error: CoreError },
}
