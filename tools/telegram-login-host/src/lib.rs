// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! HTTP adapter for the Telegram text client's AIS login: login start and the
//! sender's confirmation behind workload authentication, and the public AIS
//! browser callback between them. Only the workload middleware can insert the
//! authenticated, login-scoped bot principal into request extensions.
//!
//! One Core and one product database serve the Mini App, the browser and the
//! chat: a confirmed chat session lives beside their sessions and follows the
//! same Single Active Client rules.

mod authority;
pub use authority::{Authority, ISSUER};

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, RETRY_AFTER},
    },
    response::{IntoResponse as _, Response},
    routing::{get, post},
};
use nilxone_api::bot_login::{
    CONFIRM_PATH, LoginConfirmRefusal, LoginConfirmRequest, LoginConfirmResponse,
    LoginStartRefusal, LoginStartRequest, START_PATH,
};
use nilxone_identity::{
    aiaiaiai_sign_in::SignInClient,
    public_info::{LegacyBondMigrationRepository, PublicInfoRepository, TelegramLoginError},
    rate_limit::AttemptLimiter,
};
use serde::{Deserialize, Serialize};

/// The AIS redirect for chat sign-in, registered for client `0x1`.
pub const CALLBACK_PATH: &str = "/api/v1/telegram/login/callback";

/// Trusted middleware supplies the verified bot workload subject after checking
/// its login-start permission. Never deserialize this value from the wire.
#[derive(Clone)]
struct AuthorizedLoginBot(String);

/// What the host needs to finish a chat sign-in after login start.
pub struct ChatSignIn {
    /// Gives a first-time owner their Bond, as browser sign-in does.
    pub provisioning: Option<LegacyBondMigrationRepository>,
    /// The chat session lifetime; the same as a browser product session.
    pub session_ttl_seconds: u64,
}

#[derive(Clone)]
struct LoginHost {
    repository: PublicInfoRepository,
    client: SignInClient,
    /// The bot's token: it also verifies the Telegram initData of the AIS page.
    authority: Authority,
    chat: Option<Arc<ChatSignIn>>,
    /// Per-process only; each replica has its own budget.
    attempts: AttemptLimiter,
}

/// Login start, and with `chat` the callback and confirmation too. Without
/// `chat` the host only starts logins, as before.
pub fn authenticated_router(
    repository: PublicInfoRepository,
    client: SignInClient,
    authority: Authority,
    chat: Option<ChatSignIn>,
) -> Router {
    let host = LoginHost {
        repository,
        client,
        authority: authority.clone(),
        chat: chat.map(Arc::new),
        attempts: AttemptLimiter::default(),
    };
    let callback = host.chat.is_some().then(|| public_router(host.clone()));
    let authenticated = router(host).route_layer(axum::middleware::from_fn_with_state(
        authority,
        authority::authenticate,
    ));
    match callback {
        Some(callback) => authenticated.merge(callback),
        None => authenticated,
    }
}

fn router(host: LoginHost) -> Router {
    let confirm = host.chat.is_some();
    let router = Router::new().route(START_PATH, post(start));
    let router = if confirm {
        router.route(CONFIRM_PATH, post(confirm_login))
    } else {
        router
    };
    router.with_state(host)
}

/// The browser reaches this route from AIS, never with workload credentials.
fn public_router(host: LoginHost) -> Router {
    Router::new()
        .route(CALLBACK_PATH, get(callback))
        .with_state(host)
}

fn now_unix_seconds() -> Option<u64> {
    Some(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs())
}

fn no_store_json(status: StatusCode, body: impl Serialize) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn no_store_error(status: StatusCode, code: &str, message: &str) -> Response {
    no_store_json(
        status,
        serde_json::json!({"error": {"code": code, "message": message}}),
    )
}

fn refusal(reason: LoginStartRefusal) -> Response {
    no_store_error(
        StatusCode::from_u16(reason.http_status()).expect("Core HTTP status"),
        reason.code(),
        "Telegram login could not be started.",
    )
}

fn confirm_refusal(reason: LoginConfirmRefusal) -> Response {
    no_store_error(
        StatusCode::from_u16(reason.http_status()).expect("Core HTTP status"),
        reason.code(),
        "Telegram login could not be confirmed.",
    )
}

fn rate_limited(mut response: Response, retry: u64) -> Response {
    response
        .headers_mut()
        .insert(RETRY_AFTER, HeaderValue::from(retry));
    response
}

