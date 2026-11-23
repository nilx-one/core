// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The product host's own direct AIS grant for Telegram provider evidence.
//! AIS issues it to the registered `0x1` product key for audience `ais` and
//! scope `telegram.identity.resolve` only. Neither the browser nor the bot
//! ever sees it.

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer as _, SigningKey};
use reqwest::{Client, redirect::Policy};
use serde::Deserialize;

use crate::now_unix_seconds;

const CLIENT_ID: &str = "0x1";
const AUDIENCE: &str = "ais";
const SCOPE: &str = "telegram.identity.resolve";
const ASSERTION_TYPE: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";
const ASSERTION_LIFETIME_SECONDS: u64 = 60;

#[derive(Clone)]
pub struct HostAisToken {
    http: Client,
    token_endpoint: String,
    kid: String,
    key: SigningKey,
}

impl HostAisToken {
    /// `seed` is the base64url (unpadded) 32-byte Ed25519 seed whose public
    /// key AIS registered for the `0x1` product under `kid`.
    pub fn new(issuer: &str, kid: &str, seed: &str) -> Option<Self> {
        let seed: [u8; 32] = URL_SAFE_NO_PAD.decode(seed.trim()).ok()?.try_into().ok()?;
        let valid_kid = (1..=64).contains(&kid.len())
            && kid
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        let issuer = url::Url::parse(issuer).ok()?;
        if !valid_kid || issuer.scheme() != "https" || issuer.host_str().is_none() {
            return None;
        }
        let http = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(6))
            .build()
            .ok()?;
        Some(Self {
            http,
            token_endpoint: format!("{}/token", issuer.as_str().trim_end_matches('/')),
            kid: kid.to_owned(),
            key: SigningKey::from_bytes(&seed),
        })
    }

    fn assertion(&self, now: u64) -> Option<String> {
        let mut nonce = [0_u8; 18];
        getrandom::fill(&mut nonce).ok()?;
        let header = serde_json::json!({"alg": "EdDSA", "kid": self.kid});
        let claims = serde_json::json!({
            "iss": CLIENT_ID, "sub": CLIENT_ID, "aud": self.token_endpoint,
            "iat": now, "exp": now + ASSERTION_LIFETIME_SECONDS, "jti": URL_SAFE_NO_PAD.encode(nonce),
        });
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self.key.sign(signed.as_bytes());
        Some(format!(
            "{signed}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        ))
    }

    /// A fresh grant per callback: a callback is rare and one-use, so the
    /// host keeps no token between requests.
    pub async fn access_token(&self) -> Option<String> {
        #[derive(Deserialize)]
        struct Grant {
            access_token: String,
            token_type: String,
            expires_in: u64,
            #[serde(default)]
            scope: Option<String>,
        }
        let assertion = self.assertion(now_unix_seconds()?)?;
        let response = self
            .http
            .post(&self.token_endpoint)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", CLIENT_ID),
                ("audience", AUDIENCE),
                ("scope", SCOPE),
                ("client_assertion_type", ASSERTION_TYPE),
                ("client_assertion", assertion.as_str()),
            ])
            .send()
            .await
            .map_err(|error| tracing::error!(%error, "AIS Telegram grant request failed"))
            .ok()?;
        if !response.status().is_success() {
            tracing::error!(status = %response.status(), "AIS refused the Telegram grant");
            return None;
        }
        let grant: Grant = response.json().await.ok()?;
        let scoped = grant.scope.as_deref().is_none_or(|scope| scope == SCOPE);
        (scoped
            && grant.token_type.eq_ignore_ascii_case("Bearer")
            && (1..=300).contains(&grant.expires_in)
            && (1..=4096).contains(&grant.access_token.len()))
        .then_some(grant.access_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier as _};

    #[test]
    fn assertion_names_the_0x1_product_and_the_token_endpoint_only() {
        let seed = URL_SAFE_NO_PAD.encode([7u8; 32]);
        let token = HostAisToken::new("https://auth.aiaiaiai.org", "host-1", &seed).unwrap();
        let assertion = token.assertion(1_000).unwrap();
        let (signed, signature) = assertion.rsplit_once('.').unwrap();
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).unwrap()).unwrap();
        SigningKey::from_bytes(&[7; 32])
            .verifying_key()
            .verify(signed.as_bytes(), &signature)
            .unwrap();
        let claims: serde_json::Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(signed.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["iss"], "0x1");
        assert_eq!(claims["sub"], "0x1");
        assert_eq!(claims["aud"], "https://auth.aiaiaiai.org/token");
        assert_eq!(claims["exp"], 1_060);
        assert!(claims["jti"].as_str().unwrap().len() >= 16);
    }

    #[test]
    fn refuses_weak_configuration() {
        let seed = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert!(HostAisToken::new("http://auth.aiaiaiai.org", "k", &seed).is_none());
        assert!(HostAisToken::new("https://auth.aiaiaiai.org", "", &seed).is_none());
        assert!(HostAisToken::new("https://auth.aiaiaiai.org", "k/1", &seed).is_none());
        assert!(HostAisToken::new("https://auth.aiaiaiai.org", "k", "short").is_none());
    }
}
