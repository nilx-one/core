// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Thin WebAssembly translation boundary for 0x1 Core.
//!
//! These exports report compatibility metadata and deterministic contract results.
//! They do not create identity, authority, a Bond, a `BondChain`, reciprocity, or
//! any product state.

use wasm_bindgen::prelude::wasm_bindgen;

fn label_wire(result: Result<String, nilxone_contracts::PubDressLabelError>) -> String {
    match result {
        Ok(label) => format!("label:{label}"),
        Err(error) => format!("error:{}", error.code()),
    }
}

/// Returns `valid` or a stable canonical `pub_dress` failure code.
#[must_use]
#[wasm_bindgen]
pub fn validate_pub_dress(value: &str) -> String {
    match value.parse::<nilxone_contracts::PubDress>() {
        Ok(_) => "valid".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

/// Derives the Core-owned DNS A-label for a raw `pub_dress`.
///
/// The stable wire form is `label:<a-label>` on success or
/// `error:<pub_dress_label_error_code>` on failure.
#[must_use]
#[wasm_bindgen]
pub fn derive_pub_dress_label(value: &str) -> String {
    label_wire(
        nilxone_contracts::PubDressLabel::stem_from_str(value).map(|stem| stem.as_str().to_owned()),
    )
}

/// Composes a collision suffix before the Core-owned UTS-46 encoding.
///
/// The stable wire form matches [`derive_pub_dress_label`].
#[must_use]
#[wasm_bindgen]
pub fn compose_pub_dress_label(value: &str, suffix: &str) -> String {
    let result = nilxone_contracts::PubDressLabel::stem_from_str(value)
        .and_then(|stem| nilxone_contracts::PubDressLabel::compose(&stem, suffix))
        .map(|label| label.as_str().to_owned());
    label_wire(result)
}

/// Returns `valid` when `value` is a station of the business `owner`, or a
/// stable failure code (a `pub_dress` code, or `not_owners_address`).
#[must_use]
#[wasm_bindgen]
pub fn validate_station_pub_dress(owner: &str, value: &str) -> String {
    let Ok(owner) = owner.parse::<nilxone_contracts::PubDress>() else {
        return "invalid_owner".to_owned();
    };
    match nilxone_contracts::StationPubDress::for_owner(&owner, value) {
        Ok(_) => "valid".to_owned(),
        Err(error) => error.code().to_owned(),
    }
}

/// Forms the station address a business's caption names.
///
/// The stable wire form is `station:<address>` on success or
/// `error:<station_pub_dress_error_code>` on failure.
#[must_use]
#[wasm_bindgen]
pub fn station_pub_dress_from_caption(owner: &str, caption: &str) -> String {
    let Ok(owner) = owner.parse::<nilxone_contracts::PubDress>() else {
        return "error:invalid_owner".to_owned();
    };
    match nilxone_contracts::StationPubDress::from_caption(&owner, caption) {
        Ok(station) => format!("station:{}", station.address),
        Err(error) => format!("error:{}", error.code()),
    }
}

/// Applies one command to a stored `BusinessPoint` (the empty string before
/// `create`). `now_s` is the service clock in seconds as a decimal string.
/// Answers `{"ok":true,"state":…,"status":…}` or `{"ok":false,"error":…}`;
/// see `docs/business-point.md`.
#[must_use]
#[wasm_bindgen]
pub fn apply_business_point_command(state: &str, command: &str, now_s: &str) -> String {
    match now_s.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::apply_business_point_command(state, command, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// The effective status and map visibility of a stored `BusinessPoint` at
/// `now_s`, given the host-asserted organization verification.
#[must_use]
#[wasm_bindgen]
pub fn business_point_view(state: &str, now_s: &str, organization_verified: bool) -> String {
    match now_s.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::business_point_view(state, now.get(), organization_verified),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Returns the pinned Unicode version used by the `PubDress` scalar policy.
#[must_use]
#[wasm_bindgen]
pub fn pub_dress_unicode_version() -> String {
    nilxone_contracts::PUB_DRESS_UNICODE_VERSION.to_owned()
}

/// Returns the exact UTS-46 implementation pin owned by Core.
#[must_use]
#[wasm_bindgen]
pub fn pub_dress_uts46_implementation() -> String {
    nilxone_contracts::PUB_DRESS_UTS46_IMPLEMENTATION.to_owned()
}

/// Which item a rolled find is: `item:<id>` or `error:<code>`.
#[must_use]
#[wasm_bindgen]
pub fn find_item(artifact_id: &str, tier: u8) -> String {
    nilxone_contracts::find_item_wire(artifact_id, tier)
}

/// Whether the stored pick-up setting picks up a find of `tier`: `yes`, `no`
/// or `error:<code>`.
#[must_use]
#[wasm_bindgen]
pub fn picks_up(rarities: &str, tier: u8) -> String {
    nilxone_contracts::picks_up_wire(rarities, tier)
}

/// Things, recipes, grids and the currency, as canonical JSON.
#[must_use]
#[wasm_bindgen]
pub fn economy_catalog() -> String {
    nilxone_contracts::economy_catalog_json()
}

/// Whether xSasha's backpack gift is due for a stored inventory at the Bond's
/// level: `yes`, `no` or `error:invalid`.
#[must_use]
#[wasm_bindgen]
pub fn backpack_gift_due(state: &str, bond_level: u32) -> String {
    nilxone_contracts::backpack_gift_due_wire(state, bond_level)
}

/// Applies one inventory command to a stored inventory. `now_ms` is a
/// decimal string; a malformed one answers `{"ok":false,"error":"invalid"}`.
#[must_use]
#[wasm_bindgen]
pub fn apply_inventory_command(state: &str, command: &str, now_ms: &str) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::apply_inventory_command(state, command, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// One transition of the Avaia's drive: the stored state (or the empty
/// string for a fresh one), one input, the wall clock as a decimal string and
/// the local hour. Answers `{"ok":true,"state":…,"commands":[…]}` or
/// `{"ok":false,"error":"invalid"}`; see `docs/avaia-drive.md`.
#[must_use]
#[wasm_bindgen]
pub fn avaia_drive_step(state: &str, input: &str, now_ms: &str, hour: u8) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::avaia_drive_step_wire(state, input, now.get(), hour),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Active orb placement wire version. Hosts must not send a `via` waypoint
/// until this value is at least 2.
#[must_use]
#[wasm_bindgen]
pub fn orb_spill_version() -> u32 {
    nilxone_contracts::ORB_SPILL_VERSION
}

/// The orbs around a Bond: what lies where and when it lands, and what the
/// Bond and the Avaia reach now. `world` is an `OrbWorld` as JSON and
/// `now_ms` a decimal string. Answers `{"ok":true,"view":…}` or
/// `{"ok":false,"error":"invalid"}`; see `docs/orb-spills.md`.
#[must_use]
#[wasm_bindgen]
pub fn orb_world(world: &str, now_ms: &str) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::orb_world_wire(world, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Where a courier such as xPing goes: the nearest of the offered points.
/// `request` is a `CourierDestinationRequest` as JSON. Answers
/// `{"ok":true,"point_id":…}` or `{"ok":false,"error":…}`; see
/// `docs/courier-destination.md`.
#[must_use]
#[wasm_bindgen]
pub fn courier_destination(request: &str) -> String {
    nilxone_contracts::courier_destination_wire(request)
}

/// xPing's engine at `now_ms`: energy, smoke, whether it is broken, when
/// it is full and what a repair costs. `state` is the stored engine (the
/// empty string is a new one), `now_ms` a decimal string. Answers
/// `{"ok":true,"view":…}` or `{"ok":false,"error":"invalid"}`; see
/// `docs/xping-engine.md`.
#[must_use]
#[wasm_bindgen]
pub fn xping_engine_view(state: &str, now_ms: &str) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::xping_engine_view_wire(state, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Applies one engine command (`force` or `repaired`) at `now_ms`.
/// Answers `{"ok":true,"state":…,"view":…}` or `{"ok":false,"error":…}`;
/// see `docs/xping-engine.md`.
#[must_use]
#[wasm_bindgen]
pub fn xping_engine_command(state: &str, command: &str, now_ms: &str) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::xping_engine_command_wire(state, command, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Pays for an xPing engine repair from the inventory at the engine's own
/// price at `now_ms`. Answers `{"ok":true,"inventory":…,"engine":…,
/// "outcome":…}` or `{"ok":false,"error":…}`; see `docs/xping-engine.md`.
#[must_use]
#[wasm_bindgen]
pub fn xping_repair(engine: &str, inventory: &str, now_ms: &str) -> String {
    match now_ms.parse::<nilxone_contracts::DecimalU64>() {
        Ok(now) => nilxone_contracts::xping_repair_wire(engine, inventory, now.get()),
        Err(_) => r#"{"error":"invalid","ok":false}"#.to_owned(),
    }
}

/// Returns the normative Core contract version implemented by this build.
#[must_use]
#[wasm_bindgen]
pub fn contract_version() -> String {
    nilxone_kernel::contract_version().to_owned()
}

/// Returns the version of the canonical synthetic parity corpus.
#[must_use]
#[wasm_bindgen]
pub fn fixture_corpus_version() -> String {
    nilxone_kernel::fixture_corpus_version().to_owned()
}

/// Returns the validated digest of the canonical synthetic parity corpus.
#[must_use]
#[wasm_bindgen]
pub fn fixture_corpus_digest() -> String {
    nilxone_kernel::fixture_corpus_digest().to_owned()
}

/// Applies one observation to local AI Bond needs and spatial state.
#[must_use]
#[wasm_bindgen]
pub fn apply_avaia_life(state: &str, owner: &str, subject: &str, command: &str) -> String {
    nilxone_contracts::apply_avaia_life(state, owner, subject, command)
}

/// Reads Core's distance-bound reveal policy for an Avaia and its Bond. The
/// host passes back whether the previous answer blocked reveals (`true` when
/// it has none), which is what makes the 90% restore a real transition.
#[must_use]
#[wasm_bindgen]
pub fn avaia_proximity(distance_m: u32, artifacts: u32, previously_blocked: bool) -> String {
    nilxone_contracts::avaia_proximity_wire(distance_m, artifacts, previously_blocked)
}

/// Returns the reporter input for a Core failure, named by its `errors.v1`
/// surface and `snake_case` code. The result is `{"ok":true,"report":{...}}`, where
/// `report` carries `errorId`, `severity`, `message`, `context` and `tags` for a
/// browser reporter, or `{"ok":false,"error":"unknown_failure"}` outside the
/// closed taxonomy.
#[must_use]
#[wasm_bindgen]
pub fn core_failure_report(surface: &str, code: &str) -> String {
    match nilxone_contracts::failure_report(surface, code) {
        Ok(report) => format!(r#"{{"ok":true,"report":{report}}}"#),
        Err(_) => r#"{"error":"unknown_failure","ok":false}"#.to_owned(),
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
    fn wasm_surface_exposes_the_economy() {
        assert_eq!(
            super::find_item("art:seg:312346:298243:e2908:1:0", 1),
            "item:bottle_cap"
        );
        assert_eq!(super::picks_up("common", 2), "yes");
        assert_eq!(super::backpack_gift_due("", 3), "yes");
        assert!(super::economy_catalog().contains("\"emblem\":\"₴€£\""));
        assert!(
            super::apply_inventory_command("", r#"{"op":"finish_craft"}"#, "0").contains("no_job")
        );
        assert_eq!(
            super::apply_inventory_command("", r#"{"op":"finish_craft"}"#, "01"),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    #[test]
    fn wasm_surface_exposes_the_avaia_proximity_policy() {
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
    fn wasm_surface_exposes_the_orb_world() {
        assert_eq!(
            super::orb_spill_version(),
            nilxone_contracts::ORB_SPILL_VERSION
        );
        assert_eq!(super::orb_spill_version(), 2);
        let guided = r#"{"spills":[{"artifact_id":"art:seg:312346:298243:e2908:1:0","from":{"longitude_e7":"304461000","latitude_e7":"504650000"},"via":{"longitude_e7":"304470000","latitude_e7":"504665000"},"to":{"longitude_e7":"304469000","latitude_e7":"504655000"},"appeared_at":"1000","expires_at":"1801000","count":20,"taken":[]}],"picked":[],"bond":null,"avaia":null}"#;
        assert_eq!(
            super::orb_world(guided, "2000"),
            nilxone_contracts::orb_world_wire(guided, 2_000)
        );
        assert_eq!(
            super::orb_world(r#"{"spills":[]}"#, "1000"),
            nilxone_contracts::orb_world_wire(r#"{"spills":[]}"#, 1_000)
        );
        assert!(super::orb_world(r#"{"spills":[]}"#, "1000").starts_with(r#"{"ok":true"#));
        assert_eq!(
            super::orb_world(r#"{"spills":[]}"#, "01"),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    #[test]
    fn wasm_surface_exposes_the_avaia_drive() {
        let answer = super::avaia_drive_step("", r#"{"type":"tap","to":"b"}"#, "1000", 13);
        assert!(
            answer.starts_with(r#"{"commands":[{"do":"walk""#),
            "{answer}"
        );
        assert_eq!(
            super::avaia_drive_step("", r#"{"type":"tick"}"#, "01", 13),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    #[test]
    fn wasm_surface_matches_native_handshake() {
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
    fn wasm_surface_exposes_canonical_pub_dress_validation() {
        assert_eq!(validate_pub_dress("0x0sky"), "valid");
        assert_eq!(validate_pub_dress("0x0Sky"), "valid");
        assert_eq!(validate_pub_dress("0x0небо"), "valid");
        assert_eq!(validate_pub_dress("0xgsky"), "invalid_discriminator");
    }

    #[test]
    fn wasm_surface_exposes_core_failure_reports() {
        let report = super::core_failure_report("avaia.drive", "invalid");
        assert!(
            report.starts_with(r#"{"ok":true,"report":{"context""#),
            "{report}"
        );
        assert!(report.contains(r#""errorId":"core.avaia.drive.invalid""#));
        assert_eq!(
            super::core_failure_report("avaia.drive", "unknown"),
            r#"{"error":"unknown_failure","ok":false}"#
        );
    }

    #[test]
    fn wasm_surface_exposes_core_owned_label_derivation() {
        assert_eq!(derive_pub_dress_label("0x0небо"), "label:xn--0x0-dddt1cj");
        assert_eq!(derive_pub_dress_label("0x0Небо"), "label:xn--0x0-dddt1cj");
        assert_eq!(derive_pub_dress_label("0x0a🌍"), "error:disallowed_scalar");
        assert!(compose_pub_dress_label("0x0небо", "42").starts_with("label:xn--"));
        assert_eq!(pub_dress_unicode_version(), "16.0.0");
        assert_eq!(
            pub_dress_uts46_implementation(),
            "idna=1.1.0;idna_adapter=1.1.0;idna_mapping=1.1.0"
        );
    }

    #[test]
    fn wasm_surface_exposes_business_points() {
        let command = r#"{"op":"submit","organization_verified":true}"#;
        assert_eq!(
            super::apply_business_point_command("", command, "1800000000"),
            nilxone_contracts::apply_business_point_command("", command, 1_800_000_000)
        );
        assert_eq!(
            super::apply_business_point_command("", command, "01"),
            r#"{"error":"invalid","ok":false}"#
        );
        assert_eq!(
            super::business_point_view("{}", "1", true),
            r#"{"error":"invalid","ok":false}"#
        );
        assert_eq!(
            super::business_point_view("", "-1", true),
            r#"{"error":"invalid","ok":false}"#
        );
    }

    #[test]
    fn wasm_surface_exposes_business_stations() {
        assert_eq!(
            super::station_pub_dress_from_caption("0xda-market", "xda-market Золоті Ворота"),
            "station:0xda-market-zoloti-vorota"
        );
        assert_eq!(
            super::station_pub_dress_from_caption("0xda-market", "xda-market"),
            "error:no_place"
        );
        assert_eq!(
            super::validate_station_pub_dress("0xda-market", "0xda-market-zoloti-vorota"),
            "valid"
        );
        assert_eq!(
            super::validate_station_pub_dress("0xda-market", "0xdrival-shop"),
            "not_owners_address"
        );
        assert_eq!(
            super::validate_station_pub_dress("nobody", "0xdrival-shop"),
            "invalid_owner"
        );
    }
}
