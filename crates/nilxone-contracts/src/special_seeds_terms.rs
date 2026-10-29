// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Structural eligibility for issuer-owned Special purchases.
//! Authentication, quote verification and persistence remain authority concerns.

/// Validate already verified issuer terms without authorizing a debit.
/// Prices and durations come from the verified order, never a browser balance.
pub fn valid_special_seeds_terms(
    sku: &str,
    issuer: &str,
    denomination: &str,
    status: &str,
    price_id: u64,
    amount_minor: u64,
    duration_ms: u64,
) -> bool {
    matches!(
        sku,
        "0x1_special_1m" | "0x1_special_3m" | "0x1_special_6m" | "0x1_special_12m"
    ) && issuer == "0x1"
        && denomination == "seeds"
        && status == "processing"
        && price_id > 0
        && amount_minor > 0
        && duration_ms > 0
}

#[cfg(test)]
mod tests {
    use super::valid_special_seeds_terms as valid;

    #[test]
    fn only_special_processing_seed_orders_are_eligible() {
        for sku in ["0x1_special_1m", "0x1_special_3m", "0x1_special_6m", "0x1_special_12m"] {
            assert!(valid(sku, "0x1", "seeds", "processing", 1, 640, 1));
        }
        for sku in ["0x1_exclusive_1m", "0x1_special_2m", "", "0x1_special_1m "] {
            assert!(!valid(sku, "0x1", "seeds", "processing", 1, 640, 1));
        }
        assert!(!valid("0x1_special_1m", "other", "seeds", "processing", 1, 640, 1));
        assert!(!valid("0x1_special_1m", "0x1", "USDT", "processing", 1, 640, 1));
        assert!(!valid("0x1_special_1m", "0x1", "seeds", "pending", 1, 640, 1));
        for (price, amount, duration) in [(0, 640, 1), (1, 0, 1), (1, 640, 0)] {
            assert!(!valid("0x1_special_1m", "0x1", "seeds", "processing", price, amount, duration));
        }
    }
}
