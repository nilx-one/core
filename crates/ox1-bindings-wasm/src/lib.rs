// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Thin WebAssembly translation boundary for 0x1 Core.
//!
//! These exports report compatibility metadata only. They do not create identity,
//! authority, a Bond, a BondChain, reciprocity, or any product state.

use wasm_bindgen::prelude::wasm_bindgen;

/// Returns the normative Core contract version implemented by this build.
#[must_use]
#[wasm_bindgen]
pub fn contract_version() -> String {
    ox1_kernel::contract_version().to_owned()
}

/// Returns the version of the canonical synthetic parity corpus.
#[must_use]
#[wasm_bindgen]
pub fn fixture_corpus_version() -> String {
    ox1_kernel::fixture_corpus_version().to_owned()
}

/// Returns the validated digest of the canonical synthetic parity corpus.
#[must_use]
#[wasm_bindgen]
pub fn fixture_corpus_digest() -> String {
    ox1_kernel::fixture_corpus_digest().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{contract_version, fixture_corpus_digest, fixture_corpus_version};

    #[test]
    fn wasm_surface_matches_native_handshake() {
        assert_eq!(contract_version(), ox1_kernel::contract_version());
        assert_eq!(fixture_corpus_version(), ox1_kernel::fixture_corpus_version());
        assert_eq!(fixture_corpus_digest(), ox1_kernel::fixture_corpus_digest());
    }
}
