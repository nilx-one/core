// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MPL-2.0

//! Authority-side duration accounting. This is not a client grant API.

use serde::{Deserialize, Serialize};

use crate::{
    DecimalU64, PubDress, SubscriptionEntitlement, SubscriptionSnapshot, SubscriptionTier,
};

/// Persisted remaining durations at one authority-clock instant.
/// Exclusive consumes time first; Special is paused until Exclusive is exhausted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionClock {
    /// Exact authenticated Bond owner, never an Avaia identity.
    pub bond_pub_dress: String,
    /// Last instant at which these remaining durations were settled.
    pub settled_at_ms: DecimalU64,
    /// Unconsumed Special time, including time paused behind Exclusive.
    pub special_remaining_ms: DecimalU64,
    /// Unconsumed Exclusive time; zero permits Special to run.
    pub exclusive_remaining_ms: DecimalU64,
}

/// Invalid authority input leaves the original clock unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionClockError {
    /// Supported paid terms are one, three, six or twelve calendar months.
    InvalidTerm,
    /// Not a valid Bond identity.
    InvalidOwner,
    /// Authority timestamps must not move backwards.
    ClockRollback,
    /// Grants must contain a positive duration.
    EmptyGrant,
    /// The combined periods cannot be represented in u64 milliseconds.
    Overflow,
    /// Snapshot freshness must extend strictly past its observation.
    InvalidFreshness,
}

impl SubscriptionClock {
    /// Extend Special by calendar months in UTC, keeping the time of day and
    /// clamping the day to the last valid day of the destination month.
    /// The anchor is the projected end of existing Special after Exclusive.
    /// Later Exclusive grants still pause the resulting remaining duration.
    ///
    /// # Errors
    /// Rejects unsupported terms, invalid clocks and unrepresentable dates.
    pub fn grant_special_months(
        &self,
        months: u32,
        now_ms: u64,
    ) -> Result<Self, SubscriptionClockError> {
        if !matches!(months, 1 | 3 | 6 | 12) {
            return Err(SubscriptionClockError::InvalidTerm);
        }
        let current = self.advance(now_ms)?;
        let anchor = now_ms
            .checked_add(current.exclusive_remaining_ms.get())
            .and_then(|end| end.checked_add(current.special_remaining_ms.get()))
            .ok_or(SubscriptionClockError::Overflow)?;
        let timestamp = i64::try_from(anchor).map_err(|_| SubscriptionClockError::Overflow)?;
        let end = chrono::DateTime::from_timestamp_millis(timestamp)
            .and_then(|date| date.checked_add_months(chrono::Months::new(months)))
            .and_then(|date| u64::try_from(date.timestamp_millis()).ok())
            .ok_or(SubscriptionClockError::Overflow)?;
        current.grant(SubscriptionTier::Special, end - anchor, now_ms)
    }

    /// Whether a new Special purchase may be opened now: only while the Bond
    /// holds no Special time at all, running or paused behind Exclusive. A
    /// Bond that has Special cannot buy it again until it ends; purchases
    /// never stack on an existing Special.
    ///
    /// # Errors
    /// Rejects invalid state and clock rollback.
    pub fn special_purchase_open(&self, now_ms: u64) -> Result<bool, SubscriptionClockError> {
        Ok(self.advance(now_ms)?.special_remaining_ms.get() == 0)
    }

    /// An empty clock for a known Bond. Only a trusted authority may grant time.
    ///
    /// # Errors
    /// Returns `InvalidOwner` for an invalid Bond address.
    pub fn new(owner: &str, now_ms: u64) -> Result<Self, SubscriptionClockError> {
        if owner.parse::<PubDress>().is_err() {
            return Err(SubscriptionClockError::InvalidOwner);
        }
        Ok(Self {
            bond_pub_dress: owner.to_owned(),
            settled_at_ms: DecimalU64::new(now_ms),
            special_remaining_ms: DecimalU64::new(0),
            exclusive_remaining_ms: DecimalU64::new(0),
        })
    }

