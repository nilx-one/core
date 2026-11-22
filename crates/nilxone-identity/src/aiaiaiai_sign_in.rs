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

#[path = "ais_purchase.rs"]
pub mod purchase;

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
    /// `None` for a public client, which holds no secret and proves its code
    /// with the PKCE verifier alone.
    client_secret: Option<String>,
    redirect_uri: Url,
}

impl SignInClientConfig {
    /// A client that holds a secret AIS has the digest of. `issuer` is the origin people see (`https://auth.aiaiaiai.org`) and the
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
        if client_secret.len() < MIN_CLIENT_SECRET_BYTES {
            return Err(SignInConfigError::ClientSecret);
        }
        Self::build(
            issuer,
            backchannel,
            client_id,
            Some(client_secret),
            redirect_uri,
        )
    }

    /// A public client: no secret to generate, store or carry. The code is
    /// bound to the client by its PKCE verifier, which never leaves the
    /// host's own transaction cookie, and by the exact https redirect URI AIS
    /// has registered. The client must be registered without a secret there.
    pub fn public(
        issuer: &str,
        backchannel: Option<&str>,
        client_id: &str,
        redirect_uri: &str,
    ) -> Result<Self, SignInConfigError> {
        Self::build(issuer, backchannel, client_id, None, redirect_uri)
    }

    fn build(
        issuer: &str,
        backchannel: Option<&str>,
        client_id: &str,
        client_secret: Option<&str>,
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
        let redirect_uri = Url::parse(redirect_uri).map_err(|_| SignInConfigError::RedirectUri)?;
        if redirect_uri.scheme() != "https" || redirect_uri.fragment().is_some() {
            return Err(SignInConfigError::RedirectUri);
        }
        Ok(Self {
            issuer,
            backchannel,
            client_id: client_id.to_owned(),
            client_secret: client_secret.map(str::to_owned),
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

    pub(crate) fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
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
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "<redacted>"),
            )
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
    /// The public address at sign-in: a mutable display snapshot, never the
    /// identity key. Only the legacy account resolver uses it for a first link;
    /// the separate product store attaches exclusively by `(issuer, subject)`.
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

    pub(crate) fn authorization_url(&self, transaction: &SignInTransaction) -> Url {
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
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
            ("code_verifier", transaction.code_verifier.as_str()),
        ];
        let mut request = self.http.post(token_url);
        match &self.config.client_secret {
            Some(secret) => {
                request = request.basic_auth(
                    form_encode(&self.config.client_id),
                    Some(form_encode(secret)),
                );
            }
            // A public client names itself and sends no secret.
            None => form.push(("client_id", self.config.client_id.as_str())),
        }
        let response = request
            .form(&form)
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

pub(crate) const TELEGRAM_PROOF_MAX_LIFETIME_SECONDS: u32 = 120;

/// Verified signature and claims for an AIS lookup of an existing Telegram
/// binding. Core must consume the nonce and proof ID before using the subject.
#[derive(Eq, PartialEq)]
pub(crate) struct VerifiedTelegramProvider {
    pairwise_subject: String,
    replay_id: String,
}

impl std::fmt::Debug for VerifiedTelegramProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VerifiedTelegramProvider")
            .finish_non_exhaustive()
    }
}

impl VerifiedTelegramProvider {
    pub(crate) fn pairwise_subject(&self) -> &str {
        &self.pairwise_subject
    }

    pub(crate) fn replay_id(&self) -> &str {
        &self.replay_id
    }
}

