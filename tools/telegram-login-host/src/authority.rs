// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Direct AIS workload grants for the isolated Telegram login-start route.

use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::VerifyingKey;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use nilxone_api::bot_login::LoginStartRefusal;
use serde::Deserialize;

use crate::{AuthorizedLoginBot, now_unix_seconds, refusal};

pub const ISSUER: &str = "https://auth.aiaiaiai.org";
const CLIENT: &str = "0x1-telegram-test-bot";
const SCOPE: &str = "telegram.login.start";

#[derive(Clone)]
pub struct Authority {
    kid: String,
    key: DecodingKey,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claims {
    iss: String,
    aud: String,
    sub: String,
    client_id: String,
    scope: String,
    jti: String,
    iat: u64,
    exp: u64,
}

impl Authority {
    /// A trusted public key provisioned by the operator from the AIS JWKS.
    pub fn from_public_jwk(json: &str) -> Option<Self> {
        #[derive(Deserialize)]
        struct Jwk {
            kid: String,
            kty: String,
            crv: String,
            alg: String,
            #[serde(rename = "use")]
            usage: String,
            x: String,
        }
        let jwk: Jwk = serde_json::from_str(json).ok()?;
        if jwk.kid.is_empty()
            || jwk.kty != "OKP"
            || jwk.crv != "Ed25519"
            || jwk.alg != "EdDSA"
            || jwk.usage != "sig"
        {
            return None;
        }
        let bytes: [u8; 32] = URL_SAFE_NO_PAD.decode(&jwk.x).ok()?.try_into().ok()?;
        let key = VerifyingKey::from_bytes(&bytes).ok()?;
        if key.is_weak() {
            return None;
        }
        Some(Self {
            kid: jwk.kid,
            key: DecodingKey::from_ed_der(&bytes),
        })
    }

    fn accepts(&self, token: &str, now: u64) -> bool {
        if token.len() > 4096 {
            return false;
        }
        let Ok(h) = decode_header(token) else {
            return false;
        };
        if h.alg != Algorithm::EdDSA
            || h.typ.as_deref() != Some("at+jwt")
            || h.kid.as_ref() != Some(&self.kid)
        {
            return false;
        }
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.set_audience(&["0x1"]);
        validation.set_issuer(&[ISSUER]);
        validation.leeway = 0;
        let Ok(data) = decode::<Claims>(token, &self.key, &validation) else {
            return false;
        };
        let c = data.claims;
        c.iss == ISSUER
            && c.aud == "0x1"
            && c.sub == CLIENT
            && c.client_id == CLIENT
            && c.scope == SCOPE
            && !c.jti.is_empty()
            && c.iat <= now
            && c.exp > now
            && c.exp.saturating_sub(c.iat) <= 300
    }
}

pub(super) async fn authenticate(
    State(authority): State<Authority>,
    mut req: Request,
    next: Next,
) -> Response {
    let mut values = req.headers().get_all(AUTHORIZATION).iter();
    let token = values
        .next()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if values.next().is_some()
        || !token
            .zip(now_unix_seconds())
            .is_some_and(|(t, now)| authority.accepts(t, now))
    {
        return refusal(LoginStartRefusal::ServiceAuthenticationRequired);
    }
    req.extensions_mut()
        .insert(AuthorizedLoginBot(CLIENT.to_owned()));
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::{Value, json};

    fn signed(key: &SigningKey, claims: &Value, typ: &str) -> String {
        let header = json!({"alg":"EdDSA", "typ":typ, "kid":"ais-test"});
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(key.sign(input.as_bytes()).to_bytes())
        )
    }

    #[test]
    fn accepts_only_the_direct_scoped_test_bot_grant() {
        let key = SigningKey::from_bytes(&[19; 32]);
        let jwk = json!({"kid":"ais-test","kty":"OKP","crv":"Ed25519","alg":"EdDSA",
            "use":"sig","x":URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())});
        let authority = Authority::from_public_jwk(&jwk.to_string()).unwrap();
        let now = now_unix_seconds().unwrap();
        let claims = json!({"iss":ISSUER, "sub":CLIENT, "client_id":CLIENT, "aud":"0x1", "scope":SCOPE,
            "iat":now, "exp":now+300, "jti":"test-grant"});
        let token = signed(&key, &claims, "at+jwt");
        assert!(authority.accepts(&token, now));
        assert!(!authority.accepts(&token, now + 300));
        for (field, value) in [
            ("aud", json!("ais")),
            ("sub", json!("another-bot")),
            ("client_id", json!("another-bot")),
            ("iss", json!("https://other.example")),
            ("scope", json!("telegram.identity.resolve")),
            ("act", json!({"sub":CLIENT})),
            ("exp", json!(now + 301)),
            ("iat", json!(now + 1)),
        ] {
            let mut wrong = claims.clone();
            wrong[field] = value;
            assert!(
                !authority.accepts(&signed(&key, &wrong, "at+jwt"), now),
                "{field}"
            );
        }
        assert!(!authority.accepts(&signed(&key, &claims, "JWT"), now));
        assert!(!authority.accepts(
            &signed(&SigningKey::from_bytes(&[3; 32]), &claims, "at+jwt"),
            now
        ));
    }
}
