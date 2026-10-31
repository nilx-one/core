// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Structural eligibility for issuer-owned Special purchases.
//! Authentication, quote verification and persistence remain authority concerns.

/// A Special purchase paid in seeds, as the issuer's verified order states it.
///
/// Named fields, so a caller cannot silently swap the issuer and the
/// denomination, or the price id and the amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialSeedsOrder<'a> {
    pub sku: &'a str,
    pub issuer: &'a str,
    pub denomination: &'a str,
    pub status: &'a str,
    pub price_id: u64,
    pub amount_minor: u64,
}

/// The calendar months a Special SKU buys, or `None` for any other SKU. The
/// SKU alone fixes the term, whatever state its order is in.
#[must_use]
pub fn special_sku_months(sku: &str) -> Option<u32> {
    match sku {
        "0x1_special_1m" => Some(1),
        "0x1_special_3m" => Some(3),
        "0x1_special_6m" => Some(6),
        "0x1_special_12m" => Some(12),
        _ => None,
    }
}

/// The calendar-month term a structurally eligible Special seeds order buys,
/// for [`SubscriptionClock::grant_special_months`], or `None` for any other
/// order. This validates structure only and never authorizes a debit: prices
/// come from the verified order, never a browser balance.
///
/// [`SubscriptionClock::grant_special_months`]: crate::SubscriptionClock::grant_special_months
#[must_use]
pub fn special_seeds_term_months(order: &SpecialSeedsOrder<'_>) -> Option<u32> {
    let months = special_sku_months(order.sku)?;
    let eligible = order.issuer == "0x1"
        && order.denomination == "seeds"
        && order.status == "processing"
        && order.price_id > 0
        && order.amount_minor > 0;
    eligible.then_some(months)
}

#[cfg(test)]
mod tests {
    use super::{SpecialSeedsOrder, special_seeds_term_months as term, special_sku_months};
    use crate::SubscriptionClock;

    const ORDER: SpecialSeedsOrder<'static> = SpecialSeedsOrder {
        sku: "0x1_special_1m",
        issuer: "0x1",
        denomination: "seeds",
        status: "processing",
        price_id: 1,
        amount_minor: 640,
    };

    #[test]
    fn each_special_sku_buys_its_calendar_term() {
        for (sku, months) in [
            ("0x1_special_1m", 1),
            ("0x1_special_3m", 3),
            ("0x1_special_6m", 6),
            ("0x1_special_12m", 12),
        ] {
            let order = SpecialSeedsOrder { sku, ..ORDER };
            assert_eq!(term(&order), Some(months), "{sku}");
            // The SKU fixes the term whatever the order's state.
            assert_eq!(special_sku_months(sku), Some(months), "{sku}");
            // Every term this accepts is one the clock grants.
            let clock = SubscriptionClock::new("0x0sky", 1).unwrap();
            assert!(clock.grant_special_months(months, 1).is_ok(), "{sku}");
        }
        for sku in ["0x1_exclusive_1m", "0x1_special_2m", "", "0x1_special_1M"] {
            assert_eq!(special_sku_months(sku), None, "{sku}");
        }
    }

    #[test]
    fn only_special_processing_seed_orders_are_eligible() {
        for sku in ["0x1_exclusive_1m", "0x1_special_2m", "", "0x1_special_1m "] {
            assert_eq!(term(&SpecialSeedsOrder { sku, ..ORDER }), None, "{sku:?}");
        }
        for order in [
            SpecialSeedsOrder {
                issuer: "other",
                ..ORDER
            },
            SpecialSeedsOrder {
                denomination: "USDT",
                ..ORDER
            },
            SpecialSeedsOrder {
                status: "pending",
                ..ORDER
            },
            SpecialSeedsOrder {
                price_id: 0,
                ..ORDER
            },
            SpecialSeedsOrder {
                amount_minor: 0,
                ..ORDER
            },
        ] {
            assert_eq!(term(&order), None, "{order:?}");
        }
    }
}
