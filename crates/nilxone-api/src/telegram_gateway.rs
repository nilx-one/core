// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Wire contract for the Telegram text Gateway.
//!
//! The request carries transport metadata only. The Core service owns the
//! authorization decision and must derive it from AIS evidence and the
//! matching live product session.

use serde::{Deserialize, Serialize};

/// Proposed product Gateway path. No HTTP handler is enabled by this module.
pub const GATEWAY_PATH: &str = "/api/v1/telegram/gateway";
/// First portable Telegram Gateway wire version.
pub const WIRE_VERSION: u8 = 1;

/// A single requested Core capability, never a client-granted permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayOperation {
    /// Limited session-status projection only; not private gameplay data.
    SessionState,
    /// Read private Bond/world projections from an active product session.
    ProductRead,
    /// Perform a typed Core action on the active client.
    ProductWrite,
    /// Request a same-device transition into the embedded Telegram Mini App.
    OpenMiniApp,
}

/// Client-provided transport metadata; it carries NO identity or product
/// authorization and cannot select a Bond, device, active session or role.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayRequest {
    /// Required version of the wire representation.
    pub version: u8,
    /// Telegram update ID, for stable idempotent processing by the host.
    pub update_id: u64,
    /// Desired capability. Core verifies the caller and the live session.
    pub operation: GatewayOperation,
}

impl GatewayRequest {
    /// Reject malformed or unsupported versions without fetching product state.
    ///
    /// # Errors
    ///
    /// Unsupported versions and zero update identifiers fail closed.
    pub fn validate(&self) -> Result<(), GatewayRefusal> {
        if self.version != WIRE_VERSION {
            return Err(GatewayRefusal::UnsupportedVersion);
        }
        if self.update_id == 0 {
            return Err(GatewayRefusal::InvalidUpdate);
        }
        Ok(())
    }
}

/// Successful disposition of a mutating Gateway request.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayMutationOutcome {
    /// The authorized mutation and its result were committed atomically.
    Applied,
    /// Return the stored result without running the mutation again.
    ///
    /// Core uses this only for the same authenticated caller, update ID
    /// and operation. Reusing the ID with a different operation is a conflict.
    Replayed,
}

refusals! {
    /// Stable, typed Gateway refusal. None of these is a successful login.
    GatewayRefusal {
        UnsupportedVersion => ("gateway_unsupported_version", 422),
        InvalidUpdate => ("gateway_invalid_update", 422),
        ServiceAuthenticationRequired => ("service_authentication_required", 401),
        InvalidProviderEvidence => ("gateway_invalid_provider_evidence", 401),
        IdentityNotLinked => ("gateway_identity_not_linked", 403),
        ProductSessionRequired => ("product_session_required", 401),
        SessionInactive => ("session_inactive", 403),
        UpdateNotReserved => ("gateway_update_not_reserved", 409),
        ConflictingReplay => ("gateway_conflicting_replay", 409),
        ContinuityNotProven => ("gateway_continuity_not_proven", 403),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(operation: GatewayOperation) -> GatewayRequest {
        GatewayRequest {
            version: WIRE_VERSION,
            update_id: 42,
            operation,
        }
    }

    #[test]
    fn strict_versioned_request_has_no_bond_or_authority_claims() {
        let message = serde_json::to_string(&request(GatewayOperation::ProductWrite)).unwrap();
        assert_eq!(
            message,
            r#"{"version":1,"update_id":42,"operation":"product_write"}"#
        );
        let forged = r#"{"version":1,"update_id":42,"operation":"product_write","bond":"0x0sky"}"#;
        assert!(serde_json::from_str::<GatewayRequest>(forged).is_err());
        assert_eq!(request(GatewayOperation::SessionState).validate(), Ok(()));
        assert_eq!(
            GatewayRequest {
                version: 2,
                ..request(GatewayOperation::SessionState)
            }
            .validate(),
            Err(GatewayRefusal::UnsupportedVersion)
        );
        assert_eq!(
            GatewayRequest {
                update_id: 0,
                ..request(GatewayOperation::SessionState)
            }
            .validate(),
            Err(GatewayRefusal::InvalidUpdate)
        );
    }

    #[test]
    fn mutations_distinguish_new_results_from_replays() {
        assert_eq!(
            serde_json::to_string(&GatewayMutationOutcome::Applied).unwrap(),
            r#""applied""#
        );
        assert_eq!(
            serde_json::to_string(&GatewayMutationOutcome::Replayed).unwrap(),
            r#""replayed""#
        );
    }

    #[test]
    fn refusal_codes_have_stable_statuses() {
        for (refusal, code, status) in [
            (
                GatewayRefusal::UnsupportedVersion,
                "gateway_unsupported_version",
                422,
            ),
            (GatewayRefusal::InvalidUpdate, "gateway_invalid_update", 422),
            (
                GatewayRefusal::ServiceAuthenticationRequired,
                "service_authentication_required",
                401,
            ),
            (
                GatewayRefusal::InvalidProviderEvidence,
                "gateway_invalid_provider_evidence",
                401,
            ),
            (
                GatewayRefusal::IdentityNotLinked,
                "gateway_identity_not_linked",
                403,
            ),
            (
                GatewayRefusal::ProductSessionRequired,
                "product_session_required",
                401,
            ),
            (GatewayRefusal::SessionInactive, "session_inactive", 403),
            (
                GatewayRefusal::UpdateNotReserved,
                "gateway_update_not_reserved",
                409,
            ),
            (
                GatewayRefusal::ConflictingReplay,
                "gateway_conflicting_replay",
                409,
            ),
            (
                GatewayRefusal::ContinuityNotProven,
                "gateway_continuity_not_proven",
                403,
            ),
        ] {
            assert_eq!(refusal.code(), code);
            assert_eq!(refusal.http_status(), status);
            assert_eq!(GatewayRefusal::from_code(code), Some(refusal));
        }
    }
}
