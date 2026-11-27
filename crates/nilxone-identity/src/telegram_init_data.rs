// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Telegram Mini App `initData`, verified with the Bot API token
//! (core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app).
//! It proves which Telegram account opened a page inside Telegram; it is not
//! an AIS identity, a Bond or a product session.

use std::collections::BTreeMap;

use base64::{
    Engine as _,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, VerifyingKey};
use hmac::{Hmac, Mac as _};
use serde::Deserialize;
use sha2::Sha256;

const MAX_AGE_SECONDS: u64 = 600;
const FUTURE_SKEW_SECONDS: u64 = 30;
const MAX_INIT_DATA_BYTES: usize = 4096;
// Telegram's production third-party validation key.
const PRODUCTION_KEY: [u8; 32] = [
    0xe7, 0xbf, 0x03, 0xa2, 0xfa, 0x46, 0x02, 0xaf, 0x45, 0x80, 0x70, 0x3d, 0x88, 0xdd, 0xa5, 0xbb,
    0x59, 0xf3, 0x2e, 0xd8, 0xb0, 0x2a, 0x56, 0xc1, 0x87, 0xfe, 0x7d, 0x34, 0xca, 0xed, 0x24, 0x2d,
];

/// Provider evidence is separate from the Bot workload's request signature.
/// The production host uses only the public bot ID, never the API token.
#[derive(Clone, Copy)]
pub enum InitDataProof<'a> {
    LegacyHmac(&'a str),
    PublicBot(u64),
}

impl InitDataProof<'_> {
    pub fn verify(self, init_data: &str, now: u64) -> Result<u64, InitDataRejection> {
        match self {
            Self::LegacyHmac(token) => verify(token, init_data, now),
            Self::PublicBot(id) => verify_public(id, init_data, now),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitDataRejection {
    Malformed,
    InvalidHash,
    Expired,
    InvalidUser,
}

impl InitDataRejection {
    /// A fixed label for logs.
    #[must_use]
    pub fn reason(self) -> &'static str {
        match self {
            Self::Malformed => "telegram_init_data_malformed",
            Self::InvalidHash => "telegram_init_data_signature",
            Self::Expired => "telegram_init_data_expired",
            Self::InvalidUser => "telegram_init_data_user",
        }
    }
}

#[derive(Deserialize)]
struct TelegramUser {
    id: i64,
    #[serde(default)]
    is_bot: bool,
}

/// The Telegram user id `init_data` was signed for, if Telegram signed it
/// for this bot within the last ten minutes (time to sign in to AIS).
///
/// # Errors
///
/// Anything Telegram did not sign for this bot, stale data, or a bot user.
pub fn verify(bot_token: &str, init_data: &str, now: u64) -> Result<u64, InitDataRejection> {
    let mut fields = parse_fields(init_data)?;
    let hash = fields
        .remove("hash")
        .and_then(|hash| decode_hex(&hash))
        .ok_or(InitDataRejection::InvalidHash)?;
    let check = fields
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut secret = Hmac::<Sha256>::new_from_slice(b"WebAppData").expect("any key length");
    secret.update(bot_token.as_bytes());
    let secret = secret.finalize().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret).expect("any key length");
    mac.update(check.as_bytes());
    mac.verify_slice(&hash)
        .map_err(|_| InitDataRejection::InvalidHash)?;
    verified_user(&fields, now)
}

/// Verify Telegram's third-party Ed25519 evidence, without an HMAC fallback.
pub fn verify_public(bot_id: u64, init_data: &str, now: u64) -> Result<u64, InitDataRejection> {
    let key = VerifyingKey::from_bytes(&PRODUCTION_KEY)
        .map_err(|_| InitDataRejection::InvalidHash)?;
    verify_public_with_key(bot_id, init_data, now, &key)
}

fn verify_public_with_key(
    bot_id: u64,
    init_data: &str,
    now: u64,
    key: &VerifyingKey,
) -> Result<u64, InitDataRejection> {
    if bot_id == 0 {
        return Err(InitDataRejection::InvalidHash);
    }
    let mut fields = parse_fields(init_data)?;
    let encoded = fields.remove("signature").ok_or(InitDataRejection::InvalidHash)?;
    fields.remove("hash");
    let bytes = URL_SAFE_NO_PAD.decode(&encoded).or_else(|_| URL_SAFE.decode(&encoded))
        .map_err(|_| InitDataRejection::InvalidHash)?;
    let signature = Signature::from_slice(&bytes).map_err(|_| InitDataRejection::InvalidHash)?;
    let check = fields
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let message = format!("{bot_id}:WebAppData\n{check}");
    key.verify_strict(message.as_bytes(), &signature)
        .map_err(|_| InitDataRejection::InvalidHash)?;
    verified_user(&fields, now)
}

