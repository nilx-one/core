// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Bot workload authentication with the Telegram Bot API token the bot and
//! this host already hold. No other credential exists for the bot: whoever
//! holds the token already controls the chat, so signing with it grants no
//! new authority.

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use nilxone_api::bot_login::{
    LoginStartRefusal, SIGNATURE_HEADER, verify_request, workload_subject,
};

use crate::{AuthorizedLoginBot, now_unix_seconds, refusal};

pub const ISSUER: &str = "https://auth.aiaiaiai.org";
/// Login requests are small JSON documents.
const MAX_BODY_BYTES: usize = 4_096;

#[derive(Clone)]
pub struct Authority {
    bot_token: String,
    subject: String,
}

impl Authority {
    /// The `@nilxone_bot` token. Refuses a value that is not a Bot API token.
    pub fn from_bot_token(bot_token: &str) -> Option<Self> {
        let bot_token = bot_token.trim();
        Some(Self {
            subject: workload_subject(bot_token)?,
            bot_token: bot_token.to_owned(),
        })
    }

    /// The Bot API token; also the key Telegram signs Mini App initData with.
    pub(crate) fn bot_token(&self) -> &str {
        &self.bot_token
    }

    /// The workload this bot's login transactions belong to.
    pub(crate) fn subject(&self) -> &str {
        &self.subject
    }

    fn accepts(&self, header: &str, path: &str, body: &[u8], now: u64) -> bool {
        verify_request(&self.bot_token, header, path, body, now)
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
    use nilxone_api::bot_login::{START_PATH, sign_request};
    use tower::ServiceExt as _;

    const TOKEN: &str = "123456:test-bot-token";

    fn app() -> Router {
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
            .route_layer(axum::middleware::from_fn_with_state(
                Authority::from_bot_token(TOKEN).unwrap(),
                authenticate,
            ))
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
