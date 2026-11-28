// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Public-key verification for the Telegram Bot workload. This is not
//! Telegram provider evidence, a Bond identity or an AIS session.

use ed25519_dalek::{Signature, VerifyingKey};
use nilxone_api::bot_login::SIGNATURE_MAX_SKEW_SECONDS;

const DOMAIN: &str = "0x1-telegram-bot-request-v2";

fn hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    if value.len() != N * 2
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut bytes = [0; N];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(value.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

#[derive(Clone)]
pub struct BotRequestVerifier(VerifyingKey);

impl BotRequestVerifier {
    #[must_use]
    pub fn from_public_key(value: &str) -> Option<Self> {
        let key = VerifyingKey::from_bytes(&hex(value)?).ok()?;
        (!key.is_weak()).then_some(Self(key))
    }

    /// No HMAC or caller-supplied public key fallback. Bind timestamp,
    /// endpoint and exact request bytes to the configured workload key.
    #[must_use]
    pub fn accepts(&self, header: &str, path: &str, body: &[u8], now: u64) -> bool {
        let Some((timestamp, tag)) = header.strip_prefix("t=").and_then(|s| s.split_once(",v2="))
        else {
            return false;
        };
        let Ok(timestamp) = timestamp.parse::<u64>() else {
            return false;
        };
        if timestamp.abs_diff(now) > SIGNATURE_MAX_SKEW_SECONDS {
            return false;
        }
        let Some(bytes) = hex::<64>(tag) else {
            return false;
        };
        let mut message = format!("{DOMAIN}\n{timestamp}\n{path}\n").into_bytes();
        message.extend_from_slice(body);
        self.0
            .verify_strict(&message, &Signature::from_bytes(&bytes))
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn encode(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
    }

    #[test]
    fn matches_the_bot_public_wire_vector() {
        let verifier = BotRequestVerifier::from_public_key(
            "635f952c2b4490eef64237b043885c1823b2d53ed48422e23a7216f7302cbf31",
        )
        .unwrap();
        let header = concat!(
            "t=1000,v2=fc6771c7e61afc165ec525874bf779f3c26ef2635d040ca16200e9c1bdbd30b7a3",
            "aab5944c512e662d19774dab0a58d9fb0dba2917cc3ffb1d14af2d6e883d05"
        );
        assert!(verifier.accepts(
            header,
            "/api/v1/telegram/login/start",
            br#"{"version":1}"#,
            1_000
        ));
    }

    #[test]
    fn verifies_only_the_pinned_public_key_and_exact_request() {
        let signing = SigningKey::from_bytes(&[23; 32]);
        let verifier =
            BotRequestVerifier::from_public_key(&encode(signing.verifying_key().as_bytes()))
                .unwrap();
        let body = br#"{"version":1}"#;
        let path = "/api/v1/telegram/login/start";
        let message = format!("{DOMAIN}\n1000\n{path}\n");
        let mut bytes = message.into_bytes();
        bytes.extend_from_slice(body);
        let signature = format!("t=1000,v2={}", encode(&signing.sign(&bytes).to_bytes()));
        assert!(verifier.accepts(&signature, path, body, 1_000));
        assert!(!verifier.accepts(&signature, path, body, 1_061));
        assert!(!verifier.accepts(&signature, path, b"{}", 1_000));
        assert!(!verifier.accepts(&signature, "/other", body, 1_000));
        assert!(!verifier.accepts(&signature.replace(",v2=", ",v1="), path, body, 1_000));
        let other = SigningKey::from_bytes(&[24; 32]);
        let wrong =
            BotRequestVerifier::from_public_key(&encode(other.verifying_key().as_bytes())).unwrap();
        assert!(!wrong.accepts(&signature, path, body, 1_000));
    }
}
