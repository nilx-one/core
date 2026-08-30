// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Deterministic semantic kernel for 0x1 Core.
//!
//! Phase 0 exposes the versioned representation shell and explicit external ports.
//! Production interaction registries remain empty; no protocol state machine is
//! introduced by this change.

pub mod ports;

use ox1_contracts::{
    CONTRACT_VERSION, ContractVersion, CoreError, FIXTURE_CORPUS_DIGEST, FIXTURE_CORPUS_VERSION,
    OperationId,
};

/// Reports the normative Core representation contract implemented by this build.
#[must_use]
pub const fn contract_version() -> &'static str {
    CONTRACT_VERSION
}

/// Reports the synthetic parity corpus version.
#[must_use]
pub const fn fixture_corpus_version() -> &'static str {
    FIXTURE_CORPUS_VERSION
}

/// Reports the validated synthetic parity corpus digest.
#[must_use]
pub const fn fixture_corpus_digest() -> &'static str {
    FIXTURE_CORPUS_DIGEST
}

/// Validates the directional compatibility rule before a transition is decoded.
pub fn require_compatible_contract(
    requested: &str,
    operation_id: Option<OperationId>,
) -> Result<ContractVersion, CoreError> {
    let supported: ContractVersion = CONTRACT_VERSION
        .parse()
        .expect("static Core contract version must be canonical");
    let Ok(required) = requested.parse::<ContractVersion>() else {
        return Err(CoreError::unsupported_contract_version(
            operation_id,
            None,
            CONTRACT_VERSION.to_owned(),
        ));
    };

    if required.accepts_provider(supported) {
        Ok(required)
    } else {
        Err(CoreError::unsupported_contract_version(
            operation_id,
            Some(required.to_string()),
            CONTRACT_VERSION.to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        contract_version, fixture_corpus_digest, fixture_corpus_version,
        require_compatible_contract,
    };
    use ox1_contracts::ErrorCode;

    #[test]
    fn handshake_matches_normative_contract() {
        assert_eq!(contract_version(), "0.1.0");
        assert_eq!(fixture_corpus_version(), "0.1.0");
        assert_eq!(
            fixture_corpus_digest(),
            "sha256_d8524ee7a22aa07164362afb4098cf37404f61ab45fcfd48aab2de2fe9016009"
        );
    }

    #[test]
    fn rejects_unknown_compatibility_line() {
        let error = require_compatible_contract("0.2.0", None).expect_err("0.2 is incompatible");
        assert_eq!(error.code(), ErrorCode::UnsupportedContractVersion);
    }

    #[test]
    fn rejects_noncanonical_version_without_echoing_it_as_canonical() {
        let error = require_compatible_contract("0.01.0", None).expect_err("version is malformed");
        assert_eq!(error.code(), ErrorCode::UnsupportedContractVersion);
    }
}
