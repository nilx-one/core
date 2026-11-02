// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! 0x1's client of the aiaiaiai Identity issuer (`auth.aiaiaiai.org`).
//!
//! Identity is the authority; 0x1 is one of its clients and reaches it only
//! over OpenID Connect (authorization code + PKCE). This module is that
//! client: it builds the authorization request, exchanges the code, and
//! verifies the `id_token`. The host carries the transaction in a cookie and
//! the redirects; it never reads the issuer's own state, even while both run
//! in one process (`docs/aiaiaiai-sign-in.md`).

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use url::Url;

/// The only signature algorithm accepted. A token naming another one is
/// refused before any key is looked at, so no `none` or HMAC downgrade exists.
const ALGORITHM: &str = "EdDSA";
/// What 0x1 asks for: the person, and their public address for the first
/// link of an existing Bond.
const SCOPE: &str = "openid pub_dress";
/// Clock tolerance between the issuer and this host.
const LEEWAY_SECONDS: u64 = 60;
/// A code is exchanged right after the person signed in. An older sign-in
/// proves less about who is at the browser now.
const MAX_AUTH_AGE_SECONDS: u64 = 10 * 60;
const MIN_CLIENT_SECRET_BYTES: usize = 32;
const MAX_SUBJECT_BYTES: usize = 255;

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum SignInConfigError {
    #[error("the issuer must be an https origin")]
    Issuer,
    #[error("the back channel must be https, or http on a loopback or private host")]
    Backchannel,
    #[error("the client id must be 1-64 lowercase letters, digits or hyphens")]
    ClientId,
    #[error("the client secret must be at least 32 bytes")]
    ClientSecret,
    #[error("the redirect URI must be https")]
    RedirectUri,
}

/// Why a sign-in could not be completed. Only `Unavailable` is worth a retry;
/// every other variant means this attempt is over and the person starts
/// again.
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
pub enum SignInError {
    #[error("the issuer could not be reached")]
    Unavailable,
    #[error("the issuer refused the code")]
    CodeRejected,
    #[error("the id_token is invalid: {0}")]
    InvalidToken(&'static str),
    #[error("no entropy for the authorization request")]
    EntropyUnavailable,
}

#[derive(Clone)]
pub struct SignInClientConfig {
    issuer: Url,
    backchannel: Url,
    client_id: String,
    client_secret: String,
    redirect_uri: Url,
}

impl SignInClientConfig {
    /// `issuer` is the origin people see (`https://auth.aiaiaiai.org`) and the
    /// `iss` every token must carry. `backchannel` is where this host reaches
    /// `/token` and the keys; it defaults to the issuer and may be a private
    /// address inside the deployment.
    pub fn new(
        issuer: &str,
        backchannel: Option<&str>,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
    ) -> Result<Self, SignInConfigError> {
        let issuer = Url::parse(issuer).map_err(|_| SignInConfigError::Issuer)?;
        if issuer.scheme() != "https"
            || issuer.host_str().is_none()
            || issuer.path() != "/"
            || issuer.query().is_some()
            || issuer.fragment().is_some()
        {
            return Err(SignInConfigError::Issuer);
        }
        let backchannel = match backchannel {
            None => issuer.clone(),
            Some(value) => {
                let mut url = Url::parse(value).map_err(|_| SignInConfigError::Backchannel)?;
                let private_http =
                    url.scheme() == "http" && url.host().is_some_and(is_private_host);
                if !(url.scheme() == "https" || private_http) || url.host_str().is_none() {
                    return Err(SignInConfigError::Backchannel);
                }
                if !url.path().ends_with('/') {
                    let path = format!("{}/", url.path());
                    url.set_path(&path);
                }
                url
            }
        };
        if client_id.is_empty()
            || client_id.len() > 64
            || !client_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(SignInConfigError::ClientId);
        }
        if client_secret.len() < MIN_CLIENT_SECRET_BYTES {
            return Err(SignInConfigError::ClientSecret);
        }
        let redirect_uri = Url::parse(redirect_uri).map_err(|_| SignInConfigError::RedirectUri)?;
        if redirect_uri.scheme() != "https" || redirect_uri.fragment().is_some() {
            return Err(SignInConfigError::RedirectUri);
        }
        Ok(Self {
            issuer,
            backchannel,
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            redirect_uri,
        })
    }

    /// The `iss` value: the origin without its trailing slash, as OIDC
    /// discovery spells it.
    pub fn issuer(&self) -> &str {
        self.issuer.as_str().trim_end_matches('/')
    }

