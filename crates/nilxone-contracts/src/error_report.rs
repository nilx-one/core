// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! `errors.v1` projection for Core-owned failures.
//!
//! Core owns the failure taxonomy and the canonical bytes. The host owns the clock,
//! the event identifier and the transport to a 4x-errors reporter: it passes
//! `event_id` and `observed_at` in, so this module performs no I/O and reads no
//! ambient state.
//!
//! Two forms are offered. [`errors_v1_event`] yields a complete `errors.v1` event
//! for a producer that supplies its own identity and time. [`failure_report`] yields
//! only the reporter input (`errorId`, `severity`, `message`, `context`, `tags`) for a
//! browser host whose SDK generates `event_id` and `observed_at` itself.

use crate::{
    AvaiaPubDressError, CanonicalJsonError, ContractVersion, CoreError, DriveError, PubDressError,
    PubDressLabelError, canonical_json,
};
use core::fmt;
use serde_json::{Value, json};

/// Protocol version emitted by this projection.
pub const ERRORS_PROTOCOL_VERSION: &str = "errors.v1";

/// Project identity under which Core reports its failures.
pub const ERRORS_PROJECT: &str = "nilx-one/core";

/// A Core-owned failure with a stable `errors.v1` identity.
pub trait Reportable {
    /// Stable surface segment of the `error_id`, lowercase and dot-separated.
    const SURFACE: &'static str;

    /// Every failure code the surface can produce, as stable `snake_case` strings.
    const CODES: &'static [&'static str];

    /// Stable `snake_case` failure code owned by the surface.
    fn failure_code(&self) -> &'static str;
}

impl Reportable for PubDressError {
    const SURFACE: &'static str = "registration.pubdress";
    const CODES: &'static [&'static str] = &[
        "invalid_prefix",
        "invalid_discriminator",
        "invalid_length",
        "invalid_character",
    ];

    fn failure_code(&self) -> &'static str {
        self.code()
    }
}

impl Reportable for PubDressLabelError {
    const SURFACE: &'static str = "registration.label";
    const CODES: &'static [&'static str] = &[
        "not_a_pub_dress",
        "invalid_character",
        "disallowed_scalar",
        "bidi_rule",
        "not_encodable",
        "boundary_hyphen",
        "too_long",
        "suffix_too_long",
    ];

    fn failure_code(&self) -> &'static str {
        self.code()
    }
}

impl Reportable for AvaiaPubDressError {
    const SURFACE: &'static str = "avaia.pubdress";
    const CODES: &'static [&'static str] = &[
        "invalid_discriminator",
        "invalid_length",
        "invalid_character",
        "missing_ai_suffix",
    ];

    fn failure_code(&self) -> &'static str {
        self.code()
    }
}

impl Reportable for DriveError {
    const SURFACE: &'static str = "avaia.drive";
    const CODES: &'static [&'static str] = &["invalid"];

    fn failure_code(&self) -> &'static str {
        "invalid"
    }
}

impl Reportable for CoreError {
    const SURFACE: &'static str = "contract";
    const CODES: &'static [&'static str] = &[
        "malformed_envelope",
        "unsupported_contract_version",
        "unknown_variant",
        "invalid_identifier",
        "invalid_participants",
        "state_revision_mismatch",
        "invalid_transition",
        "terminal_bond_chain",
        "history_rollback",
        "history_divergence",
        "invalid_history",
        "unknown_authority",
        "missing_context",
    ];

    fn failure_code(&self) -> &'static str {
        self.code().as_str()
    }
}

/// Every surface core reports, with the codes it owns. Drives [`failure_report`].
const SURFACES: &[(&str, &[&str])] = &[
    (PubDressError::SURFACE, PubDressError::CODES),
    (PubDressLabelError::SURFACE, PubDressLabelError::CODES),
    (AvaiaPubDressError::SURFACE, AvaiaPubDressError::CODES),
    (DriveError::SURFACE, DriveError::CODES),
    (CoreError::SURFACE, CoreError::CODES),
];

/// Returns the stable `errors.v1` `error_id` for a Core-owned failure.
///
/// Underscores in the failure code become dots so the identifier satisfies the
/// protocol pattern, for example `core.registration.pubdress.invalid.prefix`.
#[must_use]
pub fn error_id<T: Reportable>(failure: &T) -> String {
    error_id_for(T::SURFACE, failure.failure_code())
}

fn error_id_for(surface: &str, code: &str) -> String {
    format!("core.{surface}.{}", code.replace('_', "."))
}

fn message_for(surface: &str, code: &str) -> String {
    format!("Core rejected {surface} input: {code}.")
}

