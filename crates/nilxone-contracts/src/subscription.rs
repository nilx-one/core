// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Shared subscription status, independent of any rendering surface.
//! The authority adapter supplies the snapshot; this resolver verifies neither
//! payment nor provenance and must never be used as a client-side access grant.

use serde::{Deserialize, Serialize};

use crate::{DecimalU64, PubDress};

/// Paid subscription alternatives, never cumulative tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionTier {
    /// Special subscription.
    Special,
    /// Exclusive includes Special capabilities.
    Exclusive,
}

/// One effective period selected by the authority, not a payment request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionEntitlement {
    /// The effective tier.
    pub tier: SubscriptionTier,
    /// Inclusive start, Unix milliseconds.
    pub starts_at_ms: DecimalU64,
    /// Exclusive end, Unix milliseconds.
    pub expires_at_ms: DecimalU64,
}

/// A bounded-freshness read from the identity service. Never local preferences.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionSnapshot {
    /// The authenticated Bond whose entitlement was read, matched exactly.
    pub bond_pub_dress: String,
    /// When the authority checked the entitlement.
    pub checked_at_ms: DecimalU64,
    /// Exclusive freshness limit, chosen by the authority.
    pub refresh_after_ms: DecimalU64,
    /// `None` is a confirmed absence/revocation, not a network failure.
    #[serde(deserialize_with = "required_entitlement")]
    pub entitlement: Option<SubscriptionEntitlement>,
}

fn required_entitlement<'de, D>(
    deserializer: D,
) -> Result<Option<SubscriptionEntitlement>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::deserialize(deserializer)
}

/// The single status all clients and surfaces consume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubscriptionStatus {
    /// Missing, malformed, stale, future-dated or wrong-owner evidence.
    Unknown,
    /// Confirmed absence of an effective entitlement; not an upsell command.
    None {
        /// Reevaluate at this exclusive deadline.
        valid_until_ms: DecimalU64,
    },
    /// Effective entitlement for this Bond and its currently associated Avaia.
    Active {
        /// Explicit owner binding; Avaia does not own a separate subscription.
        bond_pub_dress: String,
        /// The single effective tier.
        tier: SubscriptionTier,
        /// Reevaluate at expiry or the earlier freshness deadline.
        valid_until_ms: DecimalU64,
    },
}

/// Resolves status deterministically against an explicitly supplied clock.
#[must_use]
pub fn subscription_status(
    snapshot: Option<&SubscriptionSnapshot>,
    owner: &str,
    now_ms: u64,
) -> SubscriptionStatus {
    let Some(snapshot) = snapshot else {
        return SubscriptionStatus::Unknown;
    };
    if owner.parse::<PubDress>().is_err()
        || snapshot.bond_pub_dress != owner
        || snapshot.checked_at_ms.get() > now_ms
        || snapshot.refresh_after_ms.get() <= now_ms
        || snapshot.refresh_after_ms <= snapshot.checked_at_ms
    {
        return SubscriptionStatus::Unknown;
    }
    let none = |until| SubscriptionStatus::None {
        valid_until_ms: until,
    };
    let Some(entitlement) = &snapshot.entitlement else {
        return none(snapshot.refresh_after_ms);
    };
    if entitlement.starts_at_ms >= entitlement.expires_at_ms {
        return SubscriptionStatus::Unknown;
    }
    if now_ms < entitlement.starts_at_ms.get() {
        return none(snapshot.refresh_after_ms.min(entitlement.starts_at_ms));
    }
    if now_ms >= entitlement.expires_at_ms.get() {
        return none(snapshot.refresh_after_ms);
    }
    SubscriptionStatus::Active {
        bond_pub_dress: owner.to_owned(),
        tier: entitlement.tier,
        valid_until_ms: snapshot.refresh_after_ms.min(entitlement.expires_at_ms),
    }
}