fn parse_fields(init_data: &str) -> Result<BTreeMap<String, String>, InitDataRejection> {
    if init_data.is_empty() || init_data.len() > MAX_INIT_DATA_BYTES {
        return Err(InitDataRejection::Malformed);
    }
    let mut fields = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(init_data.as_bytes()) {
        if fields
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(InitDataRejection::Malformed);
        }
    }
    Ok(fields)
}

fn verified_user(fields: &BTreeMap<String, String>, now: u64) -> Result<u64, InitDataRejection> {
    let auth_date: u64 = fields
        .get("auth_date")
        .and_then(|value| value.parse().ok())
        .ok_or(InitDataRejection::Malformed)?;
    if auth_date > now.saturating_add(FUTURE_SKEW_SECONDS)
        || now.saturating_sub(auth_date) > MAX_AGE_SECONDS
    {
        return Err(InitDataRejection::Expired);
    }
    let user: TelegramUser = fields
        .get("user")
        .and_then(|user| serde_json::from_str(user).ok())
        .ok_or(InitDataRejection::InvalidUser)?;
    if user.is_bot {
        return Err(InitDataRejection::InvalidUser);
    }
    u64::try_from(user.id)
        .ok()
        .filter(|id| *id > 0)
        .ok_or(InitDataRejection::InvalidUser)
}

fn decode_hex(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

/// Signs `fields` the way Telegram does. For tests of code that verifies.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn sign_for_tests(bot_token: &str, fields: &[(&str, &str)]) -> String {
    let mut sorted: Vec<_> = fields.to_vec();
    sorted.sort_unstable();
    let check = sorted
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut secret = Hmac::<Sha256>::new_from_slice(b"WebAppData").expect("any key length");
    secret.update(bot_token.as_bytes());
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&secret.finalize().into_bytes()).expect("any key length");
    mac.update(check.as_bytes());
    let mut encoded = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in fields {
        encoded.append_pair(key, value);
    }
    let hash: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .fold(String::new(), |mut hex, byte| {
            use std::fmt::Write as _;
            let _ = write!(hex, "{byte:02x}");
            hex
        });
    encoded.append_pair("hash", &hash);
    encoded.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::{Signer, SigningKey};

    const TOKEN: &str = "123456:test-bot-token";

    #[test]
    fn public_evidence_is_bound_to_bot_and_has_no_legacy_fallback() {
        let signing = SigningKey::from_bytes(&[42; 32]);
        let user = r#"{"id":42}"#;
        let msg = format!("123456:WebAppData\nauth_date=1000\nuser={user}");
        let signature = URL_SAFE_NO_PAD.encode(signing.sign(msg.as_bytes()).to_bytes());
        let data = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("user", user)
            .append_pair("auth_date", "1000")
            .append_pair("signature", &signature)
            .finish();
        let key = signing.verifying_key();
        assert_eq!(verify_public_with_key(123_456, &data, 1_100, &key), Ok(42));
        assert_eq!(verify_public_with_key(123_457, &data, 1_100, &key), Err(InitDataRejection::InvalidHash));
        assert_eq!(verify_public_with_key(123_456, &data, 1_601, &key), Err(InitDataRejection::Expired));
        let legacy = sign_for_tests(TOKEN, &[("auth_date", "1000"), ("user", user)]);
        assert_eq!(verify_public_with_key(123_456, &legacy, 1_100, &key), Err(InitDataRejection::InvalidHash));
    }

    #[test]
    fn accepts_only_fresh_data_telegram_signed_for_this_bot() {
        let user = r#"{"id":42,"first_name":"A"}"#;
        let data = sign_for_tests(
            TOKEN,
            &[("auth_date", "1000"), ("user", user), ("query_id", "q")],
        );
        assert_eq!(verify(TOKEN, &data, 1_100), Ok(42));
        assert_eq!(
            verify("123456:other", &data, 1_100),
            Err(InitDataRejection::InvalidHash)
        );
        assert_eq!(verify(TOKEN, &data, 1_601), Err(InitDataRejection::Expired));
        assert_eq!(verify(TOKEN, &data, 900), Err(InitDataRejection::Expired));
        let forged = data.replace("%22id%22%3A42", "%22id%22%3A43");
        assert_eq!(
            verify(TOKEN, &forged, 1_100),
            Err(InitDataRejection::InvalidHash)
        );
        let bot = sign_for_tests(
            TOKEN,
            &[("auth_date", "1000"), ("user", r#"{"id":7,"is_bot":true}"#)],
        );
        assert_eq!(
            verify(TOKEN, &bot, 1_100),
            Err(InitDataRejection::InvalidUser)
        );
        assert_eq!(verify(TOKEN, "", 1_100), Err(InitDataRejection::Malformed));
        assert_eq!(
            verify(TOKEN, &format!("{data}&auth_date=1"), 1_100),
            Err(InitDataRejection::Malformed)
        );
    }
}