#[derive(Deserialize)]
struct ProviderProofHeader {
    alg: String,
    typ: String,
    kid: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderProofClaims {
    iss: String,
    aud: String,
    sub: String,
    azp: String,
    provider: String,
    purpose: String,
    nonce: String,
    iat: u64,
    exp: u64,
    jti: String,
}

/// Verify the short-lived `provider-evidence+jwt` issued only by AIS's
/// `/family/telegram-identity` route. The issuer cannot decide Core session
/// authority, and this function does not perform an authorization transition.
///
/// The Gateway must generate and remember a cryptographically random nonce
/// before asking AIS, and atomically mark both request and returned `jti`
/// consumed. A read from another process, a reused nonce or missing storage
/// proof must be refused by the host, even when this signature is valid.
///
/// # Errors
///
/// Any signature, audience, purpose, nonce, lifetime or claim mismatch is a
/// terminal rejection. Neither Telegram account equality nor `pub_dress`
/// can override it.
pub(crate) fn verify_telegram_provider_proof(
    proof: &str,
    keys: &JsonWebKeySet,
    expected: &Expectation<'_>,
    now: u64,
) -> Result<VerifiedTelegramProvider, SignInError> {
    if proof.len() > 4096 || expected.client_id != "0x1" {
        return Err(SignInError::InvalidToken("telegram_proof_context"));
    }
    if !(32..=128).contains(&expected.nonce.len())
        || !expected
            .nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SignInError::InvalidToken("nonce"));
    }
    let mut parts = proof.split('.');
    let (Some(header_part), Some(claims_part), Some(signature_part), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(SignInError::InvalidToken("format"));
    };
    let header: ProviderProofHeader = decode_json(header_part, "header")?;
    if header.alg != ALGORITHM || header.typ != "provider-evidence+jwt" {
        return Err(SignInError::InvalidToken("telegram_proof_type"));
    }
    if header.kid.is_empty() {
        return Err(SignInError::InvalidToken("kid"));
    }
    let key = signing_key(keys, Some(&header.kid))?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature_part)
        .map_err(|_| SignInError::InvalidToken("signature"))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| SignInError::InvalidToken("signature"))?;
    let signed = &proof[..header_part.len() + 1 + claims_part.len()];
    key.verify_strict(signed.as_bytes(), &signature)
        .map_err(|_| SignInError::InvalidToken("signature"))?;

    let claims: ProviderProofClaims = decode_json(claims_part, "telegram_proof_claims")?;
    if claims.iss != expected.issuer {
        return Err(SignInError::InvalidToken("issuer"));
    }
    if claims.aud != "0x1" || claims.azp != "0x1" {
        return Err(SignInError::InvalidToken("audience"));
    }
    if claims.provider != "telegram" || claims.purpose != "telegram.identity.resolve" {
        return Err(SignInError::InvalidToken("telegram_proof_purpose"));
    }
    if !bool::from(claims.nonce.as_bytes().ct_eq(expected.nonce.as_bytes())) {
        return Err(SignInError::InvalidToken("nonce"));
    }
    if claims.iat > now
        || claims.exp <= now
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > u64::from(TELEGRAM_PROOF_MAX_LIFETIME_SECONDS)
    {
        return Err(SignInError::InvalidToken("telegram_proof_lifetime"));
    }
    let subject_valid = claims.sub.len() == 43
        && URL_SAFE_NO_PAD
            .decode(&claims.sub)
            .is_ok_and(|bytes| bytes.len() == 32);
    if !subject_valid
        || !(16..=128).contains(&claims.jti.len())
        || !claims
            .jti
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(SignInError::InvalidToken("telegram_proof_identity"));
    }
    Ok(VerifiedTelegramProvider {
        pairwise_subject: claims.sub,
        replay_id: claims.jti,
    })
}

/// A verified, client-specific continuity assertion from AIS. Only the
/// issuer may make this, after authenticating the same private person for
/// both pairwise subject namespaces. No public address participates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedClientTransition {
    successor: VerifiedSignIn,
    predecessor_subject: String,
}

impl VerifiedClientTransition {
    pub fn identity(&self) -> &VerifiedSignIn {
        &self.successor
    }

