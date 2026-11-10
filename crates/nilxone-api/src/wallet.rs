// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! The seeds wallet: one read, no mutation. A balance is a wallet fact held by
//! the service, never a number a client supplies or carries.

use serde::{Deserialize, Serialize};

use crate::value::Seeds;

/// `GET`: the signed-in Bond's balance.
pub const SEEDS_PATH: &str = "/api/v1/identity/seeds";
/// How the wallet's unit is written for a person.
pub const SEEDS_SYMBOL: &str = "₴€£";

/// The wallet's unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Denomination {
    /// Seeds, the 0x1 currency.
    Seeds,
}

/// Who vouches for the balance.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WalletAuthority {
    /// The service's own ledger. A client shows no balance from anyone else.
    Identity,
}

/// The answer to `GET` [`SEEDS_PATH`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedsWallet {
    /// The balance now.
    pub balance: Seeds,
    /// Its unit.
    pub denomination: Denomination,
    /// The unit as a person reads it: [`SEEDS_SYMBOL`].
    pub symbol: String,
    /// Who vouches for it.
    pub authority: WalletAuthority,
}

impl SeedsWallet {
    /// The service's answer for `balance`.
    #[must_use]
    pub fn of(balance: u64) -> Self {
        Self {
            balance: Seeds(balance),
            denomination: Denomination::Seeds,
            symbol: SEEDS_SYMBOL.to_owned(),
            authority: WalletAuthority::Identity,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn the_wallet_fixture_round_trips() {
        let fixture = include_str!("../fixtures/wallet/seeds.json");
        let wallet: SeedsWallet = serde_json::from_str(fixture).expect("parses");
        assert_eq!(wallet, SeedsWallet::of(10_000));
        let expected: Value = serde_json::from_str(fixture).expect("json");
        assert_eq!(serde_json::to_value(&wallet).expect("serializes"), expected);
    }

    #[test]
    fn a_balance_from_another_authority_or_unit_is_rejected() {
        for fixture in [
            include_str!("../fixtures/wallet/rejected/seeds_client_authority.json"),
            include_str!("../fixtures/wallet/rejected/seeds_other_denomination.json"),
            include_str!("../fixtures/wallet/rejected/seeds_number_balance.json"),
        ] {
            assert!(
                serde_json::from_str::<SeedsWallet>(fixture).is_err(),
                "accepted: {fixture}"
            );
        }
    }
}