fn context_for(surface: &str, code: &str) -> Value {
    json!({
        "contract_version": ContractVersion::CURRENT.to_string(),
        "surface": surface,
        "code": code,
    })
}

fn tags() -> Value {
    json!(["core", format!("contract.{}", ContractVersion::CURRENT)])
}

/// Failure to project a Core failure into `errors.v1` or reporter input.
#[derive(Debug)]
pub enum ReportError {
    /// `event_id` is not a UUID in canonical 8-4-4-4-12 hexadecimal form.
    InvalidEventId,
    /// `observed_at` is not an RFC 3339 date-time.
    InvalidObservedAt,
    /// The surface or code is not part of Core's closed failure taxonomy.
    UnknownFailure,
    /// Canonical JSON production failed.
    Canonical(CanonicalJsonError),
    /// The canonical bytes were not valid UTF-8.
    Encoding,
}

impl fmt::Display for ReportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEventId => formatter.write_str("errors.v1 event_id must be a UUID"),
            Self::InvalidObservedAt => {
                formatter.write_str("errors.v1 observed_at must be an RFC 3339 date-time")
            }
            Self::UnknownFailure => formatter.write_str("unknown Core failure surface or code"),
            Self::Canonical(error) => write!(formatter, "{error}"),
            Self::Encoding => formatter.write_str("canonical errors.v1 bytes are not UTF-8"),
        }
    }
}

impl std::error::Error for ReportError {}

fn canonical_string(value: &Value) -> Result<String, ReportError> {
    let bytes = canonical_json(value).map_err(ReportError::Canonical)?;
    String::from_utf8(bytes).map_err(|_| ReportError::Encoding)
}

/// Projects a Core-owned failure into canonical `errors.v1` event JSON.
///
/// The host supplies `event_id` (a fresh UUID per occurrence) and `observed_at`
/// (RFC 3339, in the host's clock). Both are validated, never generated here.
///
/// # Errors
///
/// Returns [`ReportError`] when a supplied identifier or timestamp is malformed.
pub fn errors_v1_event<T: Reportable>(
    failure: &T,
    event_id: &str,
    observed_at: &str,
) -> Result<String, ReportError> {
    if !is_uuid(event_id) {
        return Err(ReportError::InvalidEventId);
    }
    if !is_rfc3339(observed_at) {
        return Err(ReportError::InvalidObservedAt);
    }

    let code = failure.failure_code();
    let surface = T::SURFACE;
    let contract = ContractVersion::CURRENT.to_string();
    canonical_string(&json!({
        "protocol_version": ERRORS_PROTOCOL_VERSION,
        "event_id": event_id,
        "error_id": error_id_for(surface, code),
        "project": ERRORS_PROJECT,
        "source": "nilxone-contracts",
        "severity": "error",
        "message": message_for(surface, code),
        "full_text": format!("{surface} failed with code {code} under Core contract {contract}."),
        "observed_at": observed_at,
        "context": context_for(surface, code),
        "tags": tags(),
        "family_id": null,
        "caused_by_event_id": null,
        "correlation_id": null,
    }))
}

/// Returns the reporter input for a Core failure named by its surface and code.
///
/// The JSON object carries `errorId`, `severity`, `message`, `context` and `tags`,
/// the fields a browser reporter's `report()` accepts. The reporter generates
/// `event_id` and `observed_at`, so none is produced here.
///
/// # Errors
///
/// Returns [`ReportError::UnknownFailure`] unless the surface and code are both in
/// Core's closed taxonomy.
pub fn failure_report(surface: &str, code: &str) -> Result<String, ReportError> {
    let known = SURFACES
        .iter()
        .any(|(name, codes)| *name == surface && codes.contains(&code));
    if !known {
        return Err(ReportError::UnknownFailure);
    }

    canonical_string(&json!({
        "errorId": error_id_for(surface, code),
        "severity": "error",
        "message": message_for(surface, code),
        "context": context_for(surface, code),
        "tags": tags(),
    }))
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        })
}

