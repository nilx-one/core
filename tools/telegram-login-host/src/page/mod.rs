// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The page the Telegram Mini App lands on after AIS. On success it plays the
//! sign-in stages in the 0x1 world of auth.aiaiaiai.org/0x1, taps the
//! Telegram haptic engine and closes itself, back to the chat where the bot
//! has already claimed the login. It says nothing about the account.

use std::sync::LazyLock;

use axum::{
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest as _, Sha256};

const SCRIPT: &str = include_str!("page.js");
const STYLE: &str = include_str!("page.css");

/// The visual stages of a sign-in, in order. The last one is the result.
const STAGES: [&str; 4] = [
    "Створюємо акаунт",
    "Привʼязуємо Telegram",
    "Налаштовуємо ідентичність",
    "Bond створено успішно",
];

const SCENE: &str = concat!(
    "<div class=\"world-scene\" aria-hidden=\"true\">",
    "<div class=\"world-sun\"></div>",
    "<div class=\"world-cloud world-cloud--a\"></div><div class=\"world-cloud world-cloud--b\"></div>",
    "<svg class=\"world-skyline\" viewBox=\"0 0 1200 320\" preserveAspectRatio=\"xMidYMax slice\">",
    "<path class=\"world-hills\" d=\"M0 250 C120 205 220 215 330 230 C450 190 560 200 640 225 C760 195 880 180 1000 215 C1080 205 1150 210 1200 220 V320 H0Z\"/>",
    "<path class=\"world-city\" d=\"M90 232 V196 H104 V184 Q112 168 120 184 V196 H134 V232Z ",
    "M150 232 V150 L158 140 V118 L162 104 L166 118 V140 L174 150 V232Z ",
    "M190 232 V204 Q210 176 230 204 V232Z M240 232 V214 H262 V200 Q270 190 278 200 V232Z\"/>",
    "<path class=\"world-bridge\" d=\"M560 258 Q690 196 820 258 M560 258 H960 M600 258 V242 M650 258 V222 M700 258 V212 M750 258 V214 M800 258 V246\"/>",
    "<path class=\"world-city\" d=\"M1040 214 V150 L1046 148 L1050 96 L1056 90 L1060 98 L1062 148 L1068 150 V214Z ",
    "M980 214 V196 H1010 V214Z M1090 214 V192 H1130 V214Z\"/>",
    "</svg>",
    "<div class=\"world-river\"></div>",
    "<div class=\"world-blossom world-blossom--left\"></div><div class=\"world-blossom world-blossom--right\"></div>",
    "<div class=\"world-avaia\"><span></span><span></span></div>",
    "</div>",
);

fn csp_hash(text: &str) -> String {
    format!("'sha256-{}'", STANDARD.encode(Sha256::digest(text)))
}

/// Only this page's own script and style may run; Telegram Web shows a Mini
/// App in a frame of web.telegram.org, and no other site may frame it.
static POLICY: LazyLock<String> = LazyLock::new(|| {
    format!(
        "default-src 'none'; script-src {}; style-src {}; \
         frame-ancestors https://web.telegram.org; base-uri 'none'; form-action 'none'",
        csp_hash(SCRIPT),
        csp_hash(STYLE)
    )
});

fn success_body() -> String {
    let count = STAGES.len();
    let stages: String = STAGES
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let class = if index + 1 == count {
                "stage stage--final"
            } else {
                "stage"
            };
            format!(
                "<li class=\"{class}\"><span class=\"num\">{:02}</span>\
                 <span class=\"label\">{label}</span><span class=\"mark\"></span></li>",
                index + 1
            )
        })
        .collect();
    format!(
        "<ol class=\"stages\" id=\"stages\" aria-label=\"Вхід в 0x1\">{stages}</ol>\
         <div class=\"after\"><p>Повертаю в чат.</p>\
         <button id=\"back\" type=\"button\">Повернутися в чат</button></div>"
    )
}

const FAILURE_BODY: &str = "<div class=\"panel error\"><h1>Вхід не вдався · Sign-in failed</h1>\
    <p>Надішли /login у чаті з ботом ще раз. Send /login in the bot chat again.</p>\
    <button id=\"back\" type=\"button\">Повернутися в чат</button></div>";

pub(crate) fn callback_page(verified: bool) -> Response {
    let (status, body) = if verified {
        (StatusCode::OK, success_body())
    } else {
        (StatusCode::BAD_REQUEST, FAILURE_BODY.to_owned())
    };
    let html = format!(
        "<!doctype html><html lang=\"uk\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\
         <meta name=\"referrer\" content=\"no-referrer\"><title>0x1</title>\
         <style>{STYLE}</style></head>\
         <body>{SCENE}<main>{body}</main><script>{SCRIPT}</script></body></html>"
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
    if let Ok(value) = HeaderValue::from_str(&POLICY) {
        headers.insert("content-security-policy", value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn render(verified: bool) -> (StatusCode, String, String) {
        let page = callback_page(verified);
        let status = page.status();
        let csp = page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        let body = to_bytes(page.into_body(), 64 * 1024).await.unwrap();
        (status, csp, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn a_verified_sign_in_plays_the_stages_and_closes_the_mini_app() {
        let (status, csp, body) = render(true).await;
        assert_eq!(status, StatusCode::OK);
        for stage in STAGES {
            assert!(body.contains(stage), "{stage}");
        }
        assert!(body.contains("web_app_close") && body.contains("web_app_trigger_haptic_feedback"));
        assert!(!body.contains("telegram-web-app.js"));
        assert!(
            csp.contains(&format!("script-src {};", csp_hash(SCRIPT))),
            "{csp}"
        );
        assert!(
            csp.contains(&format!("style-src {};", csp_hash(STYLE))),
            "{csp}"
        );
        assert!(!csp.contains("https://telegram.org"), "{csp}");
    }

    #[tokio::test]
    async fn the_inline_script_and_style_are_exactly_what_the_policy_admits() {
        let (_, _, body) = render(true).await;
        assert!(body.contains(&format!("<script>{SCRIPT}</script>")));
        assert!(body.contains(&format!("<style>{STYLE}</style>")));
        assert_eq!(body.matches("<script").count(), 1);
    }

    #[tokio::test]
    async fn a_refused_sign_in_shows_no_stages() {
        let (status, _, body) = render(false).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!body.contains("id=\"stages\""));
        assert!(body.contains("Sign-in failed"));
    }
}