    pub fn predecessor_subject(&self) -> &str {
        &self.predecessor_subject
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubjectContinuity {
    from_client_id: String,
    sub: String,
}

#[derive(Deserialize)]
struct TransitionClaims {
    subject_continuity: Option<SubjectContinuity>,
}

/// Verify AIS's signed old->new subject continuity only for the exact
/// nilx-one -> 0x1 client migration. Ordinary sign-in accepts no alias.
///
/// The signature, issuer, audience (0x1), nonce, freshness and token lifetime
/// are checked before interpreting the continuity claim. The two subjects
/// must be distinct, canonical 32-byte pairwise digests. An unsigned
/// request-supplied subject, a matching pub_dress, or another OIDC audience
/// never provides this proof.
pub fn verify_client_transition_token(
    token: &str,
    keys: &JsonWebKeySet,
    expected: &Expectation<'_>,
    now: u64,
) -> Result<VerifiedClientTransition, SignInError> {
    if expected.client_id != "0x1" {
        return Err(SignInError::InvalidToken("continuity_client"));
    }
    let successor = verify_id_token(token, keys, expected, now)?;
    let payload = token
        .split('.')
        .nth(1)
        .ok_or(SignInError::InvalidToken("continuity"))?;
    let claims: TransitionClaims = decode_json(payload, "continuity")?;
    let continuity = claims
        .subject_continuity
        .ok_or(SignInError::InvalidToken("continuity"))?;
    let pairwise = |value: &str| {
        value.len() == 43
            && URL_SAFE_NO_PAD
                .decode(value)
                .is_ok_and(|bytes| bytes.len() == 32)
    };
    if continuity.from_client_id != "nilx-one"
        || !pairwise(&continuity.sub)
        || !pairwise(&successor.subject)
        || continuity.sub == successor.subject
    {
        return Err(SignInError::InvalidToken("continuity"));
    }
    Ok(VerifiedClientTransition {
        successor,
        predecessor_subject: continuity.sub,
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
/// Migration login uses a different browser transaction than normal product sign-in.
pub const MIGRATION_TRANSACTION_COOKIE: &str = "__Host-0x1_aiaiaiai_migration";
const TRANSACTION_TTL_SECONDS: u64 = 15 * 60;
const TRANSACTION_DOMAIN: &str = "aiaiaiai-sign-in-transaction";
const MIGRATION_TRANSACTION_DOMAIN: &str = "aiaiaiai-legacy-bond-migration-transaction";
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

/// A completed OIDC callback, before any 0x1 storage is consulted.
/// The identity is accessible only after transaction and token verification.
#[derive(Clone)]
pub struct VerifiedCallback {
    identity: VerifiedSignIn,
    return_path: String,
}

impl VerifiedCallback {
    pub fn identity(&self) -> &VerifiedSignIn {
        &self.identity
    }

    pub fn return_path(&self) -> &str {
        &self.return_path
    }
}

/// Verification clears the transaction cookie on both success and failure.
pub struct VerifiedFinished {
    pub outcome: Result<VerifiedCallback, SignInFailure>,
    pub clear_cookie: String,
}

/// 0x1's sign-in through aiaiaiai Identity, end to end. The host passes the
/// request's query and `Cookie` header in, and applies the redirect and the
/// `Set-Cookie` values it gets back. It decides nothing itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SignInFlowPurpose {
    Product,
    LegacyMigration,
}

#[derive(Clone, Debug)]
pub struct SignInFlow {
    client: SignInClient,
    signer: crate::SecretDigester,
    purpose: SignInFlowPurpose,
}

impl SignInFlow {
    /// `signer` keys the transaction cookie; the host's native authentication
    /// secret, under this module's own domain.
    pub fn new(client: SignInClient, signer: crate::SecretDigester) -> Self {
        Self {
            client,
            signer,
            purpose: SignInFlowPurpose::Product,
        }
    }

    /// Separate the opt-in legacy Bond migration from ordinary product sign-in.
    /// Only the 0x1 OIDC audience may prove pre-migration ownership.
    /// This scope uses its own cookie name AND MAC domain, so neither flow can
    /// accept the other's browser transaction even under the same host key.
    pub fn legacy_migration(client: SignInClient, signer: crate::SecretDigester) -> Option<Self> {
        (client.config().client_id() == "0x1").then_some(Self {
            client,
            signer,
            purpose: SignInFlowPurpose::LegacyMigration,
        })
    }

    /// A migration proof must not use a regular product sign-in transaction.
    #[must_use]
    pub const fn is_legacy_migration(&self) -> bool {
        matches!(self.purpose, SignInFlowPurpose::LegacyMigration)
    }

    const fn transaction_cookie(&self) -> &'static str {
        match self.purpose {
            SignInFlowPurpose::Product => TRANSACTION_COOKIE,
            SignInFlowPurpose::LegacyMigration => MIGRATION_TRANSACTION_COOKIE,
        }
    }

    const fn transaction_domain(&self) -> &'static str {
        match self.purpose {
            SignInFlowPurpose::Product => TRANSACTION_DOMAIN,
            SignInFlowPurpose::LegacyMigration => MIGRATION_TRANSACTION_DOMAIN,
        }
    }

    pub fn issuer(&self) -> &str {
        self.client.config().issuer()
    }

    pub fn client_id(&self) -> &str {
        self.client.config().client_id()
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
        let cookie_name = self.transaction_cookie();
        Ok(Started {
            location,
            set_cookie: format!(
                "{cookie_name}={value}; Path=/; Max-Age={TRANSACTION_TTL_SECONDS}; Secure; HttpOnly; SameSite=Lax"
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
        let verified = self.verify_callback(cookie_header, callback, now).await;
        let outcome = match verified.outcome {
            Ok(verified) => match repository
                .resolve_aiaiaiai_account(verified.identity(), now)
                .await
            {
                Ok(crate::AiaiaiaiAccountOutcome::Linked { pub_dress, .. }) => Ok(SignedIn {
                    pub_dress,
                    return_path: verified.return_path,
                }),
                Ok(crate::AiaiaiaiAccountOutcome::Unknown) => Err(SignInFailure::AccountUnknown),
                Ok(crate::AiaiaiaiAccountOutcome::BondLinkedElsewhere) => {
                    Err(SignInFailure::AccountConflict)
                }
                Err(_) => Err(SignInFailure::Unavailable),
            },
            Err(error) => Err(error),
        };
        Finished {
            outcome,
            clear_cookie: verified.clear_cookie,
        }
    }

    /// Verifies the issuer callback without reading the legacy identity/game
    /// database. Product services can attach their own state by `(iss, sub)`.
    pub async fn verify_callback(
        &self,
        cookie_header: Option<&str>,
        callback: Callback<'_>,
        now: u64,
    ) -> VerifiedFinished {
        VerifiedFinished {
            outcome: self
                .verify_callback_inner(cookie_header, callback, now)
                .await,
            clear_cookie: format!(
                "{}=; Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Lax",
                self.transaction_cookie()
            ),
        }
    }

    async fn verify_callback_inner(
        &self,
        cookie_header: Option<&str>,
        callback: Callback<'_>,
        now: u64,
    ) -> Result<VerifiedCallback, SignInFailure> {
        let raw = cookie_header
            .and_then(|header| cookie_value(header, self.transaction_cookie()))
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
        Ok(VerifiedCallback {
            identity: verified,
            return_path: stored.return_path,
        })
    }

    fn seal(&self, stored: &StoredTransaction) -> Option<String> {
        let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(stored).ok()?);
        let mac = self.signer.digest(self.transaction_domain(), &encoded);
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
        let expected = self.signer.digest(self.transaction_domain(), encoded);
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

    fn telegram_proof_claims() -> serde_json::Value {
        json!({
            "iss": ISSUER,
            "aud": "0x1",
            "sub": URL_SAFE_NO_PAD.encode([33_u8; 32]),
            "azp": "0x1",
            "provider": "telegram",
            "purpose": "telegram.identity.resolve",
            "nonce": "a-fresh-core-gateway-nonce-000000000000001",
            "iat": NOW,
            "exp": NOW + 120,
            "jti": "test-proof-replay-id-1234",
        })
    }

    fn telegram_proof_header() -> serde_json::Value {
        json!({ "alg": "EdDSA", "kid": "k1", "typ": "provider-evidence+jwt" })
    }

    fn telegram_expected() -> Expectation<'static> {
        Expectation {
            issuer: ISSUER,
            client_id: "0x1",
            nonce: "a-fresh-core-gateway-nonce-000000000000001",
        }
    }

    #[test]
    fn verifies_ais_provider_evidence_without_granting_a_session() {
        let claim = telegram_proof_claims();
        let encoded = sign(&signing_key(), telegram_proof_header(), claim.clone());
        let verified = verify_telegram_provider_proof(
            &encoded,
            &key_set(&signing_key(), "k1"),
            &telegram_expected(),
            NOW,
        )
        .unwrap();
        assert_eq!(verified.pairwise_subject, claim["sub"].as_str().unwrap());
        assert_eq!(verified.replay_id, "test-proof-replay-id-1234");
    }

    #[test]
    fn rejects_cross_product_replay_tampering_and_untrusted_issuer() {
        let keys = key_set(&signing_key(), "k1");
        let proof = sign(
            &signing_key(),
            telegram_proof_header(),
            telegram_proof_claims(),
        );
        let expected = telegram_expected();

        let mismatch = Expectation {
            nonce: "another-core-request-nonce-000000000000001",
            ..expected
        };
        assert_eq!(
            verify_telegram_provider_proof(&proof, &keys, &mismatch, NOW),
            Err(SignInError::InvalidToken("nonce"))
        );
        assert_eq!(
            verify_telegram_provider_proof(
                &proof,
                &keys,
                &Expectation {
                    client_id: "nilx-one",
                    ..telegram_expected()
                },
                NOW
            ),
            Err(SignInError::InvalidToken("telegram_proof_context"))
        );
        let bad_signer = sign(
            &SigningKey::from_bytes(&[9_u8; 32]),
            telegram_proof_header(),
            telegram_proof_claims(),
        );
        assert_eq!(
            verify_telegram_provider_proof(&bad_signer, &keys, &telegram_expected(), NOW),
            Err(SignInError::InvalidToken("signature"))
        );
        let jwt_header = sign(&signing_key(), header(), telegram_proof_claims());
        assert_eq!(
            verify_telegram_provider_proof(&jwt_header, &keys, &telegram_expected(), NOW),
            Err(SignInError::InvalidToken("telegram_proof_type"))
        );
        let mut changed = telegram_proof_claims();
        changed["aud"] = json!("another-product");
        let wrong_audience = sign(&signing_key(), telegram_proof_header(), changed);
        assert_eq!(
            verify_telegram_provider_proof(&wrong_audience, &keys, &telegram_expected(), NOW),
            Err(SignInError::InvalidToken("audience"))
        );
    }

    #[test]
    fn rejects_signed_proofs_with_invalid_nonce_grammar() {
        let keys = key_set(&signing_key(), "k1");
        for nonce in [
            "",
            "too-short",
            "invalid/nonce-even-though-it-is-long-enough",
        ] {
            let mut claims = telegram_proof_claims();
            claims["nonce"] = json!(nonce);
            let proof = sign(&signing_key(), telegram_proof_header(), claims);
            let expected = Expectation {
                nonce,
                ..telegram_expected()
            };
            assert_eq!(
                verify_telegram_provider_proof(&proof, &keys, &expected, NOW),
                Err(SignInError::InvalidToken("nonce"))
            );
        }
    }

    #[test]
    fn rejects_forged_purpose_lifetime_and_external_identity_values() {
        let keys = key_set(&signing_key(), "k1");
        let failure = |key: &str, value: serde_json::Value, expected: &'static str| {
            let mut payload = telegram_proof_claims();
            payload[key] = value;
            let proof = sign(&signing_key(), telegram_proof_header(), payload);
            assert_eq!(
                verify_telegram_provider_proof(&proof, &keys, &telegram_expected(), NOW),
                Err(SignInError::InvalidToken(expected))
            );
        };
        failure(
            "purpose",
            json!("telegram.special.purchase"),
            "telegram_proof_purpose",
        );
        failure("provider", json!("discord"), "telegram_proof_purpose");
        failure("iat", json!(NOW + 1), "telegram_proof_lifetime");
        failure("exp", json!(NOW + 121), "telegram_proof_lifetime");
        failure("exp", json!(NOW), "telegram_proof_lifetime");
        failure("sub", json!("0x0sky"), "telegram_proof_identity");
        failure("jti", json!("bad id"), "telegram_proof_identity");

        let mut leaked = telegram_proof_claims();
        leaked["telegram_id"] = json!(654321);
        let proof = sign(&signing_key(), telegram_proof_header(), leaked);
        assert_eq!(
            verify_telegram_provider_proof(&proof, &keys, &telegram_expected(), NOW),
            Err(SignInError::InvalidToken("telegram_proof_claims"))
        );
    }

    #[test]
    fn accepts_a_token_that_answers_this_transaction() {
        let verified = verify(&sign(&signing_key(), header(), claims())).unwrap();
        assert_eq!(verified.subject, "pairwise-subject");
        assert_eq!(verified.pub_dress.as_deref(), Some("0x0sky"));
        assert_eq!(verified.issuer, ISSUER);
    }

    #[test]
    fn successor_requires_signed_exact_audience_and_issuer_continuity() {
        let current = URL_SAFE_NO_PAD.encode([41_u8; 32]);
        let previous = URL_SAFE_NO_PAD.encode([17_u8; 32]);
        let mut payload = claims();
        payload["aud"] = json!("0x1");
        payload["sub"] = json!(current.clone());
        payload["subject_continuity"] =
            json!({"from_client_id":"nilx-one", "sub": previous.clone()});
        let keys = key_set(&signing_key(), "k1");
        let target = Expectation {
            issuer: ISSUER,
            client_id: "0x1",
            nonce: "the-nonce",
        };
        let token = sign(&signing_key(), header(), payload.clone());
        let verified = verify_client_transition_token(&token, &keys, &target, NOW).unwrap();
        assert_eq!(verified.identity().subject, current);
        assert_eq!(verified.predecessor_subject(), previous);
        assert_eq!(verified.identity().pub_dress.as_deref(), Some("0x0sky"));
        assert_eq!(
            verify_client_transition_token(&token, &keys, &expectation(), NOW),
            Err(SignInError::InvalidToken("continuity_client"))
        );
        let wrong_aud = sign(&signing_key(), header(), {
            let mut p = payload.clone();
            p["aud"] = json!("prism");
            p
        });
        assert_eq!(
            verify_client_transition_token(&wrong_aud, &keys, &target, NOW),
            Err(SignInError::InvalidToken("audience"))
        );
        let wrong_issuer = sign(&signing_key(), header(), {
            let mut p = payload.clone();
            p["iss"] = json!("https://another.example");
            p
        });
        assert_eq!(
            verify_client_transition_token(&wrong_issuer, &keys, &target, NOW),
            Err(SignInError::InvalidToken("issuer"))
        );
        for (key, value) in [
            ("subject_continuity", serde_json::Value::Null),
            (
                "subject_continuity",
                json!({"from_client_id":"prism", "sub":previous}),
            ),
            (
                "subject_continuity",
                json!({"from_client_id":"nilx-one", "sub":current}),
            ),
            (
                "subject_continuity",
                json!({"from_client_id":"nilx-one", "sub":"bad"}),
            ),
        ] {
            let mut invalid = payload.clone();
            invalid[key] = value;
            assert_eq!(
                verify_client_transition_token(
                    &sign(&signing_key(), header(), invalid),
                    &keys,
                    &target,
                    NOW
                ),
                Err(SignInError::InvalidToken("continuity"))
            );
        }
        let mut forged: Vec<String> = token.split('.').map(str::to_owned).collect();
        let mut changed = payload;
        changed["subject_continuity"]["sub"] = json!(URL_SAFE_NO_PAD.encode([99_u8; 32]));
        forged[1] = URL_SAFE_NO_PAD.encode(changed.to_string());
        assert_eq!(
            verify_client_transition_token(&forged.join("."), &keys, &target, NOW),
            Err(SignInError::InvalidToken("signature"))
        );
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

    const CALLBACK: &str = "https://nilx.one/api/v1/auth/aiaiaiai/callback";

    #[test]
    fn a_public_client_needs_no_secret_and_is_held_to_the_same_rest() {
        let public = SignInClientConfig::public(ISSUER, None, "nilx-one", CALLBACK).unwrap();
        assert!(format!("{public:?}").contains("client_secret: None"));
        // The secret-free path loosens nothing else.
        assert_eq!(
            SignInClientConfig::public("http://auth.aiaiaiai.org", None, "nilx-one", CALLBACK)
                .err(),
            Some(SignInConfigError::Issuer)
        );
        assert_eq!(
            SignInClientConfig::public(ISSUER, None, "Nilx One", CALLBACK).err(),
            Some(SignInConfigError::ClientId)
        );
        assert_eq!(
            SignInClientConfig::public(ISSUER, None, "nilx-one", "http://nilx.one/cb").err(),
            Some(SignInConfigError::RedirectUri)
        );
        // A confidential client still cannot omit or shorten its secret.
        assert_eq!(
            SignInClientConfig::new(ISSUER, None, "nilx-one", "short", CALLBACK).err(),
            Some(SignInConfigError::ClientSecret)
        );
    }

    /// What the client sends to a local `/token`, which refuses it.
    async fn token_request(make: impl FnOnce(&str) -> SignInClientConfig) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let backchannel = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                received.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&received);
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
                if read == 0 {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 400 Bad Request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .unwrap();
            String::from_utf8_lossy(&received).into_owned()
        });
        let client = SignInClient::new(make(&backchannel));
        let transaction = SignInTransaction {
            state: "state".to_owned(),
            nonce: "nonce".to_owned(),
            code_verifier: "v".repeat(43),
        };
        let outcome = client
            .complete("the-code", &transaction, 1_800_000_000)
            .await;
        assert_eq!(outcome, Err(SignInError::CodeRejected));
        server.join().unwrap()
    }

    #[tokio::test]
    async fn the_token_request_names_a_public_client_and_never_sends_a_secret() {
        let request = token_request(|backchannel| {
            SignInClientConfig::public(ISSUER, Some(backchannel), "nilx-one", CALLBACK).unwrap()
        })
        .await
        .to_ascii_lowercase();
        assert!(request.starts_with("post /token "), "{request}");
        assert!(!request.contains("authorization:"), "{request}");
        assert!(request.contains("client_id=nilx-one"), "{request}");
        assert!(request.contains("code_verifier=vvvv"), "{request}");
        assert!(!request.contains("client_secret"), "{request}");

        // A client that holds a secret still authenticates with Basic and
        // does not repeat its id in the body.
        let secret = "a-client-secret-of-thirty-two-bytes!";
        let request = token_request(|backchannel| {
            SignInClientConfig::new(ISSUER, Some(backchannel), "nilx-one", secret, CALLBACK)
                .unwrap()
        })
        .await
        .to_ascii_lowercase();
        assert!(request.contains("authorization: basic "), "{request}");
        assert!(!request.contains("client_id="), "{request}");
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
    async fn legacy_migration_transactions_are_separate_from_product_sign_in() {
        let normal = flow();
        let digester = crate::NativeAuthConfig::new([1; 32], [2; 32])
            .unwrap()
            .secret_digester();
        let product_client = || {
            SignInClientConfig::new(
                ISSUER,
                None,
                "0x1",
                "a-client-secret-of-thirty-two-bytes!",
                CALLBACK,
            )
            .unwrap()
        };
        assert!(
            SignInFlow::legacy_migration(SignInClient::new(config()), digester.clone()).is_none(),
            "only the 0x1 client may prove legacy ownership"
        );
        let migration =
            SignInFlow::legacy_migration(SignInClient::new(product_client()), digester.clone())
                .expect("the registered 0x1 OIDC client is accepted");
        assert!(migration.is_legacy_migration());
        assert!(!normal.is_legacy_migration());
        let product_started = normal.start(Some("/map"), NOW).unwrap();
        let migration_started = migration
            .start(Some("/api/v1/auth/aiaiaiai/migrate/confirm/example"), NOW)
            .unwrap();
        assert!(cookie_of(&product_started).starts_with("__Host-0x1_aiaiaiai=v1."));
        assert!(cookie_of(&migration_started).starts_with("__Host-0x1_aiaiaiai_migration=v1."));
        for started in [&product_started, &migration_started] {
            assert!(
                started
                    .set_cookie
                    .contains("Secure; HttpOnly; SameSite=Lax")
            );
            assert!(!started.set_cookie.contains("Domain="));
        }
        let product_cookie = cookie_of(&product_started);
        let migration_cookie = cookie_of(&migration_started);
        assert_eq!(
            migration
                .verify_callback(Some(&product_cookie), Callback::default(), NOW)
                .await
                .outcome
                .err(),
            Some(SignInFailure::Expired)
        );
        assert_eq!(
            normal
                .verify_callback(Some(&migration_cookie), Callback::default(), NOW)
                .await
                .outcome
                .err(),
            Some(SignInFailure::Expired)
        );
        // Renaming a signed cookie cannot bypass the MAC purpose separation.
        let renamed =
            migration_cookie.replacen(MIGRATION_TRANSACTION_COOKIE, TRANSACTION_COOKIE, 1);
        assert_eq!(
            normal
                .verify_callback(Some(&renamed), Callback::default(), NOW)
                .await
                .outcome
                .err(),
            Some(SignInFailure::Invalid)
        );
        assert!(
            migration
                .verify_callback(Some(&migration_cookie), Callback::default(), NOW)
                .await
                .clear_cookie
                .starts_with("__Host-0x1_aiaiaiai_migration=;")
        );
        let foreign = SignInClientConfig::public(
            ISSUER,
            None,
            "prism",
            "https://nilx.one/api/v1/auth/aiaiaiai/callback",
        )
        .unwrap();
        assert!(SignInFlow::legacy_migration(SignInClient::new(foreign), digester).is_none());
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
