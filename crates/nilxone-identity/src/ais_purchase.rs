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
struct Claims {
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
pub async fn settle(
    repository: &IdentityRepository,
    authority: &PurchaseAuthority<'_>,
    permit: &str,
    market_order: &IssuerOrder,
    now_ms: u64,
) -> Result<SpecialSeedsOutcome, PurchaseError> {
    let claims = verify(
        permit,
        authority.keys,
        authority.issuer,
        authority.product,
        now_ms / 1000,
    )?;
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
        order_id: wallet_order_key(&claims),
        authorization: SpendAuthorization::AisTelegram {
            issuer: claims.iss,
            ais_subject: claims.sub,
            telegram,
        },
        issuer: market.issuer.clone(),
        denomination: market.denomination.clone(),
        status: market.status.clone(),
        recipient_pub_dress: owner.as_str().to_owned(),
        sku: claims.sku,
        price_id: claims.price_id,
        amount_minor: claims.amount_minor,
        term_months: 1,
    };
    Ok(repository
        .settle_special_seeds(&owner, &snapshot, now_ms)
        .await?)
}