    fn validate(&self) -> Result<(), SubscriptionClockError> {
        if self.bond_pub_dress.parse::<PubDress>().is_err() {
            return Err(SubscriptionClockError::InvalidOwner);
        }
        self.settled_at_ms
            .get()
            .checked_add(self.exclusive_remaining_ms.get())
            .and_then(|end| end.checked_add(self.special_remaining_ms.get()))
            .ok_or(SubscriptionClockError::Overflow)?;
        Ok(())
    }

    /// Settle elapsed time, even across both expiry boundaries without a timer.
    /// Repeated settlement at the same timestamp is idempotent.
    ///
    /// # Errors
    /// Rejects invalid persisted state and clock rollback.
    pub fn advance(&self, now_ms: u64) -> Result<Self, SubscriptionClockError> {
        self.validate()?;
        let elapsed = now_ms
            .checked_sub(self.settled_at_ms.get())
            .ok_or(SubscriptionClockError::ClockRollback)?;
        let exclusive = self.exclusive_remaining_ms.get();
        let special_elapsed = elapsed.saturating_sub(exclusive);
        Ok(Self {
            bond_pub_dress: self.bond_pub_dress.clone(),
            settled_at_ms: DecimalU64::new(now_ms),
            exclusive_remaining_ms: DecimalU64::new(exclusive.saturating_sub(elapsed)),
            special_remaining_ms: DecimalU64::new(
                self.special_remaining_ms
                    .get()
                    .saturating_sub(special_elapsed),
            ),
        })
    }

    /// Add authorized time after settling the existing clock. Special granted
    /// during Exclusive waits; extending Exclusive preserves all paused Special.
    /// The authority must authenticate, deduplicate grants and persist atomically.
    ///
    /// # Errors
    /// Rejects zero durations, invalid state, rollback and arithmetic overflow.
    pub fn grant(
        &self,
        tier: SubscriptionTier,
        duration_ms: u64,
        now_ms: u64,
    ) -> Result<Self, SubscriptionClockError> {
        if duration_ms == 0 {
            return Err(SubscriptionClockError::EmptyGrant);
        }
        let mut next = self.advance(now_ms)?;
        let remaining = match tier {
            SubscriptionTier::Special => &mut next.special_remaining_ms,
            SubscriptionTier::Exclusive => &mut next.exclusive_remaining_ms,
        };
        *remaining = DecimalU64::new(
            remaining
                .get()
                .checked_add(duration_ms)
                .ok_or(SubscriptionClockError::Overflow)?,
        );
        next.validate()?;
        Ok(next)
    }

