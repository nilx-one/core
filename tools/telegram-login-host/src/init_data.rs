// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Telegram Mini App `initData`, verified with the Bot API token this host
//! already holds (core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app).
//! It proves which Telegram account opened the login page inside Telegram;
//! it is not an AIS identity, a Bond or a product session.

use std::collections::BTreeMap;

use hmac::{Hmac, Mac as _};
use serde::Deserialize;
use sha2::Sha256;

const MAX_AGE_SECONDS: u64 = 300;
const FUTURE_SKEW_SECONDS: u64 = 30;
const MAX_INIT_DATA_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitDataRejection {
    Malformed,
    InvalidHash,
    Expired,
    InvalidUser,
}

impl InitDataRejection {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Malformed => "init_data_malformed",
            Self::InvalidHash => "init_data_signature",
            Self::Expired => "init_data_expired",
            Self::InvalidUser => "init_data_user",
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
/// for this bot within the last five minutes.
pub fn verify(bot_token: &str, init_data: &str, now: u64) -> Result<u64, InitDataRejection> {
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Signs `fields` the way Telegram does, for tests.
    pub(crate) fn signed(bot_token: &str, fields: &[(&str, &str)]) -> String {
        let mut sorted: Vec<_> = fields.to_vec();
        sorted.sort();
        let check = sorted
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut secret = Hmac::<Sha256>::new_from_slice(b"WebAppData").unwrap();
        secret.update(bot_token.as_bytes());
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret.finalize().into_bytes()).unwrap();
        mac.update(check.as_bytes());
        let hash: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut encoded = url::form_urlencoded::Serializer::new(String::new());
        for (key, value) in fields {
            encoded.append_pair(key, value);
        }
        encoded.append_pair("hash", &hash);
        encoded.finish()
    }

    const TOKEN: &str = "123456:test-bot-token";

    #[test]
    fn accepts_only_fresh_data_telegram_signed_for_this_bot() {
        let user = r#"{"id":42,"first_name":"A"}"#;
        let data = signed(
            TOKEN,
            &[("auth_date", "1000"), ("user", user), ("query_id", "q")],
        );
        assert_eq!(verify(TOKEN, &data, 1_100), Ok(42));
        assert_eq!(
            verify("123456:other", &data, 1_100),
            Err(InitDataRejection::InvalidHash)
        );
        assert_eq!(verify(TOKEN, &data, 1_301), Err(InitDataRejection::Expired));
        assert_eq!(verify(TOKEN, &data, 900), Err(InitDataRejection::Expired));
        let forged = data.replace("%22id%22%3A42", "%22id%22%3A43");
        assert_eq!(
            verify(TOKEN, &forged, 1_100),
            Err(InitDataRejection::InvalidHash)
        );
        let bot = signed(
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
