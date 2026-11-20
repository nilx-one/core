// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Portable, fail-closed authority gate for the future Telegram text Gateway.
//!
//! The Telegram Bot API identifies a *sender*, not a Bond or device. This
//! module checks facts supplied by a trusted Core host; it NEVER verifies
//! provider evidence, authenticates the host, persists sessions or performs
//! a game write itself. The server must derive the snapshot from authenticated
//! service access, verified provider evidence, AIS and durable Core session
//! state, not from this JSON request.

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
    /// Desired capability. Core still evaluates the authenticated snapshot.
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

/// Trusted host facts, not a wire DTO. Each field MUST be derived by the host
/// after authenticated Telegram transport, AIS and Core lookups. A Telegram
/// user ID, callback payload or public pub_dress cannot fill these fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct GatewayAuthority {
    /// The requesting bot workload is authenticated by its own credential.
    pub service_authenticated: bool,
    /// Telegram transport established genuine provider sender evidence.
    pub provider_verified: bool,
    /// AIS resolved a verified provider link for this exact person.
    pub ais_subject_linked: bool,
    /// The AIS identity matches the owner of this Core product session.
    pub product_subject_matches: bool,
    /// A Core product session exists and has not been revoked.
    pub session_exists: bool,
    /// The session is the active client, with no pending transfer stop.
    pub session_may_act: bool,
    /// A durable atomic update/idempotency reservation was accepted.
    pub update_reserved: bool,
    /// Core independently proved same host and device, not account equality.
    pub same_device_proved: bool,
    /// A fresh, one-use, session/audience-bound continuation was consumed.
    pub continuation_consumed: bool,
}

/// A permission to execute only the requested scope; the host must still
/// enforce resource ownership and the typed Core action's own requirements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GatewayGrant {
    /// Only a minimal status, even if the client is inactive.
    SessionState,
    /// Private read allowed while the owning product session may act.
    ProductRead,
    /// One authorized, idempotency-claimed command.
    ProductWrite,
    /// One authorized Mini App surface transition (no activation transfer).
    OpenMiniApp,
}

/// Evaluate one requested operation against verified host-derived facts.
///
/// This is **not** a credential verifier. The caller must authenticate the bot,
/// validate Telegram evidence and AIS mapping, atomically reserve update IDs
/// for side effects, and verify any device/continuation proof independently.
///
/// # Errors
///
/// A missing fact refuses capability; no fallback to Telegram sender identity.
pub fn authorize(
    request: &GatewayRequest,
    authority: GatewayAuthority,
) -> Result<GatewayGrant, GatewayRefusal> {
    request.validate()?;
    if !authority.service_authenticated {
        return Err(GatewayRefusal::ServiceAuthenticationRequired);
    }
    if !authority.provider_verified {
        return Err(GatewayRefusal::InvalidProviderEvidence);
    }
    if !authority.ais_subject_linked || !authority.product_subject_matches {
        return Err(GatewayRefusal::IdentityNotLinked);
    }
    if !authority.session_exists {
        return Err(GatewayRefusal::ProductSessionRequired);
    }
    match request.operation {
        GatewayOperation::SessionState => Ok(GatewayGrant::SessionState),
        GatewayOperation::ProductRead => {
            if authority.session_may_act {
                Ok(GatewayGrant::ProductRead)
            } else {
                Err(GatewayRefusal::SessionInactive)
            }
        }
        GatewayOperation::ProductWrite => {
            if !authority.session_may_act {
                return Err(GatewayRefusal::SessionInactive);
            }
            if !authority.update_reserved {
                return Err(GatewayRefusal::UpdateNotReserved);
            }
            Ok(GatewayGrant::ProductWrite)
        }
        GatewayOperation::OpenMiniApp => {
            if !authority.session_may_act {
                return Err(GatewayRefusal::SessionInactive);
            }
            if !authority.update_reserved {
                return Err(GatewayRefusal::UpdateNotReserved);
            }
            if !authority.same_device_proved || !authority.continuation_consumed {
                return Err(GatewayRefusal::ContinuityNotProven);
            }
            Ok(GatewayGrant::OpenMiniApp)
        }
    }
}