    /// Project one effective period into the existing cross-client status wire.
    /// At Exclusive expiry the next snapshot exposes resumed Special immediately.
    /// This read is pure; persistence and freshness policy belong to the service.
    ///
    /// # Errors
    /// Rejects invalid state, clock rollback or a non-future freshness deadline.
    pub fn snapshot(
        &self,
        now_ms: u64,
        refresh_after_ms: u64,
    ) -> Result<SubscriptionSnapshot, SubscriptionClockError> {
        if refresh_after_ms <= now_ms {
            return Err(SubscriptionClockError::InvalidFreshness);
        }
        let settled = self.advance(now_ms)?;
        let (tier, remaining) = if settled.exclusive_remaining_ms.get() > 0 {
            (
                SubscriptionTier::Exclusive,
                settled.exclusive_remaining_ms.get(),
            )
        } else {
            (
                SubscriptionTier::Special,
                settled.special_remaining_ms.get(),
            )
        };
        Ok(SubscriptionSnapshot {
            bond_pub_dress: settled.bond_pub_dress,
            checked_at_ms: DecimalU64::new(now_ms),
            refresh_after_ms: DecimalU64::new(refresh_after_ms),
            entitlement: if remaining == 0 {
                None
            } else {
                Some(SubscriptionEntitlement {
                    tier,
                    starts_at_ms: DecimalU64::new(now_ms),
                    expires_at_ms: DecimalU64::new(
                        now_ms
                            .checked_add(remaining)
                            .ok_or(SubscriptionClockError::Overflow)?,
                    ),
                })
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SubscriptionStatus, subscription_status};

    fn utc(value: &str) -> u64 {
        u64::try_from(
            value
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
                .timestamp_millis(),
        )
        .unwrap()
    }

    #[test]
    fn special_can_be_bought_only_while_the_bond_holds_none() {
        let empty = SubscriptionClock::new("0x0sky", 0).unwrap();
        assert_eq!(empty.special_purchase_open(0), Ok(true));

        let special = empty.grant(SubscriptionTier::Special, 10, 0).unwrap();
        assert_eq!(special.special_purchase_open(9), Ok(false));
        assert_eq!(special.special_purchase_open(10), Ok(true));

        // Special paused behind Exclusive is still held.
        let paused = special.grant(SubscriptionTier::Exclusive, 100, 0).unwrap();
        assert_eq!(paused.special_purchase_open(50), Ok(false));
        assert_eq!(paused.special_purchase_open(110), Ok(true));

        // Exclusive alone does not hold Special.
        let exclusive = empty.grant(SubscriptionTier::Exclusive, 100, 0).unwrap();
        assert_eq!(exclusive.special_purchase_open(1), Ok(true));
        assert_eq!(
            special
                .special_purchase_open(0)
                .and(special.advance(5))
                .and_then(|c| c.special_purchase_open(1)),
            Err(SubscriptionClockError::ClockRollback)
        );
    }

    #[test]
    fn calendar_terms_preserve_time_and_clamp_end_of_month() {
        for (start, months, end) in [
            ("2026-01-31T12:34:56.789Z", 1, "2026-02-28T12:34:56.789Z"),
            ("2028-01-31T12:34:56.789Z", 1, "2028-02-29T12:34:56.789Z"),
            ("2028-02-29T12:34:56.789Z", 12, "2029-02-28T12:34:56.789Z"),
            ("2026-08-31T12:34:56.789Z", 3, "2026-11-30T12:34:56.789Z"),
            ("2026-08-31T12:34:56.789Z", 6, "2027-02-28T12:34:56.789Z"),
        ] {
            let start = utc(start);
            let clock = SubscriptionClock::new("0x0sky", start)
                .unwrap()
                .grant_special_months(months, start)
                .unwrap();
            assert_eq!(
                clock
                    .snapshot(start, start + 1)
                    .unwrap()
                    .entitlement
                    .unwrap()
                    .expires_at_ms
                    .get(),
                utc(end)
            );
        }
    }

    #[test]
    fn calendar_extension_uses_existing_end_after_exclusive_and_stays_paused() {
        let start = utc("2026-01-01T00:00:00Z");
        let feb = utc("2026-02-01T00:00:00Z");
        let march = utc("2026-03-01T00:00:00Z");
        let clock = SubscriptionClock::new("0x0sky", start)
            .unwrap()
            .grant_special_months(1, start)
            .unwrap()
            .grant_special_months(1, start + 1)
            .unwrap();
        assert_eq!(
            clock
                .snapshot(start + 1, march)
                .unwrap()
                .entitlement
                .unwrap()
                .expires_at_ms
                .get(),
            march
        );
        let paused = SubscriptionClock::new("0x0sky", start)
            .unwrap()
            .grant(SubscriptionTier::Exclusive, feb - start, start)
            .unwrap()
            .grant_special_months(1, start)
            .unwrap();
        assert_eq!(paused.special_remaining_ms.get(), march - feb);
        assert_eq!(
            paused.advance(feb - 1).unwrap().special_remaining_ms,
            paused.special_remaining_ms
        );
        assert_eq!(
            paused
                .snapshot(feb, march + 1)
                .unwrap()
                .entitlement
                .unwrap()
                .expires_at_ms
                .get(),
            march
        );
        assert!(paused.grant_special_months(2, start).is_err());
        assert!(paused.grant_special_months(1, start - 1).is_err());
        assert!(
            SubscriptionClock::new("0x0sky", u64::MAX)
                .unwrap()
                .grant_special_months(1, u64::MAX)
                .is_err()
        );
    }

    fn upgraded() -> SubscriptionClock {
        SubscriptionClock::new("0x0sky", 100)
            .unwrap()
            .grant(SubscriptionTier::Special, 1_000, 100)
            .unwrap()
            .grant(SubscriptionTier::Exclusive, 500, 300)
            .unwrap()
    }

    #[test]
    fn exclusive_pauses_special_and_resumes_exactly_at_expiry() {
        let clock = upgraded();
        assert_eq!(clock.special_remaining_ms.get(), 800);
        assert_eq!(clock.advance(799).unwrap().special_remaining_ms.get(), 800);
        let before = clock.snapshot(799, 2_000).unwrap();
        assert!(matches!(subscription_status(Some(&before), "0x0sky", 799),
            SubscriptionStatus::Active { tier: SubscriptionTier::Exclusive, valid_until_ms, .. }
            if valid_until_ms.get() == 800));
        let resumed = clock.snapshot(800, 2_000).unwrap();
        assert!(matches!(subscription_status(Some(&resumed), "0x0sky", 800),
            SubscriptionStatus::Active { tier: SubscriptionTier::Special, valid_until_ms, .. }
            if valid_until_ms.get() == 1_600));
        assert!(clock.snapshot(1_600, 2_000).unwrap().entitlement.is_none());
        // Reads do not consume stored balances or depend on a running client.
        assert_eq!(clock.special_remaining_ms.get(), 800);
    }

    #[test]
    fn extensions_and_special_grants_during_exclusive_preserve_paused_time() {
        let clock = upgraded()
            .grant(SubscriptionTier::Exclusive, 200, 400)
            .unwrap()
            .grant(SubscriptionTier::Special, 100, 500)
            .unwrap();
        assert_eq!(clock.special_remaining_ms.get(), 900);
        assert_eq!(clock.exclusive_remaining_ms.get(), 500);
        let resumed = clock.snapshot(1_000, 3_000).unwrap().entitlement.unwrap();
        assert_eq!(resumed.tier, SubscriptionTier::Special);
        assert_eq!(resumed.expires_at_ms.get(), 1_900);
    }

    #[test]
    fn elapsed_time_is_independent_of_settlement_frequency_and_serialization() {
        let clock = upgraded();
        let restored: SubscriptionClock =
            serde_json::from_str(&serde_json::to_string(&clock).unwrap()).unwrap();
        for now in [300, 799, 800, 900, 1_600, 9_000] {
            let direct = restored.advance(now).unwrap();
            let intermediate = restored.advance(300).unwrap().advance(now).unwrap();
            assert_eq!(direct, intermediate);
            assert_eq!(direct.advance(now).unwrap(), direct);
        }
        assert_eq!(
            clock.advance(799).unwrap().advance(900).unwrap(),
            clock.advance(900).unwrap()
        );
    }

    #[test]
    fn exclusive_without_special_expires_to_none() {
        let clock = SubscriptionClock::new("0x0sky", 0)
            .unwrap()
            .grant(SubscriptionTier::Exclusive, 100, 0)
            .unwrap();
        assert!(clock.snapshot(100, 200).unwrap().entitlement.is_none());
    }

    #[test]
    fn invalid_inputs_fail_without_mutating_or_clamping_grants() {
        let clock = upgraded();
        assert_eq!(
            clock.advance(299),
            Err(SubscriptionClockError::ClockRollback)
        );
        assert_eq!(
            clock.grant(SubscriptionTier::Special, 0, 300),
            Err(SubscriptionClockError::EmptyGrant)
        );
        assert_eq!(
            clock.grant(SubscriptionTier::Special, u64::MAX, 300),
            Err(SubscriptionClockError::Overflow)
        );
        assert_eq!(
            clock.snapshot(300, 300),
            Err(SubscriptionClockError::InvalidFreshness)
        );
        assert_eq!(
            SubscriptionClock::new("invalid", 0),
            Err(SubscriptionClockError::InvalidOwner)
        );
        let boundary = SubscriptionClock::new("0x0sky", u64::MAX - 10)
            .unwrap()
            .grant(SubscriptionTier::Special, 10, u64::MAX - 10)
            .unwrap();
        assert_eq!(
            boundary.grant(SubscriptionTier::Exclusive, 1, u64::MAX - 10),
            Err(SubscriptionClockError::Overflow)
        );
        assert_eq!(
            boundary
                .advance(u64::MAX)
                .unwrap()
                .special_remaining_ms
                .get(),
            0
        );
        assert_eq!(clock, upgraded());
    }
}