    pub fn client_id(&self) -> &str {
        &self.client_id
    }
}

// The secret never reaches a log line through `{:?}`.
impl std::fmt::Debug for SignInClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SignInClientConfig")
            .field("issuer", &self.issuer.as_str())
            .field("backchannel", &self.backchannel.as_str())
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .field("redirect_uri", &self.redirect_uri.as_str())
            .finish()
    }
}

fn is_private_host(host: url::Host<&str>) -> bool {
    match host {
        url::Host::Ipv4(address) => address.is_loopback() || address.is_private(),
        url::Host::Ipv6(address) => address.is_loopback(),
        // A container alias on the deployment's own network, such as
        // `nilxone-identity`. A dotted name is public and needs TLS.
        url::Host::Domain(name) => name == "localhost" || !name.contains('.'),
    }
}

/// What the host keeps between sending the person to the issuer and their
/// return, in a signed, short-lived, HttpOnly cookie.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SignInTransaction {
    pub state: String,
    pub nonce: String,
    pub code_verifier: String,
}

/// A verified sign-in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSignIn {
    /// The issuer this sign-in came from.
    pub issuer: String,
    /// Identity's pairwise subject for this client. Stable for 0x1, opaque,
    /// never Identity's private identifier.
    pub subject: String,
    /// The public address at the moment of the sign-in. Mutable, so it is
    /// only used to link a subject the first time, never as a key.
    pub pub_dress: Option<String>,
    pub auth_time: u64,
}

#[derive(Clone, Debug)]
pub struct SignInClient {
    config: SignInClientConfig,
    http: reqwest::Client,
}

impl SignInClient {
    pub fn new(config: SignInClientConfig) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            // A redirect from the token endpoint would carry the client secret
            // somewhere this host never chose.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("the sign-in HTTP client must build");
        Self { config, http }
    }

    pub fn config(&self) -> &SignInClientConfig {
        &self.config
    }

    /// The URL to send the person to, and the transaction to keep until they
    /// return.
    pub fn begin(&self) -> Result<(Url, SignInTransaction), SignInError> {
        let transaction = SignInTransaction {
            state: random_token(24)?,
            nonce: random_token(24)?,
            code_verifier: random_token(32)?,
        };
        Ok((self.authorization_url(&transaction), transaction))
    }

    fn authorization_url(&self, transaction: &SignInTransaction) -> Url {
        let mut url = self
            .config
            .issuer
            .join("authorize")
            .expect("authorize is a relative path on the issuer origin");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", self.config.redirect_uri.as_str())
            .append_pair("scope", SCOPE)
            .append_pair("state", &transaction.state)
            .append_pair("nonce", &transaction.nonce)
            .append_pair(
                "code_challenge",
                &code_challenge(&transaction.code_verifier),
            )
            .append_pair("code_challenge_method", "S256");
        url
    }

    /// Exchanges the code the person came back with and verifies the token.
    /// The host has already checked `state` against the transaction.
    pub async fn complete(
        &self,
        code: &str,
        transaction: &SignInTransaction,
        now: u64,
    ) -> Result<VerifiedSignIn, SignInError> {
        if code.is_empty() || code.len() > 512 {
            return Err(SignInError::CodeRejected);
        }
        let token_url = self
            .config
            .backchannel
            .join("token")
            .map_err(|_| SignInError::Unavailable)?;
        let response = self
            .http
            .post(token_url)
            .basic_auth(
                form_encode(&self.config.client_id),
                Some(form_encode(&self.config.client_secret)),
            )
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", self.config.redirect_uri.as_str()),
                ("code_verifier", transaction.code_verifier.as_str()),
            ])
            .send()
            .await
            .map_err(|_| SignInError::Unavailable)?;
        let status = response.status();
        if status.is_server_error() {
            return Err(SignInError::Unavailable);
        }
        if !status.is_success() {
            return Err(SignInError::CodeRejected);
        }
        let tokens = response
            .json::<TokenResponse>()
            .await
            .map_err(|_| SignInError::InvalidToken("token_response"))?;
        let keys = self.signing_keys().await?;
        verify_id_token(
            &tokens.id_token,
            &keys,
            &Expectation {
                issuer: self.config.issuer(),
                client_id: &self.config.client_id,
                nonce: &transaction.nonce,
            },
            now,
        )
    }

    async fn signing_keys(&self) -> Result<JsonWebKeySet, SignInError> {
        let url = self
            .config
            .backchannel
            .join(".well-known/jwks.json")
            .map_err(|_| SignInError::Unavailable)?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| SignInError::Unavailable)?;
        if !response.status().is_success() {
            return Err(SignInError::Unavailable);
        }
        response
            .json::<JsonWebKeySet>()
            .await
            .map_err(|_| SignInError::Unavailable)
    }
}