refusals! {
    /// Stable, typed Gateway refusal. None of these is a successful login.
    GatewayRefusal {
        UnsupportedVersion => ("gateway_unsupported_version", 422),
        InvalidUpdate => ("gateway_invalid_update", 422),
        ServiceAuthenticationRequired => ("service_authentication_required", 401),
        InvalidProviderEvidence => ("invalid_provider_evidence", 401),
        IdentityNotLinked => ("gateway_identity_not_linked", 403),
        ProductSessionRequired => ("product_session_required", 401),
        SessionInactive => ("session_inactive", 403),
        UpdateNotReserved => ("gateway_update_not_reserved", 409),
        ContinuityNotProven => ("gateway_continuity_not_proven", 403),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority() -> GatewayAuthority {
        GatewayAuthority {
            service_authenticated: true,
            provider_verified: true,
            ais_subject_linked: true,
            product_subject_matches: true,
            session_exists: true,
            session_may_act: true,
            update_reserved: true,
            same_device_proved: false,
            continuation_consumed: false,
        }
    }

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
    fn provider_sender_or_anonymous_service_never_confers_bond_permission() {
        for authority in [
            GatewayAuthority {
                service_authenticated: false,
                ..authority()
            },
            GatewayAuthority {
                provider_verified: false,
                ..authority()
            },
            GatewayAuthority {
                ais_subject_linked: false,
                ..authority()
            },
            GatewayAuthority {
                product_subject_matches: false,
                ..authority()
            },
            GatewayAuthority {
                session_exists: false,
                ..authority()
            },
        ] {
            assert!(authorize(&request(GatewayOperation::ProductRead), authority).is_err());
            assert!(authorize(&request(GatewayOperation::ProductWrite), authority).is_err());
        }
        assert_eq!(
            authorize(
                &request(GatewayOperation::ProductRead),
                GatewayAuthority::default()
            ),
            Err(GatewayRefusal::ServiceAuthenticationRequired)
        );
    }

    #[test]
    fn inactive_session_can_only_see_minimal_session_state() {
        let inactive = GatewayAuthority {
            session_may_act: false,
            ..authority()
        };
        assert_eq!(
            authorize(&request(GatewayOperation::SessionState), inactive),
            Ok(GatewayGrant::SessionState)
        );
        for operation in [
            GatewayOperation::ProductRead,
            GatewayOperation::ProductWrite,
            GatewayOperation::OpenMiniApp,
        ] {
            assert_eq!(
                authorize(&request(operation), inactive),
                Err(GatewayRefusal::SessionInactive)
            );
        }
    }

    #[test]
    fn mutations_require_durable_update_reservation() {
        let unreserved = GatewayAuthority {
            update_reserved: false,
            ..authority()
        };
        assert_eq!(
            authorize(&request(GatewayOperation::ProductRead), unreserved),
            Ok(GatewayGrant::ProductRead)
        );
        assert_eq!(
            authorize(&request(GatewayOperation::ProductWrite), unreserved),
            Err(GatewayRefusal::UpdateNotReserved)
        );
        assert_eq!(
            authorize(&request(GatewayOperation::ProductWrite), authority()),
            Ok(GatewayGrant::ProductWrite)
        );
    }

    #[test]
    fn telegram_account_match_is_not_device_continuity() {
        assert_eq!(
            authorize(&request(GatewayOperation::OpenMiniApp), authority()),
            Err(GatewayRefusal::ContinuityNotProven)
        );
        let provider_only = GatewayAuthority {
            same_device_proved: true,
            ..authority()
        };
        assert_eq!(
            authorize(&request(GatewayOperation::OpenMiniApp), provider_only),
            Err(GatewayRefusal::ContinuityNotProven)
        );
        let accepted = GatewayAuthority {
            same_device_proved: true,
            continuation_consumed: true,
            ..authority()
        };
        assert_eq!(
            authorize(&request(GatewayOperation::OpenMiniApp), accepted),
            Ok(GatewayGrant::OpenMiniApp)
        );
    }

    #[test]
    fn refusal_codes_have_stable_statuses() {
        assert_eq!(GatewayRefusal::SessionInactive.code(), "session_inactive");
        assert_eq!(GatewayRefusal::ContinuityNotProven.http_status(), 403);
        assert_eq!(
            GatewayRefusal::from_code("gateway_update_not_reserved"),
            Some(GatewayRefusal::UpdateNotReserved)
        );
    }
}
