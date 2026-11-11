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

/// What a client makes of the answer to `GET` [`SEEDS_PATH`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WalletReading {
    /// The service's balance.
    Read {
        /// The balance now.
        balance: Seeds,
    },
    /// Nobody is signed in: sign in to see a balance.
    AuthenticationRequired,
    /// This client is signed in but is not the active one.
    Inactive,
    /// No balance to show: never fall back to a number held on the device.
    Unavailable,
}

/// Reads the wallet answer: `ok` is whether the HTTP status was a success.
#[must_use]
pub fn read_seeds(ok: bool, body: &str) -> WalletReading {
    if !ok {
        // A failed read is never a balance, whatever its body says; only who
        // may ask again is read from it.
        return match crate::error::error_code(body).as_deref() {
            Some(crate::error::PROVIDER_AUTHENTICATION_REQUIRED) => {
                WalletReading::AuthenticationRequired
            }
            Some(crate::error::SESSION_INACTIVE) => WalletReading::Inactive,
            _ => WalletReading::Unavailable,
        };
    }
    serde_json::from_str::<SeedsWallet>(body).map_or(WalletReading::Unavailable, |wallet| {
        WalletReading::Read {
            balance: wallet.balance,
        }
    })
}

/// [`read_seeds`] as JSON, for a binding that passes strings:
/// `{"kind":"read","balance":"10000"}`, `{"kind":"authentication_required"}`,
/// `{"kind":"inactive"}` or `{"kind":"unavailable"}`.
#[must_use]
pub fn read_seeds_wire(ok: bool, body: &str) -> String {
    serde_json::to_string(&read_seeds(ok, body))
        .unwrap_or_else(|_| r#"{"kind":"unavailable"}"#.to_owned())
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
    fn a_client_reads_only_the_services_own_balance() {
        let fixture = include_str!("../fixtures/wallet/seeds.json");
        assert_eq!(
            read_seeds(true, fixture),
            WalletReading::Read {
                balance: Seeds(10_000)
            }
        );
        assert_eq!(
            read_seeds_wire(true, fixture),
            r#"{"kind":"read","balance":"10000"}"#
        );
        // A failed request is never read as a balance, whatever its body says.
        assert_eq!(read_seeds(false, fixture), WalletReading::Unavailable);
        assert_eq!(
            read_seeds_wire(
                false,
                r#"{"error":{"code":"provider_authentication_required","message":"x"}}"#
            ),
            r#"{"kind":"authentication_required"}"#
        );
        assert_eq!(
            read_seeds_wire(
                false,
                r#"{"error":{"code":"session_inactive","message":"x"}}"#
            ),
            r#"{"kind":"inactive"}"#
        );
        for body in [
            r#"{"error":{"code":"rate_limited"}}"#,
            r#"{"error":{"code":"taken"}}"#,
            "",
        ] {
            assert_eq!(
                read_seeds(false, body),
                WalletReading::Unavailable,
                "{body}"
            );
        }
        for body in [
            include_str!("../fixtures/wallet/rejected/seeds_client_authority.json"),
            include_str!("../fixtures/wallet/rejected/seeds_other_denomination.json"),
            include_str!("../fixtures/wallet/rejected/seeds_number_balance.json"),
            "",
            "not json",
            "[]",
        ] {
            assert_eq!(
                read_seeds_wire(true, body),
                r#"{"kind":"unavailable"}"#,
                "{body}"
            );
        }
        // A field this contract does not know is ignored, not a failure.
        let future = r#"{"balance":"7","denomination":"seeds","symbol":"x","authority":"identity","tier":2}"#;
        assert_eq!(
            read_seeds(true, future),
            WalletReading::Read { balance: Seeds(7) }
        );
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
