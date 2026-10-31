// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! A Bond's visit to 0xda-market and its return, as Identity decides them.
//!
//! A visit binds an opaque reference to the Bond and the session that opened
//! it, for a limited time to spend in. The tab comes back to a 0x1 host page
//! naming the visit and an order; that navigation is never payment proof. A
//! return is confirmed only when Identity's committed receipt and Market's
//! authenticated fulfilment agree with each other and with the visit.
//!
//! HTTP routes, sessions and the Market wire belong to the adapters.

use nilxone_contracts::special_sku_months;

use crate::{
    market_issuer::{ISSUER, IssuerOrder, TERM_POLICY, VISIT_PROVIDER},
    repository::{MarketVisit, SpecialSeedsReceipt},
};

/// How long a visit may start a new debit: Market's handoff (1 minute),
/// its session (30 minutes) and an order's authorization (15 minutes) fit.
pub const MARKET_VISIT_SPEND_WINDOW_MS: u64 = 60 * 60 * 1000;

/// A visit reference is 32 random bytes, unpadded base64url.
const VISIT_ID_BYTES: usize = 32;
const VISIT_ID_LEN: usize = 43;
const ORDER_ID_MAX: usize = 128;

/// A fresh, unguessable visit reference.
///
/// # Errors
/// When the platform has no randomness to give.
pub fn new_market_visit_id() -> Result<String, getrandom::Error> {
    let mut bytes = [0_u8; VISIT_ID_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        bytes,
    ))
}

/// Whether `value` has the shape of a visit reference.
#[must_use]
pub fn is_market_visit_id(value: &str) -> bool {
    value.len() == VISIT_ID_LEN
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// Whether `value` has the shape of a Market order id.
#[must_use]
pub fn is_market_order_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= ORDER_ID_MAX
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
}

/// Whether `path` is a 0x1 host page a visit may return to: `/` or one
/// lowercase segment such as `/telegram/`. Never a URL.
#[must_use]
pub fn is_market_return_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    rest.is_empty()
        || rest.strip_suffix('/').is_some_and(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}

/// The visit and order a returning tab names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarketReturn<'a> {
    pub visit_id: &'a str,
    pub order_id: &'a str,
}

