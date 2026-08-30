// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Thin `UniFFI` translation boundary for 0x1 Core.
//!
//! These exports report compatibility metadata only. They do not create identity,
//! authority, a Bond, a `BondChain`, reciprocity, or any product state.

/// Returns `valid` or a stable canonical `pub_dress` failure code.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn validate_pub_dress(value: String) -> String {
    match value.parse::<ox1_contracts::PubDress>() {
        Ok(_) => "valid".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

/// Returns the normative Core contract version implemented by this build.
#[must_use]
#[uniffi::export]
pub fn contract_version() -> String {
    ox1_kernel::contract_version().to_owned()
}

/// Returns the version of the canonical synthetic parity corpus.
#[must_use]
#[uniffi::export]
pub fn fixture_corpus_version() -> String {
    ox1_kernel::fixture_corpus_version().to_owned()
}

/// Returns the validated digest of the canonical synthetic parity corpus.
#[must_use]
#[uniffi::export]
pub fn fixture_corpus_digest() -> String {
    ox1_kernel::fixture_corpus_digest().to_owned()
}

uniffi::setup_scaffolding!();

#[cfg(test)]
mod tests {
    use super::{
        contract_version, fixture_corpus_digest, fixture_corpus_version, validate_pub_dress,
    };

    #[test]
    fn uniffi_surface_matches_native_handshake() {
        assert_eq!(contract_version(), ox1_kernel::contract_version());
        assert_eq!(
            fixture_corpus_version(),
            ox1_kernel::fixture_corpus_version()
        );
        assert_eq!(fixture_corpus_digest(), ox1_kernel::fixture_corpus_digest());
    }

    #[test]
    fn uniffi_surface_exposes_canonical_pub_dress_validation() {
        assert_eq!(validate_pub_dress("0x0sky".to_owned()), "valid");
        assert_eq!(validate_pub_dress("0x0Sky".to_owned()), "valid");
        assert_eq!(
            validate_pub_dress("0xgsky".to_owned()),
            "invalid_discriminator"
        );
    }
}