/// RFC 6749 §2.3.1: the client id and secret are form-encoded before Basic.
fn form_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn random_token(bytes: usize) -> Result<String, SignInError> {
    let mut value = vec![0_u8; bytes];
    getrandom::fill(&mut value).map_err(|_| SignInError::EntropyUnavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(value))
}

/// The PKCE S256 challenge for a verifier.
pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JsonWebKeySet {
    pub keys: Vec<JsonWebKey>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct JsonWebKey {
    pub kty: String,
    #[serde(default)]
    pub crv: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default, rename = "use")]
    pub key_use: Option<String>,
}

pub struct Expectation<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub nonce: &'a str,
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    iat: u64,
    auth_time: Option<u64>,
    nonce: Option<String>,
    azp: Option<String>,
    pub_dress: Option<String>,
}

/// Verifies a compact EdDSA `id_token` against the issuer's keys and the
/// transaction it answers.
pub fn verify_id_token(
    token: &str,
    keys: &JsonWebKeySet,
    expected: &Expectation<'_>,
    now: u64,
) -> Result<VerifiedSignIn, SignInError> {
    let mut parts = token.split('.');
    let (Some(header_part), Some(claims_part), Some(signature_part), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(SignInError::InvalidToken("format"));
    };
    let header: Header = decode_json(header_part, "header")?;
    if header.alg != ALGORITHM {
        return Err(SignInError::InvalidToken("algorithm"));
    }
    let key = signing_key(keys, header.kid.as_deref())?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature_part)
        .map_err(|_| SignInError::InvalidToken("signature"))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| SignInError::InvalidToken("signature"))?;
    let signed = &token[..header_part.len() + 1 + claims_part.len()];
    key.verify_strict(signed.as_bytes(), &signature)
        .map_err(|_| SignInError::InvalidToken("signature"))?;

    let claims: Claims = decode_json(claims_part, "claims")?;
    if claims.iss != expected.issuer {
        return Err(SignInError::InvalidToken("issuer"));
    }
    let audience_ok = match &claims.aud {
        Audience::One(value) => value == expected.client_id,
        Audience::Many(values) => {
            values.iter().any(|value| value == expected.client_id)
                && (values.len() == 1 || claims.azp.as_deref() == Some(expected.client_id))
        }
    };
    if !audience_ok {
        return Err(SignInError::InvalidToken("audience"));
    }
    if claims.exp.saturating_add(LEEWAY_SECONDS) <= now {
        return Err(SignInError::InvalidToken("expired"));
    }
    if claims.iat > now.saturating_add(LEEWAY_SECONDS) {
        return Err(SignInError::InvalidToken("issued_in_future"));
    }
    let nonce_ok = claims
        .nonce
        .as_deref()
        .is_some_and(|nonce| nonce.as_bytes().ct_eq(expected.nonce.as_bytes()).into());
    if !nonce_ok {
        return Err(SignInError::InvalidToken("nonce"));
    }
    let Some(auth_time) = claims.auth_time else {
        return Err(SignInError::InvalidToken("auth_time"));
    };
    if auth_time > now.saturating_add(LEEWAY_SECONDS)
        || auth_time.saturating_add(MAX_AUTH_AGE_SECONDS) < now
    {
        return Err(SignInError::InvalidToken("auth_time"));
    }
    if claims.sub.is_empty()
        || claims.sub.len() > MAX_SUBJECT_BYTES
        || claims
            .sub
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(SignInError::InvalidToken("subject"));
    }
    Ok(VerifiedSignIn {
        issuer: claims.iss,
        subject: claims.sub,
        pub_dress: claims.pub_dress,
        auth_time,
    })
}

fn decode_json<T: for<'de> Deserialize<'de>>(
    part: &str,
    what: &'static str,
) -> Result<T, SignInError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| SignInError::InvalidToken(what))?;
    serde_json::from_slice(&bytes).map_err(|_| SignInError::InvalidToken(what))
}