/// Whether a return is a confirmed Special purchase: Market fulfilled this
/// order for this visit's recipient, and Identity committed the matching debit
/// for the visit's Bond. It holds after the visit expires and after a rename.
#[must_use]
pub fn market_return_confirmed(
    claim: MarketReturn<'_>,
    visit: &MarketVisit,
    receipt: &SpecialSeedsReceipt,
    order: &IssuerOrder,
) -> bool {
    let a = &order.attributes;
    // The SKU fixes the term. The receipt's duration is what Core's calendar
    // months came to on the clock at the grant, so it is not recomputed from
    // the order: a stacked purchase starts later.
    let term = special_sku_months(&a.sku).is_some_and(|months| {
        a.term
            .as_ref()
            .is_some_and(|term| term.policy == TERM_POLICY && term.months == months)
    });
    order.id == claim.order_id
        && a.status == "fulfilled"
        && a.issuer == ISSUER
        && a.denomination == "seeds"
        && a.link_provider == VISIT_PROVIDER
        && a.link_provider_user_id == claim.visit_id
        && a.recipient_subject == visit.recipient_pub_dress
        && receipt.owner_pub_dress == visit.owner.as_str()
        && receipt.sku == a.sku
        && receipt.price_id == a.price_id
        && receipt.amount_minor == a.amount_minor
        && term
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market_issuer::{IssuerOrderAttributes, IssuerOrderTerm};

    fn fixture() -> (String, MarketVisit, SpecialSeedsReceipt, IssuerOrder) {
        let visit_id = "v".repeat(43);
        let visit = MarketVisit {
            owner: "0x0sky".parse().unwrap(),
            recipient_pub_dress: "0x0old".into(),
            session_token_hash: vec![1; 32],
            spend_until_ms: 1,
        };
        let receipt = SpecialSeedsReceipt {
            owner_pub_dress: "0x0sky".into(),
            sku: "0x1_special_1m".into(),
            price_id: 7,
            amount_minor: 640,
            duration_ms: 31 * 86_400_000,
        };
        let order = IssuerOrder {
            id: "order-1234".into(),
            attributes: IssuerOrderAttributes {
                sku: receipt.sku.clone(),
                denomination: "seeds".into(),
                issuer: "0x1".into(),
                amount_minor: 640,
                price_id: 7,
                recipient_subject: "0x0old".into(),
                link_provider: "nilx_visit".into(),
                link_provider_user_id: visit_id.clone(),
                status: "fulfilled".into(),
                created_at: "2026-10-29T12:00:00Z".into(),
                term: Some(IssuerOrderTerm {
                    months: 1,
                    policy: "calendar_months_utc".into(),
                }),
            },
        };
        (visit_id, visit, receipt, order)
    }

    #[test]
    fn completion_requires_matching_owner_visit_receipt_and_fulfilment() {
        let (visit_id, visit, receipt, order) = fixture();
        let claim = MarketReturn {
            visit_id: &visit_id,
            order_id: "order-1234",
        };
        // Completed purchases survive rename and visit/session expiry.
        assert!(market_return_confirmed(claim, &visit, &receipt, &order));
        let mutations: [fn(&mut IssuerOrder); 11] = [
            |o| o.attributes.status = "processing".into(),
            |o| o.attributes.status = "cancelled".into(),
            |o| o.id = "other-order".into(),
            |o| o.attributes.link_provider_user_id = "w".repeat(43),
            |o| o.attributes.link_provider = "telegram".into(),
            |o| o.attributes.recipient_subject = "0x0other".into(),
            |o| o.attributes.amount_minor = 1,
            |o| o.attributes.price_id = 8,
            |o| o.attributes.term = None,
            |o| o.attributes.term.as_mut().unwrap().months = 3,
            |o| o.attributes.sku = "0x1_special_3m".into(),
        ];
        for mutate in mutations {
            let mut bad = order.clone();
            mutate(&mut bad);
            assert!(!market_return_confirmed(claim, &visit, &receipt, &bad));
        }
        let mut other = receipt.clone();
        other.owner_pub_dress = "0x0other".into();
        assert!(!market_return_confirmed(claim, &visit, &other, &order));
        let wrong = MarketReturn {
            visit_id: &"w".repeat(43),
            order_id: "order-1234",
        };
        assert!(!market_return_confirmed(wrong, &visit, &receipt, &order));
    }

    #[test]
    fn return_path_is_a_host_page_never_a_url() {
        for path in ["/", "/telegram/", "/discord-activity/", "/x1/"] {
            assert!(is_market_return_path(path), "{path}");
        }
        for path in [
            "",
            "telegram/",
            "/telegram",
            "//evil.example/",
            "/a/b/",
            "/Telegram/",
            "/te_st/",
            "https://evil.example/",
            "/%2e%2e/",
        ] {
            assert!(!is_market_return_path(path), "{path}");
        }
    }

    #[test]
    fn visit_and_order_references_have_their_wire_shape() {
        let fresh = new_market_visit_id().unwrap();
        assert!(is_market_visit_id(&fresh), "{fresh}");
        assert_ne!(fresh, new_market_visit_id().unwrap());
        for bad in [
            "",
            "short",
            &"v".repeat(44),
            &format!("{}=", "v".repeat(42)),
        ] {
            assert!(!is_market_visit_id(bad), "{bad}");
        }
        for good in ["o1", "order-1234", "a.b_c:d-e", &"o".repeat(128)] {
            assert!(is_market_order_id(good), "{good}");
        }
        for bad in ["", "o/1", "o 1", &"o".repeat(129)] {
            assert!(!is_market_order_id(bad), "{bad}");
        }
    }
}
