// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Service-authenticated Telegram login initiation, not an identity provider.
//! The transport delivers verified Telegram update metadata. The product host
//! authenticates the bot service and creates durable, single-use OIDC state.
//! This portable wire contract never gives Telegram authority over a Bond.

use serde::{Deserialize, Serialize};

/// POST, with authenticated bot-service credentials; not a public API.
pub const START_PATH: &str = "/api/v1/telegram/login/start";
/// A started request may remain pending for at most fifteen minutes.
pub const MAX_TTL_SECONDS: u64 = 900;
/// Current wire version.
pub const WIRE_VERSION: u8 = 1;

/// Metadata from trusted Telegram transport. Not a proof of Bond authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginStartRequest {
    /// The expected wire contract.
    pub version: u8,
    /// Only Telegram is supported in this stage.
    pub provider: String,
    /// Verified Telegram sender id, not a user-supplied Bond.
    pub provider_actor_id: u64,
    /// The private chat that received the command.
    pub chat_id: i64,
    /// Original Telegram command message, for scoped idempotency.
    pub message_id: i32,
}

impl LoginStartRequest {
    /// Verifies structural invariants. Service authentication, provider update
    /// verification, TTL, replay and binding are mandatory host responsibilities.
    ///
    /// # Errors
    ///
    /// Invalid or unsupported request fields fail closed.
    pub fn validate(&self) -> Result<(), LoginStartRefusal> {
        if self.version != WIRE_VERSION || self.provider != "telegram" {
            return Err(LoginStartRefusal::UnsupportedProvider);
        }
        if self.provider_actor_id == 0 || self.chat_id <= 0 || self.message_id <= 0 {
            return Err(LoginStartRefusal::InvalidProviderEvidence);
        }
        Ok(())
    }
}

/// Issued by the authenticated Core host, never constructed by a messenger.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoginStartResponse {
    /// AIS-scoped URL, with host-created state, nonce and PKCE challenge.
    /// This URL is not a login success.
    pub authorization_url: String,
    /// Remaining time before one-use host transaction expires.
    pub expires_in_seconds: u64,
}

impl LoginStartResponse {
    /// Checks portable lifetime bounds. The host must still validate its
    /// AIS client, callback, PKCE, transaction and origin.
    #[must_use]
    pub fn has_valid_ttl(&self) -> bool {
        (1..=MAX_TTL_SECONDS).contains(&self.expires_in_seconds)
    }
}

refusals! {
    /// Why a login transaction could not be initiated.
    LoginStartRefusal {
        InvalidProviderEvidence => ("invalid_provider_evidence", 422),
        UnsupportedProvider => ("unsupported_provider", 422),
        ServiceAuthenticationRequired => ("service_authentication_required", 401),
        ProviderLinkConflict => ("provider_link_conflict", 409),
        LoginRateLimited => ("login_rate_limited", 429),
        LoginServiceUnavailable => ("login_service_unavailable", 503),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_is_closed_and_versioned() {
        let req = LoginStartRequest {
            version: WIRE_VERSION,
            provider: "telegram".to_owned(),
            provider_actor_id: 123,
            chat_id: 123,
            message_id: 42,
        };
        assert!(req.validate().is_ok());
        assert_eq!(
            serde_json::to_string(&req).unwrap(),
            r#"{"version":1,"provider":"telegram","provider_actor_id":123,"chat_id":123,"message_id":42}"#
        );
        let mut forged = serde_json::to_value(req).unwrap();
        forged["bond_pub_dress"] = serde_json::json!("0x0sky");
        assert!(serde_json::from_value::<LoginStartRequest>(forged).is_err());
    }

    #[test]
    fn invalid_evidence_and_different_providers_fail_closed() {
        let request = LoginStartRequest {
            version: 1,
            provider: "telegram".into(),
            provider_actor_id: 0,
            chat_id: 123,
            message_id: 1,
        };
        assert_eq!(
            request.validate(),
            Err(LoginStartRefusal::InvalidProviderEvidence)
        );
        let wrong = LoginStartRequest {
            provider_actor_id: 123,
            provider: "discord".into(),
            ..request
        };
        assert_eq!(wrong.validate(), Err(LoginStartRefusal::UnsupportedProvider));
    }

    #[test]
    fn response_is_a_pending_link_not_a_session() {
        let response = LoginStartResponse {
            authorization_url: "https://auth.aiaiaiai.org/0x1?state=example".into(),
            expires_in_seconds: 300,
        };
        assert!(response.has_valid_ttl());
        assert_eq!(
            serde_json::from_str::<LoginStartResponse>(
                &serde_json::to_string(&response).unwrap()
            )
            .unwrap(),
            response
        );
        let expired = LoginStartResponse {
            expires_in_seconds: MAX_TTL_SECONDS + 1,
            ..response
        };
        assert!(!expired.has_valid_ttl());
    }

    #[test]
    fn login_refusal_codes_are_stable() {
        assert_eq!(
            LoginStartRefusal::ServiceAuthenticationRequired.http_status(),
            401
        );
        assert_eq!(
            LoginStartRefusal::from_code("login_service_unavailable"),
            Some(LoginStartRefusal::LoginServiceUnavailable)
        );
    }
}