fn is_rfc3339(value: &str) -> bool {
    if !value.is_ascii() {
        return false;
    }
    let Some((date, time)) = value.split_once('T') else {
        return false;
    };
    let date_ok = date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| match index {
            4 | 7 => byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    let Some(zone_start) = time.find(['Z', 'z', '+', '-']) else {
        return false;
    };
    let (clock, zone) = time.split_at(zone_start);
    let clock_bytes = clock.as_bytes();
    let clock_ok = clock_bytes.len() >= 8
        && clock_bytes[..8]
            .iter()
            .enumerate()
            .all(|(index, byte)| match index {
                2 | 5 => *byte == b':',
                _ => byte.is_ascii_digit(),
            });
    let fraction_ok = match clock.get(8..) {
        Some("") => true,
        Some(fraction) => {
            fraction.len() > 1
                && fraction.starts_with('.')
                && fraction[1..].bytes().all(|byte| byte.is_ascii_digit())
        }
        None => false,
    };
    let zone_ok = matches!(zone, "Z" | "z")
        || (zone.len() == 6
            && matches!(zone.as_bytes()[0], b'+' | b'-')
            && zone.as_bytes()[3] == b':'
            && zone[1..]
                .bytes()
                .enumerate()
                .all(|(index, byte)| match index {
                    2 => byte == b':',
                    _ => byte.is_ascii_digit(),
                }));
    date_ok && clock_ok && fraction_ok && zone_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;
    use serde_json::Value;

    const EVENT_ID: &str = "0b6f0c52-3f3e-4a8e-9d4f-6a1c2d3e4f50";
    const OBSERVED_AT: &str = "2026-10-14T16:34:00.123+02:00";

    fn is_error_id(value: &str) -> bool {
        value.split('.').count() >= 2
            && value.split('.').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            })
    }

    /// Every variant of each failure enum, matched exhaustively so that adding a
    /// variant without updating `CODES` fails to compile or fails the test below.
    fn every_failure_code() -> Vec<(&'static str, &'static str)> {
        let mut codes = Vec::new();
        for error in [
            PubDressError::InvalidPrefix,
            PubDressError::InvalidDiscriminator,
            PubDressError::InvalidLength,
            PubDressError::InvalidCharacter,
        ] {
            match error {
                PubDressError::InvalidPrefix
                | PubDressError::InvalidDiscriminator
                | PubDressError::InvalidLength
                | PubDressError::InvalidCharacter => {}
            }
            codes.push((PubDressError::SURFACE, error.code()));
        }
        for error in [
            PubDressLabelError::NotAPubDress,
            PubDressLabelError::InvalidCharacter,
            PubDressLabelError::DisallowedScalar,
            PubDressLabelError::BidiRule,
            PubDressLabelError::NotEncodable,
            PubDressLabelError::BoundaryHyphen,
            PubDressLabelError::TooLong,
            PubDressLabelError::SuffixTooLong,
        ] {
            match error {
                PubDressLabelError::NotAPubDress
                | PubDressLabelError::InvalidCharacter
                | PubDressLabelError::DisallowedScalar
                | PubDressLabelError::BidiRule
                | PubDressLabelError::NotEncodable
                | PubDressLabelError::BoundaryHyphen
                | PubDressLabelError::TooLong
                | PubDressLabelError::SuffixTooLong => {}
            }
            codes.push((PubDressLabelError::SURFACE, error.code()));
        }
        for error in [
            AvaiaPubDressError::InvalidDiscriminator,
            AvaiaPubDressError::InvalidLength,
            AvaiaPubDressError::InvalidCharacter,
            AvaiaPubDressError::MissingAiSuffix,
        ] {
            match error {
                AvaiaPubDressError::InvalidDiscriminator
                | AvaiaPubDressError::InvalidLength
                | AvaiaPubDressError::InvalidCharacter
                | AvaiaPubDressError::MissingAiSuffix => {}
            }
            codes.push((AvaiaPubDressError::SURFACE, error.code()));
        }
        codes.push((DriveError::SURFACE, DriveError.failure_code()));
        for code in [
            ErrorCode::MalformedEnvelope,
            ErrorCode::UnsupportedContractVersion,
            ErrorCode::UnknownVariant,
            ErrorCode::InvalidIdentifier,
            ErrorCode::InvalidParticipants,
            ErrorCode::StateRevisionMismatch,
            ErrorCode::InvalidTransition,
            ErrorCode::TerminalBondChain,
            ErrorCode::HistoryRollback,
            ErrorCode::HistoryDivergence,
            ErrorCode::InvalidHistory,
            ErrorCode::UnknownAuthority,
            ErrorCode::MissingContext,
        ] {
            match code {
                ErrorCode::MalformedEnvelope
                | ErrorCode::UnsupportedContractVersion
                | ErrorCode::UnknownVariant
                | ErrorCode::InvalidIdentifier
                | ErrorCode::InvalidParticipants
                | ErrorCode::StateRevisionMismatch
                | ErrorCode::InvalidTransition
                | ErrorCode::TerminalBondChain
                | ErrorCode::HistoryRollback
                | ErrorCode::HistoryDivergence
                | ErrorCode::InvalidHistory
                | ErrorCode::UnknownAuthority
                | ErrorCode::MissingContext => {}
            }
            let serde_code = serde_json::to_value(code).expect("serializes");
            assert_eq!(serde_code, Value::String(code.as_str().to_owned()));
            codes.push((CoreError::SURFACE, code.as_str()));
        }
        codes
    }

    #[test]
    fn error_ids_follow_the_errors_v1_pattern() {
        assert_eq!(
            error_id(&PubDressError::InvalidPrefix),
            "core.registration.pubdress.invalid.prefix"
        );
        assert_eq!(
            error_id(&PubDressLabelError::NotAPubDress),
            "core.registration.label.not.a.pub.dress"
        );
        assert_eq!(
            error_id(&AvaiaPubDressError::MissingAiSuffix),
            "core.avaia.pubdress.missing.ai.suffix"
        );
        assert_eq!(error_id(&DriveError), "core.avaia.drive.invalid");

        for (surface, code) in every_failure_code() {
            assert!(
                is_error_id(&error_id_for(surface, code)),
                "{surface} {code}"
            );
        }
    }

    #[test]
    fn declared_codes_cover_every_variant() {
        for (surface, code) in every_failure_code() {
            let declared = SURFACES
                .iter()
                .find(|(name, _)| *name == surface)
                .expect("surface is declared")
                .1;
            assert!(declared.contains(&code), "{surface} must declare {code}");
        }
    }

    #[test]
    fn projects_a_failure_into_a_complete_errors_v1_event() {
        let event = errors_v1_event(&DriveError, EVENT_ID, OBSERVED_AT).expect("valid input");
        let value: Value = serde_json::from_str(&event).expect("json");

        assert_eq!(
            value,
            json!({
                "protocol_version": "errors.v1",
                "event_id": EVENT_ID,
                "error_id": "core.avaia.drive.invalid",
                "project": "nilx-one/core",
                "source": "nilxone-contracts",
                "severity": "error",
                "message": "Core rejected avaia.drive input: invalid.",
                "full_text": "avaia.drive failed with code invalid under Core contract 0.1.0.",
                "observed_at": OBSERVED_AT,
                "context": {
                    "contract_version": "0.1.0",
                    "surface": "avaia.drive",
                    "code": "invalid"
                },
                "tags": ["core", "contract.0.1.0"],
                "family_id": null,
                "caused_by_event_id": null,
                "correlation_id": null
            })
        );
    }

    #[test]
    fn rejects_malformed_host_supplied_identity_and_time() {
        assert!(matches!(
            errors_v1_event(&DriveError, "not-a-uuid", OBSERVED_AT),
            Err(ReportError::InvalidEventId)
        ));
        for observed_at in [
            "2026-10-14 16:34:00Z",
            "2026-10-14T16:34Z",
            "2026-10-14T16:34:00",
            "2026-10-14T16:34:00.Z",
            "2026-10-14T16:34:00+0200",
        ] {
            assert!(
                matches!(
                    errors_v1_event(&DriveError, EVENT_ID, observed_at),
                    Err(ReportError::InvalidObservedAt)
                ),
                "{observed_at} must be rejected"
            );
        }
        assert!(errors_v1_event(&DriveError, EVENT_ID, "2026-10-14T16:34:00Z").is_ok());
    }

    #[test]
    fn failure_report_is_the_reporter_input_for_a_known_failure() {
        let report = failure_report("registration.pubdress", "invalid_prefix").expect("known");
        let value: Value = serde_json::from_str(&report).expect("json");

        assert_eq!(
            value,
            json!({
                "errorId": "core.registration.pubdress.invalid.prefix",
                "severity": "error",
                "message": "Core rejected registration.pubdress input: invalid_prefix.",
                "context": {
                    "contract_version": "0.1.0",
                    "surface": "registration.pubdress",
                    "code": "invalid_prefix"
                },
                "tags": ["core", "contract.0.1.0"]
            })
        );
        assert!(!report.contains("event_id") && !report.contains("observed_at"));
    }

    #[test]
    fn failure_report_rejects_anything_outside_the_taxonomy() {
        assert!(matches!(
            failure_report("registration.pubdress", "no_such_code"),
            Err(ReportError::UnknownFailure)
        ));
        assert!(matches!(
            failure_report("avaia.drive", "invalid_prefix"),
            Err(ReportError::UnknownFailure)
        ));
        assert!(matches!(
            failure_report("cutscene.render", "invalid"),
            Err(ReportError::UnknownFailure)
        ));
    }
}