fn signing_key(keys: &JsonWebKeySet, kid: Option<&str>) -> Result<VerifyingKey, SignInError> {
    let mut candidates = keys.keys.iter().filter(|key| {
        key.kty == "OKP"
            && key.crv.as_deref() == Some("Ed25519")
            && key.key_use.as_deref().is_none_or(|value| value == "sig")
            && (kid.is_none() || key.kid.as_deref() == kid)
    });
    let key = candidates
        .next()
        .ok_or(SignInError::InvalidToken("unknown_key"))?;
    // Without a `kid`, only an unambiguous key set can be trusted to name the
    // signer.
    if kid.is_none() && candidates.next().is_some() {
        return Err(SignInError::InvalidToken("unknown_key"));
    }
    let bytes = key
        .x
        .as_deref()
        .and_then(|x| URL_SAFE_NO_PAD.decode(x).ok())
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or(SignInError::InvalidToken("unknown_key"))?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| SignInError::InvalidToken("unknown_key"))
}

// ---------------------------------------------------------------------------
// The whole browser flow, so a host only forwards (`docs/aiaiaiai-sign-in.md`)
// ---------------------------------------------------------------------------

/// The cookie that carries a sign-in between leaving for the issuer and
/// coming back. `__Host-` pins it to this origin, path `/`, Secure.
pub const TRANSACTION_COOKIE: &str = "__Host-0x1_aiaiaiai";
const TRANSACTION_TTL_SECONDS: u64 = 15 * 60;
const TRANSACTION_DOMAIN: &str = "aiaiaiai-sign-in-transaction";
const MAX_RETURN_PATH_BYTES: usize = 512;

#[derive(Deserialize, Serialize)]
struct StoredTransaction {
    sign_in: SignInTransaction,
    return_path: String,
    expires_at: u64,
}

/// Where the person goes next, and the cookie the host sets on the way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Started {
    pub location: Url,
    /// A complete `Set-Cookie` value.
    pub set_cookie: String,
}

/// The issuer's answer, exactly as the callback URL carried it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Callback<'a> {
    pub code: Option<&'a str>,
    pub state: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// Why a callback did not sign anyone in. `as_str` is the code the host shows
/// the person (`/?auth_error=…`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignInFailure {
    /// No transaction, or an expired one: the person starts again.
    Expired,
    /// A transaction or `state` that does not match: possibly forged.
    Invalid,
    /// The person or the issuer declined.
    Cancelled,
    /// The issuer or storage could not answer; worth a retry.
    Unavailable,
    /// A valid sign-in for an account no Bond is linked to.
    AccountUnknown,
    /// The named Bond is linked to another Identity account.
    AccountConflict,
}

impl SignInFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Expired => "aiaiaiai_sign_in_expired",
            Self::Invalid => "aiaiaiai_sign_in_invalid",
            Self::Cancelled => "aiaiaiai_sign_in_cancelled",
            Self::Unavailable => "aiaiaiai_sign_in_unavailable",
            Self::AccountUnknown => "aiaiaiai_account_unknown",
            Self::AccountConflict => "aiaiaiai_account_conflict",
        }
    }
}

/// The Bond a callback signed in, and the same-origin path to return to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedIn {
    pub pub_dress: crate::PubDress,
    pub return_path: String,
}

/// A finished callback. The transaction cookie is cleared either way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finished {
    pub outcome: Result<SignedIn, SignInFailure>,
    /// A complete `Set-Cookie` value that removes the transaction.
    pub clear_cookie: String,
}

/// 0x1's sign-in through aiaiaiai Identity, end to end. The host passes the
/// request's query and `Cookie` header in, and applies the redirect and the
/// `Set-Cookie` values it gets back. It decides nothing itself.
#[derive(Clone, Debug)]
pub struct SignInFlow {
    client: SignInClient,
    signer: crate::SecretDigester,
}

impl SignInFlow {
    /// `signer` keys the transaction cookie; the host's native authentication
    /// secret, under this module's own domain.
    pub fn new(client: SignInClient, signer: crate::SecretDigester) -> Self {
        Self { client, signer }
    }

    pub fn issuer(&self) -> &str {
        self.client.config().issuer()
    }

