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

/// POST, with the same authenticated bot-service credentials as login start.
/// The bot claims the verified callback of its /login message. It asks right
/// after /login and repeats while the answer is `ConfirmationNotFound`: the
/// callback itself proves the identity belongs to this Telegram sender, so
/// the person presses nothing in the chat.
pub const CONFIRM_PATH: &str = "/api/v1/telegram/login/confirm";

/// Read the Bond profile of the chat's current product session.
pub const CHAT_PROFILE_PATH: &str = "/api/v1/telegram/profile";

/// The verified sender and chat of the `/login` command and that message,
/// as sent at login start. The fields and checks are
/// those of [`LoginStartRequest`]; it selects no identity either.
pub type LoginConfirmRequest = LoginStartRequest;

/// The chat is signed in to its own product session. Under Single Active
/// Client it may start inactive; the activation handoff is the only way to
/// make it active while another client of the owner is.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginConfirmResponse {
    /// The owner's public product profile id, not a credential.
    pub profile_id: String,
    /// The public address display snapshot, if any.
    pub pub_dress: Option<String>,
    /// Whether the chat is now the owner's active client.
    pub active: bool,
}

refusals! {
    /// Why a confirmation did not open a session. None of these is a login.
    /// `ConfirmationNotFound`: no verified callback waits for this sender,
    /// chat and message; the browser step is unfinished, expired or used.
    /// `LoginFailed`: the browser step reached the callback and failed, so
    /// this /login cannot succeed any more; the bot stops asking.
    LoginConfirmRefusal {
        InvalidProviderEvidence => ("invalid_provider_evidence", 422),
        ServiceAuthenticationRequired => ("service_authentication_required", 401),
        ConfirmationNotFound => ("login_confirmation_not_found", 404),
        LoginFailed => ("login_failed", 410),
        LoginServiceUnavailable => ("login_service_unavailable", 503),
    }
}

/// POST [`ChatActivationQuery`], with the same authenticated bot-service
/// credentials as login start: the chat's own
/// [`crate::activation::ActivationState`]. While the chat is the active
/// client and a device asks, its state is `approval_requested` and the bot
/// shows approve and decline. No chat session answers
/// `provider_authentication_required` (401).
pub const CHAT_ACTIVATION_PATH: &str = "/api/v1/telegram/activation";

/// POST [`ChatActivationAnswer`]: the chat approves or declines the request
/// `request_id`, which only its own sender can press in its own chat. The
/// chat keeps no device state; its data is parked already, so approving is
/// the switch. The answer is the chat's state afterwards, refusals are
/// [`crate::activation::ActivationRefusal`] codes.
pub const CHAT_ACTIVATION_ANSWER_PATH: &str = "/api/v1/telegram/activation/answer";

/// The verified sender of a private chat, as at login start.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatActivationQuery {
    /// The expected wire contract.
    pub version: u8,
    /// Only Telegram.
    pub provider: String,
    /// Verified Telegram sender id.
    pub provider_actor_id: u64,
    /// The private chat, the sender's own.
    pub chat_id: i64,
}

impl ChatActivationQuery {
    /// Whether the fields have the shape login start accepts.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.version == WIRE_VERSION
            && self.provider == "telegram"
            && self.provider_actor_id > 0
            && i64::try_from(self.provider_actor_id).ok() == Some(self.chat_id)
    }
}

/// Section requested from the signed chat profile endpoint.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatProfileSection {
    #[default]
    Bond,
    Avaia,
    Wallet,
}

/// Existing clients may omit the section to read the Bond projection.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatProfileQuery {
    pub version: u8,
    pub provider: String,
    pub provider_actor_id: u64,
    pub chat_id: i64,
    #[serde(default)]
    pub section: ChatProfileSection,
}

impl ChatProfileQuery {
    #[must_use]
    pub fn chat(self) -> ChatActivationQuery {
        ChatActivationQuery {
            version: self.version,
            provider: self.provider,
            provider_actor_id: self.provider_actor_id,
            chat_id: self.chat_id,
        }
    }
}

/// The configured Avaia and last stored position of the session's own Bond.
#[derive(Debug, Serialize)]
pub struct ChatAvaiaProfile {
    pub avaia: Option<ChatAvaia>,
}

#[derive(Debug, Serialize)]
pub struct ChatAvaia {
    pub pub_dress: String,
    pub configured: bool,
    pub location: Option<ChatAvaiaLocation>,
}

#[derive(Debug, Serialize)]
pub struct ChatAvaiaLocation {
    pub latitude_e7: i32,
    pub longitude_e7: i32,
}

/// The service ledger balance. An unbound profile has no wallet projection.
#[derive(Debug, Serialize)]
pub struct ChatWalletProfile {
    pub seeds: Option<u64>,
}

/// The chat's answer to an activation request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatActivationAnswer {
    /// The expected wire contract.
    pub version: u8,
    /// Only Telegram.
    pub provider: String,
    /// Verified Telegram sender id of the press.
    pub provider_actor_id: u64,
    /// The private chat, the sender's own.
    pub chat_id: i64,
    /// The request shown to the chat.
    pub request_id: String,
    /// Approve, or decline.
    pub approve: bool,
}

impl ChatActivationAnswer {
    /// The sender and chat, as a query.
    #[must_use]
    pub fn chat(&self) -> ChatActivationQuery {
        ChatActivationQuery {
            version: self.version,
            provider: self.provider.clone(),
            provider_actor_id: self.provider_actor_id,
            chat_id: self.chat_id,
        }
    }
}

/// The header that carries the bot's request signature.
pub const SIGNATURE_HEADER: &str = "x-0x1-telegram-bot-signature";
/// How far a signature's timestamp may be from the host's clock.
pub const SIGNATURE_MAX_SKEW_SECONDS: u64 = 60;

