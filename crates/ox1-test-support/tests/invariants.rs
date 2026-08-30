// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use ox1_kernel::bond_chain::{HistoryRelation, classify_history};
use proptest::prelude::*;

proptest! {
    #[test]
    fn strict_extension_is_the_only_fast_forward(
        prefix in proptest::collection::vec(any::<u16>(), 0..64),
        suffix in proptest::collection::vec(any::<u16>(), 1..64),
    ) {
        let mut candidate = prefix.clone();
        candidate.extend(suffix);
        prop_assert_eq!(
            classify_history(&prefix, &candidate),
            HistoryRelation::FastForward { first_new_index: prefix.len() }
        );
    }

    #[test]
    fn rewrite_inside_shared_prefix_never_fast_forwards(
        prefix in proptest::collection::vec(any::<u16>(), 1..64),
        replacement in any::<u16>(),
    ) {
        let mut candidate = prefix.clone();
        let original = candidate[0];
        candidate[0] = replacement;
        if replacement != original {
            prop_assert_eq!(classify_history(&prefix, &candidate), HistoryRelation::Divergence);
        }
    }
}
