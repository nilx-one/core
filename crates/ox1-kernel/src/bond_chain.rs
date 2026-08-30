// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

use ox1_contracts::BondId;

/// Structural participant violation independent of an interaction contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidParticipants;

/// Terminal-state violation independent of an interaction contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalState;

/// Exact relationship between accepted local history and a candidate history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryRelation {
    Equal,
    FastForward { first_new_index: usize },
    Rollback,
    Divergence,
}

/// Validates the protocol invariant that a `BondChain` binds two distinct Bonds.
///
/// # Errors
///
/// Returns [`InvalidParticipants`] when both stable participant positions contain the
/// same Bond identifier.
pub fn validate_participants(
    bond_0_id: &BondId,
    bond_1_id: &BondId,
) -> Result<(), InvalidParticipants> {
    if bond_0_id == bond_1_id {
        Err(InvalidParticipants)
    } else {
        Ok(())
    }
}

/// Rejects semantic work after a chain has entered terminal state.
///
/// # Errors
///
/// Returns [`TerminalState`] when `is_terminal` is true.
pub const fn require_active(is_terminal: bool) -> Result<(), TerminalState> {
    if is_terminal {
        Err(TerminalState)
    } else {
        Ok(())
    }
}

/// Classifies candidate history under the exact-prefix-only rule.
#[must_use]
pub fn classify_history<T: Eq>(local: &[T], candidate: &[T]) -> HistoryRelation {
    let shared = local.len().min(candidate.len());
    if local[..shared] != candidate[..shared] {
        return HistoryRelation::Divergence;
    }

    match local.len().cmp(&candidate.len()) {
        core::cmp::Ordering::Equal => HistoryRelation::Equal,
        core::cmp::Ordering::Less => HistoryRelation::FastForward {
            first_new_index: local.len(),
        },
        core::cmp::Ordering::Greater => HistoryRelation::Rollback,
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryRelation, classify_history};

    #[test]
    fn classifies_exact_prefixes_only() {
        assert_eq!(classify_history(&[1, 2], &[1, 2]), HistoryRelation::Equal);
        assert_eq!(
            classify_history(&[1, 2], &[1, 2, 3]),
            HistoryRelation::FastForward { first_new_index: 2 }
        );
        assert_eq!(classify_history(&[1, 2], &[1]), HistoryRelation::Rollback);
        assert_eq!(
            classify_history(&[1, 2], &[1, 3]),
            HistoryRelation::Divergence
        );
    }
}