    /// Sends the person to the issuer. `requested_return` is the page they
    /// came from; anything but a same-origin path returns them to `/`.
    pub fn start(&self, requested_return: Option<&str>, now: u64) -> Result<Started, SignInError> {
        let (location, sign_in) = self.client.begin()?;
        let stored = StoredTransaction {
            sign_in,
            return_path: safe_return_path(requested_return),
            expires_at: now.saturating_add(TRANSACTION_TTL_SECONDS),
        };
        let value = self.seal(&stored).ok_or(SignInError::EntropyUnavailable)?;
        Ok(Started {
            location,
            set_cookie: format!(
                "{TRANSACTION_COOKIE}={value}; Path=/; Max-Age={TRANSACTION_TTL_SECONDS}; Secure; HttpOnly; SameSite=Lax"
            ),
        })
    }

    /// Completes the callback: checks the transaction and `state`, exchanges
    /// the code, verifies the token and resolves the Bond.
    pub async fn finish(
        &self,
        repository: &crate::IdentityRepository,
        cookie_header: Option<&str>,
        callback: Callback<'_>,
        now: u64,
    ) -> Finished {
        Finished {
            outcome: self.resolve(repository, cookie_header, callback, now).await,
            clear_cookie: format!(
                "{TRANSACTION_COOKIE}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax"
            ),
        }
    }

    async fn resolve(
        &self,
        repository: &crate::IdentityRepository,
        cookie_header: Option<&str>,
        callback: Callback<'_>,
        now: u64,
    ) -> Result<SignedIn, SignInFailure> {
        let raw = cookie_header
            .and_then(|header| cookie_value(header, TRANSACTION_COOKIE))
            .ok_or(SignInFailure::Expired)?;
        let stored = self.open(raw).ok_or(SignInFailure::Invalid)?;
        if stored.expires_at <= now {
            return Err(SignInFailure::Expired);
        }
        // `state` first: an answer that is not to this transaction says
        // nothing, not even that it was declined.
        let state_matches = callback.state.is_some_and(|value| {
            value
                .as_bytes()
                .ct_eq(stored.sign_in.state.as_bytes())
                .into()
        });
        if !state_matches {
            return Err(SignInFailure::Invalid);
        }
        if callback.error.is_some() {
            return Err(SignInFailure::Cancelled);
        }
        let code = callback.code.ok_or(SignInFailure::Invalid)?;
        let verified = match self.client.complete(code, &stored.sign_in, now).await {
            Ok(value) => value,
            Err(SignInError::Unavailable) => return Err(SignInFailure::Unavailable),
            Err(_) => return Err(SignInFailure::Invalid),
        };
        match repository.resolve_aiaiaiai_account(&verified, now).await {
            Ok(crate::AiaiaiaiAccountOutcome::Linked { pub_dress, .. }) => Ok(SignedIn {
                pub_dress,
                return_path: stored.return_path,
            }),
            Ok(crate::AiaiaiaiAccountOutcome::Unknown) => Err(SignInFailure::AccountUnknown),
            Ok(crate::AiaiaiaiAccountOutcome::BondLinkedElsewhere) => {
                Err(SignInFailure::AccountConflict)
            }
            Err(_) => Err(SignInFailure::Unavailable),
        }
    }

    fn seal(&self, stored: &StoredTransaction) -> Option<String> {
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(stored).ok()?);
        let mac = self.signer.digest(TRANSACTION_DOMAIN, &encoded);
        Some(format!("v1.{encoded}.{}", URL_SAFE_NO_PAD.encode(mac)))
    }

    fn open(&self, value: &str) -> Option<StoredTransaction> {
        let mut fields = value.split('.');
        let (Some("v1"), Some(encoded), Some(mac), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return None;
        };
        let supplied = URL_SAFE_NO_PAD.decode(mac).ok()?;
        let expected = self.signer.digest(TRANSACTION_DOMAIN, encoded);
        if !bool::from(supplied.ct_eq(&expected)) {
            return None;
        }
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(encoded).ok()?).ok()
    }
}

fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name && !value.is_empty()).then_some(value)
    })
}

