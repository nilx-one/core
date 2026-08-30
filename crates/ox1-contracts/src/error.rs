// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use crate::{ContractVersion, OperationId};
use core::fmt;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::collections::{BTreeMap, BTreeSet};

/// Stable error-code surface for Core contract `0.1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MalformedEnvelope,
    UnsupportedContractVersion,
    UnknownVariant,
    InvalidIdentifier,
    InvalidParticipants,
    StateRevisionMismatch,
    InvalidTransition,
    TerminalBondChain,
    HistoryRollback,
    HistoryDivergence,
    InvalidHistory,
    UnknownAuthority,
    MissingContext,
}

impl ErrorCode {
    /// Deterministic diagnostic message owned by this code.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::MalformedEnvelope => "Input is not a valid closed 0x1 Core envelope.",
            Self::UnsupportedContractVersion => "Core contract version is unsupported.",
            Self::UnknownVariant => "A closed contract variant is unknown.",
            Self::InvalidIdentifier => "An identifier is not canonical.",
            Self::InvalidParticipants => "A BondChain requires exactly two distinct Bonds.",
            Self::StateRevisionMismatch => "Command state revision does not match supplied state.",
            Self::InvalidTransition => "The requested transition is not valid for current state.",
            Self::TerminalBondChain => {
                "Terminal BondChain state cannot accept a semantic transition."
            }
            Self::HistoryRollback => "Candidate history would roll back accepted local history.",
            Self::HistoryDivergence => "Candidate history diverges from accepted local history.",
            Self::InvalidHistory => "BondChain history is invalid.",
            Self::UnknownAuthority => "Required authority is unavailable or invalid.",
            Self::MissingContext => "Required explicit context is missing.",
        }
    }

    const fn keys(self) -> &'static [&'static str] {
        match self {
            Self::MalformedEnvelope => &[],
            Self::UnsupportedContractVersion => &["requested_version", "supported_version"],
            Self::UnknownVariant => &["surface", "variant"],
            Self::InvalidIdentifier => &["field"],
            Self::InvalidParticipants => &["bond_0_id", "bond_1_id"],
            Self::StateRevisionMismatch => &["expected_revision", "actual_revision"],
            Self::InvalidTransition => &["command_kind", "state"],
            Self::TerminalBondChain => &["bch_id", "outcome"],
            Self::HistoryRollback | Self::HistoryDivergence => {
                &["bch_id", "local_head", "candidate_head"]
            }
            Self::InvalidHistory => &["bch_id", "record_sequence", "reason"],
            Self::UnknownAuthority => &["bond_id", "scope"],
            Self::MissingContext => &["port"],
        }
    }
}

/// Failure returned when a serialized failure envelope violates its closed shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorShapeError(&'static str);

impl fmt::Display for ErrorShapeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for ErrorShapeError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct Details(BTreeMap<String, Option<String>>);

impl Details {
    fn from_pairs<const N: usize>(pairs: [(&str, Option<String>); N]) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    fn validate(&self, code: ErrorCode) -> Result<(), ErrorShapeError> {
        let actual: BTreeSet<&str> = self.0.keys().map(String::as_str).collect();
        let expected: BTreeSet<&str> = code.keys().iter().copied().collect();
        if actual != expected {
            return Err(ErrorShapeError("error details do not match error code"));
        }

        if code == ErrorCode::UnknownVariant {
            self.require_one_of(
                "surface",
                &["command", "event", "effect", "projection", "terminal_outcome", "error"],
            )?;
        }
        if code == ErrorCode::MissingContext {
            self.require_one_of(
                "port",
                &[
                    "clock",
                    "entropy",
                    "identifier_generation",
                    "cryptographic_verification",
                    "capability",
                ],
            )?;
        }
        if code == ErrorCode::InvalidHistory {
            self.require_one_of(
                "reason",
                &[
                    "sequence",
                    "previous_hash",
                    "record_hash",
                    "participants",
                    "canonical_bytes",
                ],
            )?;
        }
        Ok(())
    }

    fn require_one_of(&self, key: &str, allowed: &[&str]) -> Result<(), ErrorShapeError> {
        let Some(Some(value)) = self.0.get(key) else {
            return Err(ErrorShapeError("required error detail is null"));
        };
        if allowed.contains(&value.as_str()) {
            Ok(())
        } else {
            Err(ErrorShapeError("error detail contains an unknown closed value"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCoreError {
    contract_version: ContractVersion,
    operation_id: Option<OperationId>,
    code: ErrorCode,
    message: String,
    details: Details,
}

/// Deterministic failure envelope with code-specific closed details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError(RawCoreError);

impl CoreError {
    fn checked(raw: RawCoreError) -> Result<Self, ErrorShapeError> {
        if raw.message != raw.code.message() {
            return Err(ErrorShapeError("error message does not match error code"));
        }
        raw.details.validate(raw.code)?;
        Ok(Self(raw))
    }

    /// Builds `unsupported_contract_version` with the normative closed details.
    #[must_use]
    pub fn unsupported_contract_version(
        operation_id: Option<OperationId>,
        requested_version: Option<String>,
        supported_version: String,
    ) -> Self {
        Self(RawCoreError {
            contract_version: "0.1.0".parse().expect("static contract version is valid"),
            operation_id,
            code: ErrorCode::UnsupportedContractVersion,
            message: ErrorCode::UnsupportedContractVersion.message().to_owned(),
            details: Details::from_pairs([
                ("requested_version", requested_version),
                ("supported_version", Some(supported_version)),
            ]),
        })
    }

    /// Builds `unknown_variant` with its closed surface and variant details.
    pub fn unknown_variant(
        operation_id: Option<OperationId>,
        surface: &str,
        variant: Option<String>,
    ) -> Result<Self, ErrorShapeError> {
        let raw = RawCoreError {
            contract_version: "0.1.0".parse().expect("static contract version is valid"),
            operation_id,
            code: ErrorCode::UnknownVariant,
            message: ErrorCode::UnknownVariant.message().to_owned(),
            details: Details::from_pairs([
                ("surface", Some(surface.to_owned())),
                ("variant", variant),
            ]),
        };
        Self::checked(raw)
    }

    /// Returns the stable machine-readable error code.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.0.code
    }

    /// Returns the recovered operation identifier, if available.
    #[must_use]
    pub fn operation_id(&self) -> Option<&OperationId> {
        self.0.operation_id.as_ref()
    }
}

impl Serialize for CoreError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CoreError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawCoreError::deserialize(deserializer)?;
        Self::checked(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreError, ErrorCode};

    #[test]
    fn rejects_message_code_mismatch() {
        let encoded = r#"{
            "contract_version":"0.1.0",
            "operation_id":null,
            "code":"malformed_envelope",
            "message":"wrong",
            "details":{}
        }"#;
        assert!(serde_json::from_str::<CoreError>(encoded).is_err());
    }

    #[test]
    fn rejects_unknown_surface() {
        assert!(CoreError::unknown_variant(None, "nearby", Some("x".to_owned())).is_err());
    }

    #[test]
    fn exposes_stable_code() {
        let error = CoreError::unsupported_contract_version(None, None, "0.1.0".to_owned());
        assert_eq!(error.code(), ErrorCode::UnsupportedContractVersion);
    }
}
