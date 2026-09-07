// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Thin `UniFFI` translation boundary for 0x1 Core.
//!
//! These exports report compatibility metadata and deterministic contract results.
//! They do not create identity, authority, a Bond, a `BondChain`, reciprocity, or
//! any product state.

fn label_wire(result: Result<String, ox1_contracts::PubDressLabelError>) -> String {
    match result {
        Ok(label) => format!("label:{label}"),
        Err(error) => format!("error:{}", error.code()),
    }
}

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

/// Derives the Core-owned DNS A-label.
///
/// The stable wire form is `label:<a-label>` on success or
/// `error:<pub_dress_label_error_code>` on failure.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn derive_pub_dress_label(value: String) -> String {
    label_wire(
        ox1_contracts::PubDressLabel::stem_from_str(&value).map(|stem| stem.as_str().to_owned()),
    )
}

/// Composes a collision suffix before the Core-owned UTS-46 encoding.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn compose_pub_dress_label(value: String, suffix: String) -> String {
    let result = ox1_contracts::PubDressLabel::stem_from_str(&value)
        .and_then(|stem| ox1_contracts::PubDressLabel::compose(&stem, &suffix))
        .map(|label| label.as_str().to_owned());
    label_wire(result)
}

/// Returns the pinned Unicode version used by the `PubDress` scalar policy.
#[must_use]
#[uniffi::export]
pub fn pub_dress_unicode_version() -> String {
    ox1_contracts::PUB_DRESS_UNICODE_VERSION.to_owned()
}

/// Returns the exact UTS-46 implementation pin owned by Core.
#[must_use]
#[uniffi::export]
pub fn pub_dress_uts46_implementation() -> String {
    ox1_contracts::PUB_DRESS_UTS46_IMPLEMENTATION.to_owned()
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
        compose_pub_dress_label, contract_version, derive_pub_dress_label, fixture_corpus_digest,
        fixture_corpus_version, pub_dress_unicode_version, pub_dress_uts46_implementation,
        validate_pub_dress,
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
        assert_eq!(validate_pub_dress("0x0небо".to_owned()), "valid");
        assert_eq!(
            validate_pub_dress("0xgsky".to_owned()),
            "invalid_discriminator"
        );
    }

    #[test]
    fn uniffi_surface_exposes_core_owned_label_derivation() {
        assert_eq!(
            derive_pub_dress_label("0x0небо".to_owned()),
            "label:xn--0x0-dddt1cj"
        );
        assert_eq!(
            derive_pub_dress_label("0x0Небо".to_owned()),
            "label:xn--0x0-dddt1cj"
        );
        assert_eq!(
            derive_pub_dress_label("0x0🌍".to_owned()),
            "error:disallowed_scalar"
        );
        assert!(
            compose_pub_dress_label("0x0небо".to_owned(), "42".to_owned())
                .starts_with("label:xn--")
        );
        assert_eq!(pub_dress_unicode_version(), "16.0.0");
        assert_eq!(
            pub_dress_uts46_implementation(),
            "idna=1.1.0;idna_adapter=1.1.0;idna_mapping=1.1.0"
        );
    }
}