fn map_start_error(error: TelegramLoginError) -> Response {
    match &error {
        TelegramLoginError::InvalidInput => {
            tracing::error!(%error, "Telegram login start rejected invalid input");
            refusal(LoginStartRefusal::InvalidProviderEvidence)
        }
        TelegramLoginError::WrongClient
        | TelegramLoginError::SignIn(_)
        | TelegramLoginError::Storage(_)
        | TelegramLoginError::InvalidTransaction(_) => {
            tracing::error!(%error, "Telegram login start failed");
            refusal(LoginStartRefusal::LoginServiceUnavailable)
        }
    }
}

async fn start(
    State(host): State<LoginHost>,
    principal: Option<Extension<AuthorizedLoginBot>>,
    Json(request): Json<LoginStartRequest>,
) -> Response {
    let Some(Extension(AuthorizedLoginBot(subject))) = principal else {
        return refusal(LoginStartRefusal::ServiceAuthenticationRequired);
    };
    if let Err(reason) = request.validate() {
        return refusal(reason);
    }
    if i64::try_from(request.provider_actor_id).ok() != Some(request.chat_id) {
        return refusal(LoginStartRefusal::InvalidProviderEvidence);
    }
    let Some(now) = now_unix_seconds() else {
        return refusal(LoginStartRefusal::LoginServiceUnavailable);
    };
    // Bucket per Telegram actor so one chatter cannot exhaust the shared bot.
    let limit_key = format!("{subject}:{}", request.provider_actor_id);
    if let Err(retry) = host.attempts.consume(limit_key, now, 30, 60) {
        return rate_limited(refusal(LoginStartRefusal::LoginRateLimited), retry);
    }
    match host
        .repository
        .start_telegram_login(&host.client, &subject, &request, now)
        .await
    {
        Ok(Some(started)) => no_store_json(StatusCode::OK, started),
        // Consumed or expired updates; Core has no distinct login_expired yet.
        Ok(None) => refusal(LoginStartRefusal::InvalidProviderEvidence),
        Err(error) => map_start_error(error),
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    state: Option<String>,
    code: Option<String>,
    /// An OAuth error response from AIS (RFC 6749 §4.1.2.1).
    error: Option<String>,
}

/// An issuer error code fit for a log line: the registered OAuth grammar,
/// so a crafted callback cannot write arbitrary text into the logs.
fn loggable_error_code(code: &str) -> &str {
    if (1..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        code
    } else {
        "unrecognized"
    }
}

fn refused(reason: &'static str) -> Response {
    tracing::warn!(reason, "Telegram login callback refused");
    callback_page(false)
}

/// Closes the Telegram Mini App the AIS page ran in, so the player is back in
/// the chat where the bot already asks to confirm. It speaks the Mini App
/// bridge directly (iOS and Android webviews, Desktop, Web) instead of loading
/// Telegram's SDK, which does not survive the cross-origin hops through AIS.
/// Outside Telegram it does nothing; the button retries on a tap.
const CLOSE_SCRIPT: &str = "(function(){var e='web_app_close';function c(){var p=window.TelegramWebviewProxy;if(p&&p.postEvent){p.postEvent(e,'{}');return}var x=window.external;if(x&&'notify' in x){x.notify(JSON.stringify({eventType:e,eventData:{}}));return}if(window.parent!==window){window.parent.postMessage(JSON.stringify({eventType:e,eventData:{}}),'https://web.telegram.org')}}var b=document.getElementById('back');if(b){b.addEventListener('click',c)}c();setTimeout(c,400)})();";
/// The CSP hash of [`CLOSE_SCRIPT`]: no other inline script may run.
const CLOSE_SCRIPT_HASH: &str = "'sha256-QA0JCofgdLcnXnlymOHSLt/n7YapEKaOR2Mrmw7Tg9s='";
/// Follows the Telegram light or dark theme through the system setting.
const PAGE_STYLE: &str = ":root{color-scheme:light dark;--bg:#fff;--fg:#111;--muted:#6b6b70;--accent:#2f7cf6}@media (prefers-color-scheme:dark){:root{--bg:#000;--fg:#f2f2f7;--muted:#9a9aa1}}html,body{height:100%;margin:0}body{background:var(--bg);color:var(--fg);font:16px/1.45 -apple-system,system-ui,sans-serif;display:flex;align-items:center;justify-content:center;text-align:center}main{padding:24px 16px;max-width:22rem}h1{font-size:20px;font-weight:600;margin:0 0 8px}p{margin:0 0 20px;color:var(--muted)}button{appearance:none;border:0;border-radius:12px;background:var(--accent);color:#fff;font:inherit;font-weight:600;padding:12px 20px;width:100%}";
/// The CSP hash of [`PAGE_STYLE`].
const PAGE_STYLE_HASH: &str = "'sha256-0ejL9XzUZbfdLZTkgu9PqmwEwkwQqU2YbVO0t0pV5f4='";

/// A minimal page: the browser step says nothing about the account, and
/// signing in happens only when the sender confirms in Telegram.
fn callback_page(verified: bool) -> Response {
    let (status, heading, body, action) = if verified {
        (
            StatusCode::OK,
            "Готово",
            "Повертаю в чат — підтверди вхід там.",
            format!(
                "<button id=\"back\" type=\"button\">Повернутися в чат</button>\
                 <script>{CLOSE_SCRIPT}</script>"
            ),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            "Вхід не вдався · Sign-in failed",
            "Надішли /login у чаті з ботом ще раз. Send /login in the bot chat again.",
            String::new(),
        )
    };
    let html = format!(
        "<!doctype html><html lang=\"uk\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\"><title>0x1</title>\
         <style>{PAGE_STYLE}</style></head>\
         <body><main><h1>{heading}</h1><p>{body}</p>{action}</main></body></html>"
    );
    let mut response = (status, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    // Telegram Web shows a Mini App in a frame of web.telegram.org; no other
    // site may frame the page.
    let csp = format!(
        "default-src 'none'; script-src {CLOSE_SCRIPT_HASH}; style-src {PAGE_STYLE_HASH}; \
         frame-ancestors https://web.telegram.org; base-uri 'none'; form-action 'none'"
    );
    if let Ok(value) = HeaderValue::from_str(&csp) {
        headers.insert("content-security-policy", value);
    }
    response
}

async fn callback(State(host): State<LoginHost>, Query(query): Query<CallbackQuery>) -> Response {
    if host.chat.is_none() {
        return refused("chat_sign_in_disabled");
    }
    // AIS refused the authorization request itself; the person never saw
    // a sign-in page. Only the error code is logged, never state or code.
    if let Some(error) = query.error.as_deref() {
        tracing::warn!(
            reason = "issuer_error",
            error = loggable_error_code(error),
            "Telegram login callback refused"
        );
        return callback_page(false);
    }
    let (Some(state), Some(code)) = (query.state, query.code) else {
        return refused("missing_state_or_code");
    };
    // Core checks the state first; this only keeps a malformed request from
    // costing a code exchange.
    if state.len() != 32 || code.is_empty() || code.len() > 512 {
        return refused("malformed_callback");
    }
    let Some(now) = now_unix_seconds() else {
        return refused("clock_unavailable");
    };
    if host
        .attempts
        .consume("telegram-callback".to_owned(), now, 60, 60)
        .is_err()
    {
        return refused("rate_limited");
    }
    match host
        .repository
        .verify_telegram_login_callback(
            &host.client,
            &state,
            &code,
            Some(host.authority.bot_token()),
        )
        .await
    {
        Ok(Some(_)) => {
            tracing::info!("Telegram login callback verified; awaiting confirmation");
            callback_page(true)
        }
        // Core logged the reason.
        Ok(None) => callback_page(false),
        Err(error) => {
            tracing::error!(%error, "Telegram login callback failed");
            callback_page(false)
        }
    }
}

async fn confirm_login(
    State(host): State<LoginHost>,
    principal: Option<Extension<AuthorizedLoginBot>>,
    Json(request): Json<LoginConfirmRequest>,
) -> Response {
    let Some(Extension(AuthorizedLoginBot(subject))) = principal else {
        return confirm_refusal(LoginConfirmRefusal::ServiceAuthenticationRequired);
    };
    let Some(chat) = host.chat.as_ref() else {
        return confirm_refusal(LoginConfirmRefusal::LoginServiceUnavailable);
    };
    if request.validate().is_err()
        || i64::try_from(request.provider_actor_id).ok() != Some(request.chat_id)
    {
        return confirm_refusal(LoginConfirmRefusal::InvalidProviderEvidence);
    }
    let Some(now) = now_unix_seconds() else {
        return confirm_refusal(LoginConfirmRefusal::LoginServiceUnavailable);
    };
    let limit_key = format!("confirm:{subject}:{}", request.provider_actor_id);
    if let Err(retry) = host.attempts.consume(limit_key, now, 30, 60) {
        return rate_limited(
            confirm_refusal(LoginConfirmRefusal::LoginServiceUnavailable),
            retry,
        );
    }
    let signed_in = match host
        .repository
        .confirm_telegram_login(
            &subject,
            request.provider_actor_id,
            request.chat_id,
            request.message_id,
            now,
            now.saturating_add(chat.session_ttl_seconds),
        )
        .await
    {
        Ok(Some(signed_in)) => signed_in,
        Ok(None) => return confirm_refusal(LoginConfirmRefusal::ConfirmationNotFound),
        Err(error) => {
            tracing::error!(%error, "Telegram login confirmation failed");
            return confirm_refusal(LoginConfirmRefusal::LoginServiceUnavailable);
        }
    };
    let profile = signed_in.owner.public_profile();
    // A first sign-in gets its Bond, as in the browser. Without one the chat
    // session would be a profile nobody can play: sign it out again.
    if let Some(provisioning) = chat.provisioning.as_ref()
        && let Err(failure) = provisioning
            .provision_new_bond(&profile.profile_id, now)
            .await
    {
        tracing::error!(
            ?failure,
            "Telegram chat sign-in could not provision its Bond"
        );
        if let Err(error) = host
            .repository
            .close_telegram_chat_session(&subject, request.provider_actor_id, now)
            .await
        {
            tracing::error!(%error, "closing an unprovisioned Telegram chat session failed");
        }
        return confirm_refusal(LoginConfirmRefusal::LoginServiceUnavailable);
    }
    no_store_json(
        StatusCode::OK,
        LoginConfirmResponse {
            profile_id: profile.profile_id.clone(),
            pub_dress: profile.pub_dress.clone(),
            active: signed_in.active,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, header},
    };
    use nilxone_identity::aiaiaiai_sign_in::SignInClientConfig;
    use tower::ServiceExt;

    async fn host(client: SignInClient, chat: Option<ChatSignIn>) -> LoginHost {
        LoginHost {
            repository: PublicInfoRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
            client,
            authority: Authority::from_bot_token("123456:test-bot-token").unwrap(),
            chat: chat.map(Arc::new),
            attempts: AttemptLimiter::default(),
        }
    }

    fn chat() -> ChatSignIn {
        ChatSignIn {
            provisioning: None,
            session_ttl_seconds: 3_600,
        }
    }

    fn offline_client() -> SignInClient {
        SignInClient::new(
            SignInClientConfig::public(
                "https://auth.aiaiaiai.org",
                Some("http://127.0.0.1:0"),
                "0x1",
                "https://nilx.one/api/v1/telegram/login/callback",
            )
            .unwrap(),
        )
    }

    fn confirm_request(trusted: bool, actor: u64, chat: i64, message_id: i32) -> Request<Body> {
        let mut request = Request::post(CONFIRM_PATH)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "version": 1, "provider": "telegram", "provider_actor_id": actor,
                    "chat_id": chat, "message_id": message_id
                })
                .to_string(),
            ))
            .unwrap();
        if trusted {
            request
                .extensions_mut()
                .insert(AuthorizedLoginBot("test-bot".into()));
        }
        request
    }

    async fn error_code(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        body["error"]["code"].as_str().unwrap().to_owned()
    }

    #[tokio::test]
    async fn confirm_is_mounted_only_with_chat_sign_in() {
        let without = router(host(offline_client(), None).await);
        assert_eq!(
            without
                .oneshot(confirm_request(true, 42, 42, 7))
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let app = router(host(offline_client(), Some(chat())).await);
        let unauthenticated = app
            .clone()
            .oneshot(confirm_request(false, 42, 42, 7))
            .await
            .unwrap();
        assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
        let other_chat = app
            .clone()
            .oneshot(confirm_request(true, 42, 41, 7))
            .await
            .unwrap();
        assert_eq!(other_chat.status(), StatusCode::UNPROCESSABLE_ENTITY);
        // No browser step finished for this message: never a session.
        let unconfirmed = app.oneshot(confirm_request(true, 42, 42, 7)).await.unwrap();
        assert_eq!(unconfirmed.status(), StatusCode::NOT_FOUND);
        assert_eq!(unconfirmed.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            error_code(unconfirmed).await,
            "login_confirmation_not_found"
        );
    }

    #[tokio::test]
    async fn callback_is_public_and_refuses_malformed_or_unknown_state_without_detail() {
        let app = public_router(host(offline_client(), Some(chat())).await);
        for uri in [
            CALLBACK_PATH.to_owned(),
            format!("{CALLBACK_PATH}?code=c"),
            format!("{CALLBACK_PATH}?state=short&code=c"),
            // AIS refused the request before any sign-in page.
            format!(
                "{CALLBACK_PATH}?error=invalid_scope&state={}",
                "s".repeat(32)
            ),
            format!("{CALLBACK_PATH}?error=access_denied"),
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert!(
                response.headers()[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with("text/html")
            );
            assert!(!response.headers().contains_key(header::SET_COOKIE));
        }
    }

    #[test]
    fn only_oauth_shaped_error_codes_reach_the_log() {
        assert_eq!(loggable_error_code("invalid_scope"), "invalid_scope");
        assert_eq!(loggable_error_code("access_denied"), "access_denied");
        for crafted in [
            "",
            "Invalid_Scope",
            "invalid scope",
            "x\nforged=1",
            &"a".repeat(65),
        ] {
            assert_eq!(loggable_error_code(crafted), "unrecognized", "{crafted:?}");
        }
    }

    #[tokio::test]
    async fn private_update_requires_a_verified_caller_and_recovers_the_same_link() {
        let callback = "https://nilx.one/api/v1/telegram/login/callback";
        let client =
            SignInClientConfig::public("https://auth.aiaiaiai.org", None, "0x1", callback).unwrap();
        let app = router(host(SignInClient::new(client), None).await);
        let send = |trusted, chat| {
            let mut request = Request::post(START_PATH)
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-workload-subject", "test-bot")
                .body(Body::from(
                    serde_json::json!({
                        "version": 1, "provider": "telegram", "provider_actor_id": 42,
                        "chat_id": chat, "message_id": 7
                    })
                    .to_string(),
                ))
                .unwrap();
            if trusted {
                request
                    .extensions_mut()
                    .insert(AuthorizedLoginBot("test-bot".into()));
            }
            app.clone().oneshot(request)
        };
        assert_eq!(
            send(false, 42).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            send(true, 41).await.unwrap().status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let mut links = Vec::new();
        for _ in 0..2 {
            let response = send(true, 42).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
            let started: nilxone_api::bot_login::LoginStartResponse =
                serde_json::from_slice(&bytes).unwrap();
            assert!(started.has_valid_ttl());
            let url = url::Url::parse(&started.authorization_url).unwrap();
            assert_eq!(url.path(), "/0x1");
            assert!(
                url.query_pairs()
                    .any(|(k, v)| k == "redirect_uri" && v == callback)
            );
            links.push(url);
        }
        assert_eq!(links[0], links[1]);
    }

    #[tokio::test]
    async fn rate_limit_is_isolated_per_telegram_actor() {
        let callback = "https://nilx.one/api/v1/telegram/login/callback";
        let client =
            SignInClientConfig::public("https://auth.aiaiaiai.org", None, "0x1", callback).unwrap();
        let app = router(host(SignInClient::new(client), None).await);
        let send = |actor: u64, message_id: i32| {
            let mut request = Request::post(START_PATH)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "version": 1, "provider": "telegram",
                        "provider_actor_id": actor, "chat_id": actor as i64,
                        "message_id": message_id
                    })
                    .to_string(),
                ))
                .unwrap();
            request
                .extensions_mut()
                .insert(AuthorizedLoginBot("test-bot".into()));
            app.clone().oneshot(request)
        };
        for message_id in 1..=30 {
            assert_eq!(
                send(42, message_id).await.unwrap().status(),
                StatusCode::OK,
                "actor 42 message {message_id}"
            );
        }
        let limited = send(42, 31).await.unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(limited.headers().contains_key(RETRY_AFTER));
        assert_eq!(
            send(43, 1).await.unwrap().status(),
            StatusCode::OK,
            "a different actor keeps its own budget"
        );
    }

    #[test]
    fn the_close_script_is_the_only_inline_script_the_csp_admits() {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use sha2::{Digest as _, Sha256};
        let hash = |text: &str| format!("'sha256-{}'", STANDARD.encode(Sha256::digest(text)));
        assert_eq!(CLOSE_SCRIPT_HASH, hash(CLOSE_SCRIPT));
        assert_eq!(PAGE_STYLE_HASH, hash(PAGE_STYLE));
    }

    #[tokio::test]
    async fn a_verified_sign_in_closes_the_telegram_page() {
        let page = callback_page(true);
        assert_eq!(page.status(), StatusCode::OK);
        let csp = page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(
            csp.contains(&format!("script-src {CLOSE_SCRIPT_HASH};")),
            "{csp}"
        );
        assert!(!csp.contains("https://telegram.org"), "{csp}");
        let body = to_bytes(page.into_body(), 4096).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains(CLOSE_SCRIPT));
        assert!(body.contains("web_app_close") && !body.contains("telegram-web-app.js"));
        let failed = callback_page(false);
        let body = to_bytes(failed.into_body(), 4096).await.unwrap();
        assert!(
            !String::from_utf8(body.to_vec())
                .unwrap()
                .contains("<script")
        );
    }
}