/// The numeric bot id that prefixes a Telegram Bot API token. The host names
/// the workload after it: `telegram-bot:<id>`.
#[must_use]
pub fn bot_id(bot_token: &str) -> Option<u64> {
    let (id, secret) = bot_token.split_once(':')?;
    if secret.is_empty() || id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse().ok().filter(|id| *id > 0)
}

/// The workload subject of the bot that holds `bot_token`.
#[must_use]
pub fn workload_subject(bot_token: &str) -> Option<String> {
    bot_id(bot_token).map(|id| format!("telegram-bot:{id}"))
}

fn signature_key(bot_token: &str) -> [u8; 32] {
    use hmac::Mac as _;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(b"0x1-telegram-bot-request-v1")
        .expect("HMAC accepts any key length");
    mac.update(bot_token.as_bytes());
    mac.finalize().into_bytes().into()
}

fn request_mac(
    bot_token: &str,
    timestamp: u64,
    path: &str,
    body: &[u8],
) -> hmac::Hmac<sha2::Sha256> {
    use hmac::Mac as _;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&signature_key(bot_token))
        .expect("HMAC accepts any key length");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b"\n");
    mac.update(path.as_bytes());
    mac.update(b"\n");
    mac.update(body);
    mac
}

/// Signs a bot request with the Telegram Bot API token the bot and the host
/// already hold, so the bot needs no credential of its own. Whoever holds the
/// token already controls the chat; the signature adds no new authority.
/// Value: `t=<unix seconds>,v1=<lowercase hex HMAC-SHA256>`.
#[must_use]
pub fn sign_request(bot_token: &str, timestamp: u64, path: &str, body: &[u8]) -> String {
    use hmac::Mac as _;
    let tag = request_mac(bot_token, timestamp, path, body)
        .finalize()
        .into_bytes();
    let mut value = format!("t={timestamp},v1=");
    for byte in tag {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

/// Verifies [`sign_request`] in constant time, within
/// [`SIGNATURE_MAX_SKEW_SECONDS`] of `now`. Replays inside that window reach
/// only idempotent login start or one-use confirmation.
#[must_use]
pub fn verify_request(bot_token: &str, header: &str, path: &str, body: &[u8], now: u64) -> bool {
    use hmac::Mac as _;
    let Some((timestamp, tag)) = header
        .strip_prefix("t=")
        .and_then(|rest| rest.split_once(",v1="))
    else {
        return false;
    };
    let Ok(timestamp) = timestamp.parse::<u64>() else {
        return false;
    };
    if timestamp.abs_diff(now) > SIGNATURE_MAX_SKEW_SECONDS || tag.len() != 64 {
        return false;
    }
    let Some(tag) = (0..32)
        .map(|index| u8::from_str_radix(tag.get(index * 2..index * 2 + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
    else {
        return false;
    };
    request_mac(bot_token, timestamp, path, body)
        .verify_slice(&tag)
        .is_ok()
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
        assert_eq!(
            wrong.validate(),
            Err(LoginStartRefusal::UnsupportedProvider)
        );
    }

    #[test]
    fn response_is_a_pending_link_not_a_session() {
        let response = LoginStartResponse {
            authorization_url: "https://auth.aiaiaiai.org/0x1?state=example".into(),
            expires_in_seconds: 300,
        };
        assert!(response.has_valid_ttl());
        assert_eq!(
            serde_json::from_str::<LoginStartResponse>(&serde_json::to_string(&response).unwrap())
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

    #[test]
    fn confirmation_wire_is_closed_and_has_stable_refusals() {
        let response = LoginConfirmResponse {
            profile_id: "p".into(),
            pub_dress: None,
            active: false,
        };
        assert_eq!(
            serde_json::to_string(&response).unwrap(),
            r#"{"profile_id":"p","pub_dress":null,"active":false}"#
        );
        assert!(
            serde_json::from_str::<LoginConfirmResponse>(
                r#"{"profile_id":"p","pub_dress":null,"active":true,"token":"t"}"#
            )
            .is_err()
        );
        assert_eq!(
            LoginConfirmRefusal::from_code("login_confirmation_not_found"),
            Some(LoginConfirmRefusal::ConfirmationNotFound)
        );
        assert_eq!(LoginConfirmRefusal::ConfirmationNotFound.http_status(), 404);
    }

    #[test]
    fn bot_requests_are_signed_with_the_existing_bot_token_only() {
        let token = "123456:secret-part";
        assert_eq!(bot_id(token), Some(123_456));
        assert_eq!(
            workload_subject(token).as_deref(),
            Some("telegram-bot:123456")
        );
        for bad in ["", "123456", "abc:secret", ":secret", "0:secret", "123456:"] {
            assert_eq!(bot_id(bad), None, "{bad}");
        }
        let body = br#"{"version":1}"#;
        let header = sign_request(token, 1_000, START_PATH, body);
        assert!(header.starts_with("t=1000,v1="));
        assert!(verify_request(token, &header, START_PATH, body, 1_030));
        assert!(
            !verify_request(token, &header, START_PATH, body, 1_061),
            "stale"
        );
        assert!(
            !verify_request(token, &header, CONFIRM_PATH, body, 1_000),
            "other route"
        );
        assert!(
            !verify_request(token, &header, START_PATH, b"{}", 1_000),
            "other body"
        );
        assert!(
            !verify_request("123456:other", &header, START_PATH, body, 1_000),
            "other token"
        );
        assert!(!verify_request(
            token,
            "t=1000,v1=zz",
            START_PATH,
            body,
            1_000
        ));
        assert!(!verify_request(token, "garbage", START_PATH, body, 1_000));
    }
}
