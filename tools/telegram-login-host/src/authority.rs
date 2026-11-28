// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Bot workload authentication through a pinned public verification key.
//! The production host cannot call Telegram Bot API or sign Bot requests.

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use nilxone_api::bot_login::{LoginStartRefusal, SIGNATURE_HEADER};
#[cfg(test)]
use nilxone_api::bot_login::{verify_request, workload_subject};

use crate::{AuthorizedLoginBot, now_unix_seconds, refusal};
use nilxone_identity::{
    telegram_bot_signature::BotRequestVerifier, telegram_init_data::InitDataProof,
};

pub const ISSUER: &str = "https://auth.aiaiaiai.org";
/// Login requests are small JSON documents.
const MAX_BODY_BYTES: usize = 4_096;

#[derive(Clone)]
pub struct Authority {
    proof: RequestProof,
    subject: String,
}

#[derive(Clone)]
enum RequestProof {
    #[cfg(test)]
    LegacyHmac(String),
    Public {
        bot_id: u64,
        verifier: BotRequestVerifier,
    },
}

impl Authority {
    /// Legacy fixture support only; never compiled into the production host.
    #[cfg(test)]
    pub fn from_bot_token(bot_token: &str) -> Option<Self> {
        let bot_token = bot_token.trim();
        Some(Self {
            subject: workload_subject(bot_token)?,
            proof: RequestProof::LegacyHmac(bot_token.to_owned()),
        })
    }

    /// The independently configured public identity and verification key.
    pub fn from_public_key(bot_id: u64, public_key: &str) -> Option<Self> {
        if bot_id == 0 {
            return None;
        }
        Some(Self {
            subject: format!("telegram-bot:{bot_id}"),
            proof: RequestProof::Public {
                bot_id,
                verifier: BotRequestVerifier::from_public_key(public_key)?,
            },
        })
    }

    pub(crate) fn init_data_proof(&self) -> InitDataProof<'_> {
        match &self.proof {
            #[cfg(test)]
            RequestProof::LegacyHmac(token) => InitDataProof::LegacyHmac(token),
            RequestProof::Public { bot_id, .. } => InitDataProof::PublicBot(*bot_id),
        }
    }

    fn accepts(&self, header: &str, path: &str, body: &[u8], now: u64) -> bool {
        match &self.proof {
            #[cfg(test)]
            RequestProof::LegacyHmac(token) => verify_request(token, header, path, body, now),
            RequestProof::Public { verifier, .. } => verifier.accepts(header, path, body, now),
        }
    }
}

