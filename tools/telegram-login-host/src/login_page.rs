// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The chat login page. The bot opens it as a Telegram Mini App button for
//! one `/login` message. Inside Telegram the page posts the signed `initData`;
//! the host checks that it belongs to the sender, binds the login to this
//! browser with an HttpOnly cookie and sends the browser on to AIS. After the
//! callback the page closes and the player continues in the chat. It is not
//! the game's Mini App.

use axum::{
    Json,
    extract::State,
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, SET_COOKIE},
    },
    response::{IntoResponse as _, Response},
};
use serde::Deserialize;

use crate::{LoginHost, init_data, no_store_error, no_store_json, now_unix_seconds};

/// Holds the browser token for one login, for ten minutes at most.
pub(crate) const COOKIE_NAME: &str = "__Host-0x1_telegram_login";
const COOKIE_MAX_AGE_SECONDS: u64 = 600;
pub(crate) const APP_SCRIPT_PATH: &str = "/api/v1/telegram/login/app.js";
pub(crate) const CLOSE_SCRIPT_PATH: &str = "/api/v1/telegram/login/close.js";
const TELEGRAM_SCRIPT: &str = "https://telegram.org/js/telegram-web-app.js";
/// Telegram Web shows Mini Apps in a frame; the mobile apps use a WebView.
pub(crate) const PAGE_CSP: &str = "default-src 'none'; script-src 'self' https://telegram.org; \
     connect-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; \
     frame-ancestors https://web.telegram.org";

const APP_SCRIPT: &str = r#"(function () {
  var status = document.getElementById("status");
  function show(text) { status.textContent = text; }
  var app = window.Telegram && window.Telegram.WebApp;
  var message = Number(new URLSearchParams(window.location.search).get("m"));
  if (!app || !app.initData || !(message > 0)) {
    show("Відкрий вхід кнопкою «Увійти» в чаті з ботом. · Open sign-in from the bot chat.");
    return;
  }
  app.ready();
  app.expand();
  fetch(window.location.pathname, {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ init_data: app.initData, message_id: message })
  }).then(function (response) {
    if (!response.ok) { throw new Error(String(response.status)); }
    return response.json();
  }).then(function (body) {
    window.location.replace(body.authorization_url);
  }).catch(function () {
    show("Вхід застарів. Надішли /login ще раз. · This sign-in expired. Send /login again.");
  });
})();
"#;

const CLOSE_SCRIPT: &str = r#"(function () {
  var app = window.Telegram && window.Telegram.WebApp;
  if (app) { window.setTimeout(function () { app.close(); }, 1500); }
})();
"#;

pub(crate) fn html(
    status: StatusCode,
    heading: &str,
    body: &str,
    script: Option<&str>,
) -> Response {
    let scripts = script.map_or_else(String::new, |script| {
        format!("<script src=\"{TELEGRAM_SCRIPT}\"></script><script src=\"{script}\"></script>")
    });
    let html = format!(
        "<!doctype html><html lang=\"uk\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"referrer\" content=\"no-referrer\"><title>0x1</title>\
         <style>body{{font-family:system-ui,sans-serif;margin:2rem;line-height:1.4}}</style>\
         </head><body><main><h1>{heading}</h1><p id=\"status\">{body}</p></main>{scripts}\
         </body></html>"
    );
    let mut response = (status, html).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    secure_headers(headers);
    response
}

fn secure_headers(headers: &mut HeaderMap) {
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(PAGE_CSP),
    );
}

fn script(body: &'static str) -> Response {
    let mut response = (StatusCode::OK, body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/javascript; charset=utf-8"),
    );
    secure_headers(headers);
    response
}

pub(crate) async fn page() -> Response {
    html(
        StatusCode::OK,
        "Вхід у 0x1 · Sign in to 0x1",
        "Перевіряю Telegram… · Checking Telegram…",
        Some(APP_SCRIPT_PATH),
    )
}

pub(crate) async fn app_script() -> Response {
    script(APP_SCRIPT)
}

pub(crate) async fn close_script() -> Response {
    script(CLOSE_SCRIPT)
}

/// The cookie that names this browser's login, if the request carries one.
pub(crate) fn browser_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_owned())
        .filter(|value| (1..=128).contains(&value.len()))
}

pub(crate) fn clear_cookie(response: &mut Response) {
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_static(
            "__Host-0x1_telegram_login=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0",
        ),
    );
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindBrowser {
    init_data: String,
    message_id: i32,
}

fn bind_refused(status: StatusCode, code: &'static str, reason: &'static str) -> Response {
    tracing::warn!(reason, "Telegram login page refused");
    no_store_error(status, code, "Telegram sign-in could not continue.")
}

pub(crate) async fn bind(
    State(host): State<LoginHost>,
    Json(request): Json<BindBrowser>,
) -> Response {
    if host.chat.is_none() {
        return bind_refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "login_service_unavailable",
            "chat_sign_in_disabled",
        );
    }
    let Some(now) = now_unix_seconds() else {
        return bind_refused(
            StatusCode::SERVICE_UNAVAILABLE,
            "login_service_unavailable",
            "clock_unavailable",
        );
    };
    if host
        .attempts
        .consume("telegram-login-page".to_owned(), now, 60, 60)
        .is_err()
    {
        return bind_refused(
            StatusCode::TOO_MANY_REQUESTS,
            "login_rate_limited",
            "rate_limited",
        );
    }
    let sender = match init_data::verify(host.authority.bot_token(), &request.init_data, now) {
        Ok(sender) => sender,
        Err(rejection) => {
            return bind_refused(
                StatusCode::UNAUTHORIZED,
                "invalid_provider_evidence",
                rejection.reason(),
            );
        }
    };
    match host
        .repository
        .bind_telegram_login_browser(
            &host.client,
            host.authority.subject(),
            sender,
            request.message_id,
            now,
        )
        .await
    {
        Ok(Some(bound)) => {
            let mut response = no_store_json(
                StatusCode::OK,
                serde_json::json!({"authorization_url": bound.authorization_url}),
            );
            let cookie = format!(
                "{COOKIE_NAME}={}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={COOKIE_MAX_AGE_SECONDS}",
                bound.browser_token
            );
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(SET_COOKIE, value);
            }
            response
        }
        Ok(None) => bind_refused(
            StatusCode::NOT_FOUND,
            "login_not_found",
            "no_open_login_for_sender",
        ),
        Err(error) => {
            tracing::error!(%error, "Telegram login page failed");
            no_store_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "login_service_unavailable",
                "Telegram sign-in could not continue.",
            )
        }
    }
}