/// Only a same-origin path may be a destination: an absolute URL, a
/// scheme-relative `//host` or a backslash would make the sign-in an open
/// redirect.
pub fn safe_return_path(value: Option<&str>) -> String {
    match value {
        Some(path)
            if path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains('\\')
                && path.len() <= MAX_RETURN_PATH_BYTES
                && !path.bytes().any(|byte| byte.is_ascii_control()) =>
        {
            path.to_owned()
        }
        _ => "/".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer as _, SigningKey};
    use serde_json::json;

    use super::*;

    const ISSUER: &str = "https://auth.aiaiaiai.org";
    const NOW: u64 = 1_800_000_000;

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    fn key_set(key: &SigningKey, kid: &str) -> JsonWebKeySet {
        serde_json::from_value(json!({ "keys": [{
            "kty": "OKP", "crv": "Ed25519", "use": "sig", "alg": "EdDSA", "kid": kid,
            "x": URL_SAFE_NO_PAD.encode(key.verifying_key().as_bytes()),
        }]}))
        .unwrap()
    }

    fn sign(key: &SigningKey, header: serde_json::Value, claims: serde_json::Value) -> String {
        let signed = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = key.sign(signed.as_bytes());
        format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    fn claims() -> serde_json::Value {
        json!({
            "iss": ISSUER, "sub": "pairwise-subject", "aud": "nilx-one",
            "exp": NOW + 300, "iat": NOW, "auth_time": NOW - 5,
            "nonce": "the-nonce", "pub_dress": "0x0sky",
        })
    }

    fn header() -> serde_json::Value {
        json!({ "alg": "EdDSA", "kid": "k1", "typ": "JWT" })
    }

    fn expectation() -> Expectation<'static> {
        Expectation {
            issuer: ISSUER,
            client_id: "nilx-one",
            nonce: "the-nonce",
        }
    }

    fn verify(token: &str) -> Result<VerifiedSignIn, SignInError> {
        verify_id_token(token, &key_set(&signing_key(), "k1"), &expectation(), NOW)
    }

    fn with(field: &str, value: serde_json::Value) -> String {
        let mut claims = claims();
        claims[field] = value;
        sign(&signing_key(), header(), claims)
    }

    #[test]
    fn accepts_a_token_that_answers_this_transaction() {
        let verified = verify(&sign(&signing_key(), header(), claims())).unwrap();
        assert_eq!(verified.subject, "pairwise-subject");
        assert_eq!(verified.pub_dress.as_deref(), Some("0x0sky"));
        assert_eq!(verified.issuer, ISSUER);
    }

    #[test]
    fn refuses_another_algorithm_before_looking_at_keys() {
        let token = sign(
            &signing_key(),
            json!({ "alg": "HS256", "kid": "k1" }),
            claims(),
        );
        assert_eq!(verify(&token), Err(SignInError::InvalidToken("algorithm")));
        let token = sign(
            &signing_key(),
            json!({ "alg": "none", "kid": "k1" }),
            claims(),
        );
        assert_eq!(verify(&token), Err(SignInError::InvalidToken("algorithm")));
    }

    #[test]
    fn refuses_a_token_signed_by_another_key() {
        let token = sign(&SigningKey::from_bytes(&[9; 32]), header(), claims());
        assert_eq!(verify(&token), Err(SignInError::InvalidToken("signature")));
    }

    #[test]
    fn refuses_a_tampered_payload() {
        let token = sign(&signing_key(), header(), claims());
        let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
        let mut forged = claims();
        forged["sub"] = json!("someone-else");
        parts[1] = URL_SAFE_NO_PAD.encode(forged.to_string());
        assert_eq!(
            verify(&parts.join(".")),
            Err(SignInError::InvalidToken("signature"))
        );
    }

    #[test]
    fn refuses_an_unknown_key_id() {
        let token = sign(
            &signing_key(),
            json!({ "alg": "EdDSA", "kid": "k2" }),
            claims(),
        );
        assert_eq!(
            verify(&token),
            Err(SignInError::InvalidToken("unknown_key"))
        );
    }

    #[test]
    fn refuses_another_issuer_audience_or_nonce() {
        assert_eq!(
            verify(&with("iss", json!("https://evil.example"))),
            Err(SignInError::InvalidToken("issuer"))
        );
        assert_eq!(
            verify(&with("aud", json!("0xda-market"))),
            Err(SignInError::InvalidToken("audience"))
        );
        assert_eq!(
            verify(&with("aud", json!(["nilx-one", "0xda-market"]))),
            Err(SignInError::InvalidToken("audience"))
        );
        assert!(verify(&with("aud", json!(["nilx-one"]))).is_ok());
        assert_eq!(
            verify(&with("nonce", json!("replayed"))),
            Err(SignInError::InvalidToken("nonce"))
        );
    }

    #[test]
    fn refuses_expired_or_stale_sign_ins() {
        assert_eq!(
            verify(&with("exp", json!(NOW - LEEWAY_SECONDS))),
            Err(SignInError::InvalidToken("expired"))
        );
        assert_eq!(
            verify(&with("iat", json!(NOW + LEEWAY_SECONDS + 1))),
            Err(SignInError::InvalidToken("issued_in_future"))
        );
        assert_eq!(
            verify(&with("auth_time", json!(NOW - MAX_AUTH_AGE_SECONDS - 1))),
            Err(SignInError::InvalidToken("auth_time"))
        );
        let mut missing = claims();
        missing.as_object_mut().unwrap().remove("auth_time");
        assert_eq!(
            verify(&sign(&signing_key(), header(), missing)),
            Err(SignInError::InvalidToken("auth_time"))
        );
    }

    #[test]
    fn refuses_an_empty_or_malformed_subject() {
        assert_eq!(
            verify(&with("sub", json!(""))),
            Err(SignInError::InvalidToken("subject"))
        );
        assert_eq!(
            verify(&with("sub", json!("a b"))),
            Err(SignInError::InvalidToken("subject"))
        );
    }

    #[test]
    fn refuses_malformed_tokens() {
        assert_eq!(verify("a.b"), Err(SignInError::InvalidToken("format")));
        assert_eq!(verify("a.b.c.d"), Err(SignInError::InvalidToken("format")));
        assert_eq!(verify("!.b.c"), Err(SignInError::InvalidToken("header")));
    }

    fn config() -> SignInClientConfig {
        SignInClientConfig::new(
            ISSUER,
            None,
            "nilx-one",
            "a-client-secret-of-thirty-two-bytes!",
            "https://nilx.one/api/v1/auth/aiaiaiai/callback",
        )
        .unwrap()
    }

    #[test]
    fn begins_with_a_pkce_bound_authorization_request() {
        let client = SignInClient::new(config());
        let (url, transaction) = client.begin().unwrap();
        assert_eq!(url.origin().ascii_serialization(), ISSUER);
        assert_eq!(url.path(), "/authorize");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["client_id"], "nilx-one");
        assert_eq!(query["scope"], SCOPE);
        assert_eq!(query["state"], transaction.state);
        assert_eq!(query["nonce"], transaction.nonce);
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(
            query["code_challenge"],
            code_challenge(&transaction.code_verifier)
        );
        assert!(!query.contains_key("code_verifier"));
        let (_, other) = client.begin().unwrap();
        assert_ne!(transaction, other);
    }

    #[test]
    fn debug_output_never_shows_the_client_secret() {
        let shown = format!("{:?}", SignInClient::new(config()));
        assert!(
            !shown.contains("a-client-secret-of-thirty-two-bytes!"),
            "{shown}"
        );
        assert!(shown.contains("<redacted>"));
    }

    #[test]
    fn rejects_unsafe_configuration() {
        let secret = "a-client-secret-of-thirty-two-bytes!";
        let redirect = "https://nilx.one/cb";
        let new = |issuer, backchannel, client, secret, redirect| {
            SignInClientConfig::new(issuer, backchannel, client, secret, redirect).err()
        };
        assert_eq!(
            new(
                "http://auth.aiaiaiai.org",
                None,
                "nilx-one",
                secret,
                redirect
            ),
            Some(SignInConfigError::Issuer)
        );
        assert_eq!(
            new(
                "https://auth.aiaiaiai.org/x",
                None,
                "nilx-one",
                secret,
                redirect
            ),
            Some(SignInConfigError::Issuer)
        );
        assert_eq!(
            new(
                ISSUER,
                Some("http://auth.example.org"),
                "nilx-one",
                secret,
                redirect
            ),
            Some(SignInConfigError::Backchannel)
        );
        assert_eq!(
            new(ISSUER, None, "Nilx One", secret, redirect),
            Some(SignInConfigError::ClientId)
        );
        assert_eq!(
            new(ISSUER, None, "nilx-one", "short", redirect),
            Some(SignInConfigError::ClientSecret)
        );
        assert_eq!(
            new(ISSUER, None, "nilx-one", secret, "http://nilx.one/cb"),
            Some(SignInConfigError::RedirectUri)
        );
        for backchannel in [
            "http://nilxone-identity:1927",
            "http://127.0.0.1:1927",
            "https://auth.aiaiaiai.org",
        ] {
            assert_eq!(
                new(ISSUER, Some(backchannel), "nilx-one", secret, redirect),
                None,
                "{backchannel}"
            );
        }
        assert_eq!(config().issuer(), ISSUER);
    }

    fn flow() -> SignInFlow {
        let digester = crate::NativeAuthConfig::new([1; 32], [2; 32])
            .unwrap()
            .secret_digester();
        SignInFlow::new(SignInClient::new(config()), digester)
    }

    fn cookie_of(started: &Started) -> String {
        started.set_cookie.split(';').next().unwrap().to_owned()
    }

    fn state_of(started: &Started) -> String {
        started
            .location
            .query_pairs()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value.into_owned())
            .unwrap()
    }

    async fn repository() -> crate::IdentityRepository {
        crate::IdentityRepository::connect("sqlite::memory:")
            .await
            .unwrap()
    }

    #[test]
    fn start_sets_a_host_only_secure_transaction_cookie() {
        let started = flow().start(Some("/map"), NOW).unwrap();
        assert!(started.set_cookie.starts_with("__Host-0x1_aiaiaiai=v1."));
        for attribute in [
            "Path=/",
            "Secure",
            "HttpOnly",
            "SameSite=Lax",
            "Max-Age=900",
        ] {
            assert!(started.set_cookie.contains(attribute), "{attribute}");
        }
        assert!(!started.set_cookie.contains("Domain="));
        assert_eq!(started.location.path(), "/authorize");
    }

    #[tokio::test]
    async fn finish_refuses_what_is_not_this_transaction() {
        let flow = flow();
        let repository = repository().await;
        let started = flow.start(None, NOW).unwrap();
        let cookie = cookie_of(&started);
        let state = state_of(&started);
        let finish = |header: Option<String>, callback: Callback<'static>, now| {
            let flow = flow.clone();
            let repository = repository.clone();
            async move {
                flow.finish(&repository, header.as_deref(), callback, now)
                    .await
                    .outcome
            }
        };
        let leaked: &'static str = Box::leak(state.into_boxed_str());
        let answer = Callback {
            code: Some("code"),
            state: Some(leaked),
            error: None,
        };
        assert_eq!(finish(None, answer, NOW).await, Err(SignInFailure::Expired));
        assert_eq!(
            finish(Some(cookie.clone()), answer, NOW + TRANSACTION_TTL_SECONDS).await,
            Err(SignInFailure::Expired)
        );
        let tampered = cookie.replacen("v1.", "v1.x", 1);
        assert_eq!(
            finish(Some(tampered), answer, NOW).await,
            Err(SignInFailure::Invalid)
        );
        let forged = Callback {
            state: Some("forged"),
            ..answer
        };
        assert_eq!(
            finish(Some(cookie.clone()), forged, NOW).await,
            Err(SignInFailure::Invalid)
        );
        // A decline with a foreign state is not even a decline.
        let foreign_decline = Callback {
            code: None,
            state: Some("forged"),
            error: Some("access_denied"),
        };
        assert_eq!(
            finish(Some(cookie.clone()), foreign_decline, NOW).await,
            Err(SignInFailure::Invalid)
        );
        let declined = Callback {
            code: None,
            error: Some("access_denied"),
            ..answer
        };
        assert_eq!(
            finish(Some(cookie.clone()), declined, NOW).await,
            Err(SignInFailure::Cancelled)
        );
        let without_code = Callback {
            code: None,
            ..answer
        };
        assert_eq!(
            finish(Some(cookie), without_code, NOW).await,
            Err(SignInFailure::Invalid)
        );
    }

    #[tokio::test]
    async fn finish_always_clears_the_transaction() {
        let finished = flow()
            .finish(&repository().await, None, Callback::default(), NOW)
            .await;
        assert!(finished.clear_cookie.starts_with("__Host-0x1_aiaiaiai=;"));
        assert!(finished.clear_cookie.contains("Max-Age=0"));
    }

    #[test]
    fn a_return_path_never_leaves_the_site() {
        for (requested, expected) in [
            (Some("/map?x=1"), "/map?x=1"),
            (Some("https://evil.example"), "/"),
            (Some("//evil.example"), "/"),
            (Some("/\\evil.example"), "/"),
            (Some("/a\nb"), "/"),
            (None, "/"),
        ] {
            assert_eq!(safe_return_path(requested), expected, "{requested:?}");
        }
    }

    #[test]
    fn debug_output_never_shows_the_transaction_key() {
        let shown = format!("{:?}", flow());
        assert!(shown.contains("SecretDigester(<redacted>)"), "{shown}");
    }
}
