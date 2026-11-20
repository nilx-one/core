// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Verify an AIS checkout permit and settle against a trusted Market order.
//! The application adapter supplies authority for its own environment.

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use super::{JsonWebKeySet, SignInError, Signature, URL_SAFE_NO_PAD, decode_json, signing_key};
use crate::{
    IdentityRepository, RepositoryError,
    market_issuer::{IssuerOrder, TERM_POLICY},
    repository::{IssuerSpecialOrderSnapshot, SpecialSeedsOutcome, SpendAuthorization},
};
use base64::Engine as _;

const SKU: &str = "0x1_special_1m";
const PURPOSE: &str = "telegram.special.purchase";

#[derive(Deserialize)]
struct Header {
    alg: String,
    typ: String,
    kid: String,
}

#[derive(Deserialize)]
pub(crate) struct Claims {
    iss: String,
    aud: String,
    sub: String,
    azp: String,
    purpose: String,
    order_id: String,
    telegram_id: i64,
    sku: String,
    quantity: u8,
    price_id: u64,
    amount_minor: u64,
    denomination: String,
    iat: u64,
    exp: u64,
}

impl Claims {
    pub(crate) fn order_id(&self) -> &str {
        &self.order_id
    }
}

pub(crate) fn verify_permit(
    permit: &str,
    authority: &PurchaseAuthority<'_>,
    now: u64,
) -> Result<Claims, SignInError> {
    verify(
        permit,
        authority.keys,
        authority.issuer,
        authority.product,
        now,
    )
}

fn verify(
    token: &str,
    keys: &JsonWebKeySet,
    issuer: &str,
    product: &str,
    now: u64,
) -> Result<Claims, SignInError> {
    let invalid = || SignInError::InvalidToken("purchase_permit");
    if token.len() > 4096 {
        return Err(invalid());
    }
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid());
    };
    let key: Header = decode_json(header, "purchase_header")?;
    if key.alg != "EdDSA" || key.typ != "purchase+jwt" || key.kid.is_empty() {
        return Err(invalid());
    }
    let bytes = URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?;
    let signature = Signature::from_slice(&bytes).map_err(|_| invalid())?;
    signing_key(keys, Some(&key.kid))?
        .verify_strict(
            &token.as_bytes()[..header.len() + 1 + payload.len()],
            &signature,
        )
        .map_err(|_| invalid())?;
    let claims: Claims = decode_json(payload, "purchase_claims")?;
    if claims.iss != issuer
        || claims.aud != "0x1"
        || claims.azp != product
        || claims.purpose != PURPOSE
        || claims.iat > now
        || claims.exp <= now
        || claims.exp.saturating_sub(claims.iat) > 120
        || claims.sku != SKU
        || claims.quantity != 1
        || claims.denomination != "seeds"
        || claims.telegram_id <= 0
        || claims.price_id == 0
        || claims.amount_minor == 0
        || claims.order_id.is_empty()
        || claims.order_id.len() > 128
        || !claims
            .order_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        || claims.sub.len() != 43
        || !URL_SAFE_NO_PAD
            .decode(&claims.sub)
            .is_ok_and(|decoded| decoded.len() == 32)
    {
        return Err(invalid());
    }
    Ok(claims)
}

fn wallet_order_key(claims: &Claims) -> String {
    let mut digest = Sha256::new();
    for component in [&claims.iss, &claims.azp, &claims.order_id] {
        digest.update((component.len() as u64).to_be_bytes());
        digest.update(component.as_bytes());
    }
    format!("ais-special:{:x}", digest.finalize())
}