/// Version-one wire shared by Wasm and `UniFFI`. Invalid input fails to unknown.
/// All times are canonical decimal strings, preserving u64 across runtimes.
#[must_use]
pub fn subscription_status_wire(snapshot: &str, owner: &str, now_ms: &str) -> String {
    let status = match (
        serde_json::from_str::<Option<SubscriptionSnapshot>>(snapshot),
        now_ms.parse::<DecimalU64>(),
    ) {
        (Ok(snapshot), Ok(now)) => subscription_status(snapshot.as_ref(), owner, now.get()),
        _ => SubscriptionStatus::Unknown,
    };
    serde_json::to_string(&status).unwrap_or_else(|_| r#"{"kind":"unknown"}"#.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> SubscriptionSnapshot {
        SubscriptionSnapshot {
            bond_pub_dress: "0x0sky".to_owned(),
            checked_at_ms: DecimalU64::new(100),
            refresh_after_ms: DecimalU64::new(400),
            entitlement: Some(SubscriptionEntitlement {
                tier: SubscriptionTier::Exclusive,
                starts_at_ms: DecimalU64::new(150),
                expires_at_ms: DecimalU64::new(300),
            }),
        }
    }

    #[test]
    fn inclusive_start_exclusive_expiry_and_freshness() {
        let snapshot = snapshot();
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", 149),
            SubscriptionStatus::None {
                valid_until_ms: DecimalU64::new(150)
            }
        );
        assert!(
            matches!(subscription_status(Some(&snapshot), "0x0sky", 150),
            SubscriptionStatus::Active { tier: SubscriptionTier::Exclusive, valid_until_ms, .. }
            if valid_until_ms.get() == 300)
        );
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", 300),
            SubscriptionStatus::None {
                valid_until_ms: DecimalU64::new(400)
            }
        );
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", 400),
            SubscriptionStatus::Unknown
        );
    }

    #[test]
    fn unknown_never_becomes_confirmed_absence() {
        let mut snapshot = snapshot();
        assert_eq!(
            subscription_status(None, "0x0sky", 200),
            SubscriptionStatus::Unknown
        );
        for (owner, now) in [("0x0Sky", 200), ("0x1other", 200), ("0x0sky", 99)] {
            assert_eq!(
                subscription_status(Some(&snapshot), owner, now),
                SubscriptionStatus::Unknown
            );
        }
        snapshot.entitlement.as_mut().unwrap().expires_at_ms = DecimalU64::new(150);
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", 200),
            SubscriptionStatus::Unknown
        );
    }

    #[test]
    fn downgrade_revocation_and_earlier_refresh() {
        let mut snapshot = snapshot();
        snapshot.refresh_after_ms = DecimalU64::new(250);
        snapshot.entitlement.as_mut().unwrap().tier = SubscriptionTier::Special;
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", 200),
            SubscriptionStatus::Active {
                bond_pub_dress: "0x0sky".to_owned(),
                tier: SubscriptionTier::Special,
                valid_until_ms: DecimalU64::new(250),
            }
        );
        snapshot.entitlement = None;
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", 200),
            SubscriptionStatus::None {
                valid_until_ms: DecimalU64::new(250)
            }
        );
    }

    #[test]
    fn wire_is_strict_and_preserves_u64() {
        let wire = serde_json::to_string(&snapshot()).unwrap();
        let mut missing = serde_json::to_value(snapshot()).unwrap();
        missing.as_object_mut().unwrap().remove("entitlement");
        assert_eq!(
            subscription_status_wire(&missing.to_string(), "0x0sky", "200"),
            r#"{"kind":"unknown"}"#
        );
        assert!(subscription_status_wire(&wire, "0x0sky", "200").contains("exclusive"));
        for invalid in [
            "",
            "{}",
            "null",
            &wire.replace("exclusive", "legendary"),
            &wire.replace("\"100\"", "100"),
            &wire.replace("\"100\"", "\"0100\""),
        ] {
            assert_eq!(
                subscription_status_wire(invalid, "0x0sky", "200"),
                r#"{"kind":"unknown"}"#
            );
        }
        assert_eq!(
            subscription_status_wire(&wire, "0x0sky", "0200"),
            r#"{"kind":"unknown"}"#
        );
        let mut snapshot = snapshot();
        snapshot.checked_at_ms = DecimalU64::new(u64::MAX - 2);
        snapshot.refresh_after_ms = DecimalU64::new(u64::MAX);
        snapshot.entitlement = None;
        assert_eq!(
            subscription_status(Some(&snapshot), "0x0sky", u64::MAX - 1),
            SubscriptionStatus::None {
                valid_until_ms: DecimalU64::new(u64::MAX)
            }
        );
    }
}
