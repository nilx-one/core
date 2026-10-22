// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Thin `UniFFI` translation boundary for 0x1 Core.
//!
//! These exports report compatibility metadata and deterministic contract results.
//! They do not create identity, authority, a Bond, a `BondChain`, reciprocity, or
//! any product state.

fn label_wire(result: Result<String, nilxone_contracts::PubDressLabelError>) -> String {
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
    match value.parse::<nilxone_contracts::PubDress>() {
        Ok(_) => "valid".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

/// Returns `valid` when `value` is a station of the business `owner`, or a
/// stable failure code (a `pub_dress` code, or `not_owners_address`).
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn validate_station_pub_dress(owner: String, value: String) -> String {
    let Ok(owner) = owner.parse::<nilxone_contracts::PubDress>() else {
        return "invalid_owner".to_owned();
    };
    match nilxone_contracts::StationPubDress::for_owner(&owner, &value) {
        Ok(_) => "valid".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

/// Forms the station address a business's caption names: `station:<address>`
/// or `error:<station_pub_dress_error_code>`.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn station_pub_dress_from_caption(owner: String, caption: String) -> String {
    let Ok(owner) = owner.parse::<nilxone_contracts::PubDress>() else {
        return "error:invalid_owner".to_owned();
    };
    match nilxone_contracts::StationPubDress::from_caption(&owner, &caption) {
        Ok(station) => format!("station:{}", station.address),
        Err(error) => format!("error:{}", error.code()),
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
        nilxone_contracts::PubDressLabel::stem_from_str(&value)
            .map(|stem| stem.as_str().to_owned()),
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
    let result = nilxone_contracts::PubDressLabel::stem_from_str(&value)
        .and_then(|stem| nilxone_contracts::PubDressLabel::compose(&stem, &suffix))
        .map(|label| label.as_str().to_owned());
    label_wire(result)
}

/// Returns the pinned Unicode version used by the `PubDress` scalar policy.
#[must_use]
#[uniffi::export]
pub fn pub_dress_unicode_version() -> String {
    nilxone_contracts::PUB_DRESS_UNICODE_VERSION.to_owned()
}

/// Returns the exact UTS-46 implementation pin owned by Core.
#[must_use]
#[uniffi::export]
pub fn pub_dress_uts46_implementation() -> String {
    nilxone_contracts::PUB_DRESS_UTS46_IMPLEMENTATION.to_owned()
}

/// Active orb placement wire version.
#[must_use]
#[uniffi::export]
pub fn orb_spill_version() -> u32 {
    nilxone_contracts::ORB_SPILL_VERSION
}

/// Orb spill layout through the same Core wire as Web/Wasm. The optional
/// `via` landmark is navigation only; the find and the orb identities remain
/// unchanged. Since v3, `cells` carries the per-cell spills the identity
/// service answered; since v4 a spill may name a landmark rarity `registry`
/// with `rated_landmarks`, and each rated orb reports its rarity. `now_ms` is canonical decimal milliseconds.
///
/// Returns `{"ok":true,"view":…}` or `{"ok":false,"error":"invalid"}`.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn orb_world(world: String, now_ms: String) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::orb_world_wire(&world, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Returns the normative Core contract version implemented by this build.
#[must_use]
#[uniffi::export]
pub fn contract_version() -> String {
    nilxone_kernel::contract_version().to_owned()
}

/// Returns the version of the canonical synthetic parity corpus.
#[must_use]
#[uniffi::export]
pub fn fixture_corpus_version() -> String {
    nilxone_kernel::fixture_corpus_version().to_owned()
}

/// Returns the validated digest of the canonical synthetic parity corpus.
#[must_use]
#[uniffi::export]
pub fn fixture_corpus_digest() -> String {
    nilxone_kernel::fixture_corpus_digest().to_owned()
}

uniffi::setup_scaffolding!();

/// Applies one observation to local AI Bond needs and spatial state.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn apply_avaia_life(state: String, owner: String, subject: String, command: String) -> String {
    nilxone_contracts::apply_avaia_life(&state, &owner, &subject, &command)
}

/// Reads the same Avaia proximity policy as the Wasm host.
#[must_use]
#[uniffi::export]
pub fn avaia_proximity(distance_m: u32, artifacts: u32, previously_blocked: bool) -> String {
    nilxone_contracts::avaia_proximity_wire(distance_m, artifacts, previously_blocked)
}

/// One transition of the Avaia's drive, the same as the Wasm export: the
/// stored state (or the empty string), one input, the wall clock as a decimal
/// string and the local hour. See `docs/avaia-drive.md`.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn avaia_drive_step(state: String, input: String, now_ms: String, hour: u8) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::avaia_drive_step_wire(&state, &input, now.get(), hour),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// The same `BusinessPoint` transition as the Wasm export: the stored state
/// (or the empty string before `create`), one command and the service clock
/// in seconds as a decimal string. See `docs/business-point.md`.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn apply_business_point_command(state: String, command: String, now_s: String) -> String {
    match now_s.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::apply_business_point_command(&state, &command, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// The same `BusinessPoint` status and visibility read as the Wasm export.
#[must_use]
#[uniffi::export]
#[allow(
    clippy::needless_pass_by_value,
    reason = "UniFFI string inputs are owned"
)]
pub fn business_point_view(state: String, now_s: String, organization_verified: bool) -> String {
    match now_s.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::business_point_view(&state, now.get(), organization_verified),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compose_pub_dress_label, contract_version, derive_pub_dress_label, fixture_corpus_digest,
        fixture_corpus_version, pub_dress_unicode_version, pub_dress_uts46_implementation,
        validate_pub_dress,
    };

    #[test]
    fn uniffi_surface_exposes_the_avaia_proximity_policy() {
        for (distance, artifacts, blocked) in [
            (0, 0, false),
            (15, 3, false),
            (4_499, 5, true),
            (4_500, 0, true),
            (4_500, 0, false),
            (5_000, 0, false),
        ] {
            assert_eq!(
                super::avaia_proximity(distance, artifacts, blocked),
                nilxone_contracts::avaia_proximity_wire(distance, artifacts, blocked)
            );
        }
        assert!(super::avaia_proximity(4_500, 0, true).contains(r#""can_reveal":false"#));
    }

    #[test]
    fn uniffi_surface_matches_the_v3_orb_world_contract() {
        assert_eq!(
            super::orb_spill_version(),
            nilxone_contracts::ORB_SPILL_VERSION
        );
        assert_eq!(super::orb_spill_version(), 4);
        let cells = r#"{"spills":[],"cells":[{"key":"5f1c0a7e9b2d4c6e8a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f","center":{"longitude_e7":"304461000","latitude_e7":"504650000"},"registry":1,"rated_landmarks":[{"id":"n1","at":{"longitude_e7":"304470000","latitude_e7":"504665000"}}],"appeared_at":"1000"}],"picked":[],"bond":null,"avaia":null}"#;
        assert_eq!(
            super::orb_world(cells.to_owned(), "2000".to_owned()),
            nilxone_contracts::orb_world_wire(cells, 2_000)
        );
        assert!(
            super::orb_world(cells.to_owned(), "2000".to_owned()).contains(r#""id":"orb:cell:"#)
        );

        let world = r#"{"spills":[{"artifact_id":"art:seg:312346:298243:e2908:1:0","from":{"longitude_e7":"304461000","latitude_e7":"504650000"},"via":{"longitude_e7":"304470000","latitude_e7":"504665000"},"to":{"longitude_e7":"304469000","latitude_e7":"504655000"},"appeared_at":"1000","expires_at":"1801000","count":20,"taken":[]}],"picked":[],"bond":null,"avaia":null}"#;
        assert_eq!(
            super::orb_world(world.to_owned(), "2000".to_owned()),
            nilxone_contracts::orb_world_wire(world, 2_000)
        );
        assert_eq!(
            super::orb_world(world.to_owned(), "01".to_owned()),
            r#"{"error":"invalid","ok":false}"#
        );
        assert_eq!(
            super::orb_world(
                r#"{"spills":[{"via":{"latitude_e7":1}}]}"#.to_owned(),
                "2000".to_owned()
            ),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    #[test]
    fn uniffi_surface_exposes_the_avaia_drive() {
        let answer = super::avaia_drive_step(
            String::new(),
            r#"{"type":"tap","to":"b"}"#.to_owned(),
            "1000".to_owned(),
            13,
        );
        assert!(answer.contains(r#""purpose":"tap""#), "{answer}");
    }

    #[test]
    fn uniffi_surface_matches_native_handshake() {
        assert_eq!(contract_version(), nilxone_kernel::contract_version());
        assert_eq!(
            fixture_corpus_version(),
            nilxone_kernel::fixture_corpus_version()
        );
        assert_eq!(
            fixture_corpus_digest(),
            nilxone_kernel::fixture_corpus_digest()
        );
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
            derive_pub_dress_label("0x0a🌍".to_owned()),
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

    #[test]
    fn uniffi_surface_exposes_business_points() {
        let command = r#"{"op":"submit","organization_verified":true}"#;
        assert_eq!(
            super::apply_business_point_command(
                String::new(),
                command.to_owned(),
                "1800000000".to_owned()
            ),
            nilxone_contracts::apply_business_point_command("", command, 1_800_000_000)
        );
        assert_eq!(
            super::business_point_view("{}".to_owned(), "01".to_owned(), true),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    #[test]
    fn uniffi_surface_exposes_business_stations() {
        assert_eq!(
            super::station_pub_dress_from_caption(
                "0xda-market".to_owned(),
                "xda-market Золоті Ворота".to_owned()
            ),
            "station:0xda-market-zoloti-vorota"
        );
        assert_eq!(
            super::station_pub_dress_from_caption(
                "0xda-market".to_owned(),
                "xda-market".to_owned()
            ),
            "error:no_place"
        );
        assert_eq!(
            super::validate_station_pub_dress(
                "0xda-market".to_owned(),
                "0xda-market-zoloti-vorota".to_owned()
            ),
            "valid"
        );
        assert_eq!(
            super::validate_station_pub_dress("0xda-market".to_owned(), "0xdrival-shop".to_owned()),
            "not_owners_address"
        );
        assert_eq!(
            super::validate_station_pub_dress("nobody".to_owned(), "0xdrival-shop".to_owned()),
            "invalid_owner"
        );
    }
}