#[derive(Debug, thiserror::Error)]
pub enum PurchaseError {
    #[error(transparent)]
    Permit(#[from] SignInError),
    #[error(transparent)]
    Storage(#[from] RepositoryError),
}

/// AIS as the authority for checkout permits: who signs them, which product may request one,
/// and where its public signing keys are read. There is one AIS, so these are values here and
/// not settings of any host. The keys are cached; a permit refused under an older cache is
/// tried once more under fresh keys, at most once a minute, so a stream of bad permits cannot
/// turn the service into a client hammering AIS.
pub struct AisAuthority {
    issuer: String,
    product: String,
    keys_uri: url::Url,
    http: reqwest::Client,
    cached: std::sync::Mutex<Option<(std::time::Instant, std::sync::Arc<JsonWebKeySet>)>>,
}

impl AisAuthority {
    /// The one AIS, and the only product that may request a permit for this checkout.
    pub const ISSUER: &'static str = "https://auth.aiaiaiai.org";
    pub const PRODUCT: &'static str = "0xda-market";

    const KEY_TTL: std::time::Duration = std::time::Duration::from_secs(300);
    const REFRESH_AFTER_REFUSAL: std::time::Duration = std::time::Duration::from_secs(60);
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
    const MAX_KEY_BYTES: usize = 65_536;

    pub fn production() -> Self {
        Self::for_origin(Self::ISSUER, Self::PRODUCT).expect("the pinned AIS origin is a URL")
    }

    /// An authority at another origin, for tests and tooling.
    pub fn for_origin(origin: &str, product: &str) -> Option<Self> {
        let keys_uri = url::Url::parse(origin)
            .ok()?
            .join("/.well-known/jwks.json")
            .ok()?;
        Some(Self {
            issuer: origin.trim_end_matches('/').to_owned(),
            product: product.to_owned(),
            keys_uri,
            http: reqwest::Client::builder()
                .timeout(Self::TIMEOUT)
                .build()
                .ok()?,
            cached: std::sync::Mutex::new(None),
        })
    }

    /// Start with these keys already fetched.
    pub fn with_keys(self, keys: JsonWebKeySet) -> Self {
        *self.lock() = Some((std::time::Instant::now(), std::sync::Arc::new(keys)));
        self
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<(std::time::Instant, std::sync::Arc<JsonWebKeySet>)>>
    {
        // A poisoned lock only means a panic elsewhere; the cache is a hint.
        self.cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn cached(&self, max_age: std::time::Duration) -> Option<std::sync::Arc<JsonWebKeySet>> {
        self.lock()
            .as_ref()
            .filter(|(at, _)| at.elapsed() < max_age)
            .map(|(_, keys)| std::sync::Arc::clone(keys))
    }

    async fn fetch(&self) -> Option<std::sync::Arc<JsonWebKeySet>> {
        let response = self.http.get(self.keys_uri.clone()).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = response
            .bytes()
            .await
            .ok()
            .filter(|b| b.len() <= Self::MAX_KEY_BYTES)?;
        let keys = std::sync::Arc::new(serde_json::from_slice::<JsonWebKeySet>(&body).ok()?);
        *self.lock() = Some((std::time::Instant::now(), std::sync::Arc::clone(&keys)));
        Some(keys)
    }

    pub(crate) async fn current(&self) -> Option<std::sync::Arc<JsonWebKeySet>> {
        match self.cached(Self::KEY_TTL) {
            Some(keys) => Some(keys),
            None => self.fetch().await,
        }
    }

    /// Fresh keys after a refusal, unless the cache is already younger than a minute.
    pub(crate) async fn refreshed(&self) -> Option<std::sync::Arc<JsonWebKeySet>> {
        match self.cached(Self::REFRESH_AFTER_REFUSAL) {
            Some(_) => None,
            None => self.fetch().await,
        }
    }

    pub(crate) fn purchase<'a>(&'a self, keys: &'a JsonWebKeySet) -> PurchaseAuthority<'a> {
        PurchaseAuthority {
            issuer: &self.issuer,
            product: &self.product,
            keys,
        }
    }
}

/// Authority pinned by the caller's environment. The keys must come from
/// the configured issuer, never from the permit or the Telegram request.
pub struct PurchaseAuthority<'a> {
    pub issuer: &'a str,
    pub product: &'a str,
    pub keys: &'a JsonWebKeySet,
}

/// Apply an AIS purchase after the adapter independently authenticates and
/// claims the Market order. Market must freeze the active offer when it claims
/// the order. AIS vouches for the buyer and their confirmation, not the price.
/// The permit's subject is the 0x1 client's; a profile that signed in as the predecessor client
/// is found through its signed continuity link, so the host must have prepared the client
/// transition store (`ClientTransitionRepository::prepare`) on the same database.
pub async fn settle(
    repository: &IdentityRepository,
    authority: &PurchaseAuthority<'_>,
    permit: &str,
    market_order: &IssuerOrder,
    now_ms: u64,
) -> Result<SpecialSeedsOutcome, PurchaseError> {
    let claims = verify_permit(permit, authority, now_ms / 1000)?;
    Ok(settle_claims(repository, &claims, market_order, now_ms).await?)
}

/// Settle verified permit claims against the claimed Market order.
pub(crate) async fn settle_claims(
    repository: &IdentityRepository,
    claims: &Claims,
    market_order: &IssuerOrder,
    now_ms: u64,
) -> Result<SpecialSeedsOutcome, RepositoryError> {
    let market = &market_order.attributes;
    let telegram = claims.telegram_id.to_string();
    if market_order.id != claims.order_id
        || market.sku != claims.sku
        || market.price_id != claims.price_id
        || market.amount_minor != claims.amount_minor
        || market.denomination != claims.denomination
        || market.issuer != "0x1"
        || market.status != "processing"
        || market.link_provider != "telegram"
        || market.link_provider_user_id != telegram
        || market
            .term
            .as_ref()
            .is_none_or(|term| term.months != 1 || term.policy != TERM_POLICY)
    {
        return Ok(SpecialSeedsOutcome::InvalidTerms);
    }
    let Some(owner) = repository
        .bond_for_purchase_subject(&claims.iss, &claims.sub)
        .await?
    else {
        return Ok(SpecialSeedsOutcome::AccountUnlinked);
    };
    let snapshot = IssuerSpecialOrderSnapshot {
        order_id: wallet_order_key(claims),
        authorization: SpendAuthorization::AisTelegram {
            issuer: claims.iss.clone(),
            ais_subject: claims.sub.clone(),
            telegram,
        },
        issuer: market.issuer.clone(),
        denomination: market.denomination.clone(),
        status: market.status.clone(),
        recipient_pub_dress: owner.as_str().to_owned(),
        sku: claims.sku.clone(),
        price_id: claims.price_id,
        amount_minor: claims.amount_minor,
        term_months: 1,
    };
    repository
        .settle_special_seeds(&owner, &snapshot, now_ms)
        .await
}