pub(super) async fn authenticate(
    State(authority): State<Authority>,
    request: Request,
    next: Next,
) -> Response {
    let (parts, body) = request.into_parts();
    let mut values = parts.headers.get_all(SIGNATURE_HEADER).iter();
    let header = values.next().and_then(|value| value.to_str().ok());
    let single = values.next().is_none();
    let Ok(body) = to_bytes(body, MAX_BODY_BYTES).await else {
        return refusal(LoginStartRefusal::ServiceAuthenticationRequired);
    };
    let accepted = single
        && header
            .zip(now_unix_seconds())
            .is_some_and(|(header, now)| authority.accepts(header, parts.uri.path(), &body, now));
    if !accepted {
        return refusal(LoginStartRefusal::ServiceAuthenticationRequired);
    }
    let mut request = Request::from_parts(parts, Body::from(body));
    request
        .extensions_mut()
        .insert(AuthorizedLoginBot(authority.subject));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, http::StatusCode, routing::post};
    use ed25519_dalek::{Signer, SigningKey};
    use nilxone_api::bot_login::{GATEWAY_HEALTH_PATH, START_PATH, sign_request};
    use tower::ServiceExt as _;

    const TOKEN: &str = "123456:test-bot-token";

    fn app_with(authority: Authority) -> Router {
        Router::new()
            .route(
                START_PATH,
                post(
                    |axum::Extension(AuthorizedLoginBot(subject)): axum::Extension<
                        AuthorizedLoginBot,
                    >,
                     body: String| async move { format!("{subject} {body}") },
                ),
            )
            .route(GATEWAY_HEALTH_PATH, post(crate::gateway_health))
            .route_layer(axum::middleware::from_fn_with_state(
                authority,
                authenticate,
            ))
    }

    fn app() -> Router {
        app_with(Authority::from_bot_token(TOKEN).unwrap())
    }

    fn encode(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        bytes.iter().fold(String::new(), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
    }

    fn public_authority(key: &SigningKey) -> Authority {
        Authority::from_public_key(123_456, &encode(key.verifying_key().as_bytes())).unwrap()
    }

    fn public_signature(key: &SigningKey, now: u64, path: &str, body: &[u8]) -> String {
        let mut message = format!("0x1-telegram-bot-request-v2\n{now}\n{path}\n").into_bytes();
        message.extend_from_slice(body);
        format!("t={now},v2={}", encode(&key.sign(&message).to_bytes()))
    }

    #[tokio::test]
    async fn production_middleware_accepts_only_public_v2_authority() {
        let key = SigningKey::from_bytes(&[23; 32]);
        let authority = public_authority(&key);
        let now = now_unix_seconds().unwrap();
        let body = r#"{"version":1}"#;
        let ok = app_with(authority.clone())
            .oneshot(request(
                Some(public_signature(&key, now, START_PATH, body.as_bytes())),
                body,
            ))
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let text = to_bytes(ok.into_body(), 1024).await.unwrap();
        assert_eq!(&text[..], format!("telegram-bot:123456 {body}").as_bytes());

        for signature in [
            None,
            Some(sign_request(TOKEN, now, START_PATH, body.as_bytes())),
            Some(public_signature(
                &key,
                now - 120,
                START_PATH,
                body.as_bytes(),
            )),
            Some(public_signature(&key, now, "/other", body.as_bytes())),
            Some(public_signature(&key, now, START_PATH, b"{}")),
            Some(public_signature(
                &SigningKey::from_bytes(&[24; 32]),
                now,
                START_PATH,
                body.as_bytes(),
            )),
        ] {
            let response = app_with(authority.clone())
                .oneshot(request(signature, body))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(json["error"]["code"], "service_authentication_required");
        }
    }

    #[tokio::test]
    async fn health_probe_requires_the_same_public_authority_and_is_no_store() {
        let key = SigningKey::from_bytes(&[23; 32]);
        let authority = public_authority(&key);
        let now = now_unix_seconds().unwrap();
        for signed in [false, true] {
            let mut builder = Request::post(GATEWAY_HEALTH_PATH);
            if signed {
                builder = builder.header(
                    SIGNATURE_HEADER,
                    public_signature(&key, now, GATEWAY_HEALTH_PATH, b"{}"),
                );
            }
            let response = app_with(authority.clone())
                .oneshot(builder.body(Body::from("{}")).unwrap())
                .await
                .unwrap();
            if signed {
                assert_eq!(response.status(), StatusCode::NO_CONTENT);
                assert_eq!(response.headers()["cache-control"], "no-store");
                assert!(
                    to_bytes(response.into_body(), 1024)
                        .await
                        .unwrap()
                        .is_empty()
                );
            } else {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            }
        }
        assert!(matches!(
            authority.init_data_proof(),
            InitDataProof::PublicBot(123_456)
        ));
    }

    fn request(signature: Option<String>, body: &'static str) -> Request {
        let mut builder = Request::post(START_PATH);
        if let Some(signature) = signature {
            builder = builder.header(SIGNATURE_HEADER, signature);
        }
        builder.body(Body::from(body)).unwrap()
    }

    #[tokio::test]
    async fn only_a_fresh_signature_of_this_bot_token_passes() {
        let now = now_unix_seconds().unwrap();
        let body = r#"{"version":1}"#;
        let ok = app()
            .oneshot(request(
                Some(sign_request(TOKEN, now, START_PATH, body.as_bytes())),
                body,
            ))
            .await
            .unwrap();
        assert_eq!(ok.status(), StatusCode::OK);
        let text = to_bytes(ok.into_body(), 1024).await.unwrap();
        assert_eq!(&text[..], format!("telegram-bot:123456 {body}").as_bytes());

        for signature in [
            None,
            Some(sign_request(
                "123456:other-token",
                now,
                START_PATH,
                body.as_bytes(),
            )),
            Some(sign_request(TOKEN, now - 120, START_PATH, body.as_bytes())),
            Some(sign_request(TOKEN, now, START_PATH, b"{}")),
            Some(sign_request(
                TOKEN,
                now,
                "/api/v1/telegram/login/confirm",
                body.as_bytes(),
            )),
        ] {
            let refused = app().oneshot(request(signature, body)).await.unwrap();
            assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[test]
    fn refuses_a_value_that_is_not_a_bot_token() {
        assert!(Authority::from_bot_token("").is_none());
        assert!(Authority::from_bot_token("not-a-token").is_none());
        assert!(Authority::from_bot_token(" 123456:ok ").is_some());
    }
}
